//! IPC 命令层：core API 的薄封装 + 托盘/快照副作用。
//!
//! 安全约定（红线 3）：`upsert_provider` 忽略前端传入的密文字段，
//! key 只走「写入专用」通道——`new_api_key` 非空加密落盘，空/缺省保留旧密文，
//! 明文 key 永不回传前端。

use std::collections::BTreeMap;

use quota_core::script::ScriptConfig;
use quota_core::template::{TemplateConfig, TemplateError};
use quota_core::{AppConfig, PlanVariant, ProviderEntry, ProviderKind, Vault};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::i18n::Lang;
use crate::settings::Settings;
use crate::snapshot::{SnapshotEntry, Snapshots};
use crate::state::{AppState, EntryState, ErrorInfo, QueryOutcome, now_ms};
use crate::tray;

/// 模板试查临时条目 id（AAD 绑定值，试查不落任何持久状态）。
const TEMPLATE_TEST_ID: &str = "template-test";
/// Provider 列表跨 WebView 失效事件（与前端 queries.ts 同名常量成对）。
const PROVIDERS_CHANGED_EVENT: &str = "providers-changed";
/// 条目重排事件（与前端 queries.ts 同名常量成对）：只失效列表缓存——
/// 各条目数据未变，派生缓存（查询/状态/历史/快照）不陪查。
const PROVIDERS_REORDERED_EVENT: &str = "providers-reordered";

/// 校验错误的定位信息（前端按字段高亮展示）。
#[derive(Debug, Clone, Serialize)]
pub struct TemplateErrorDto {
    pub field: String,
    pub reason: String,
}

/// 预置平台元信息（core 的 NativeMeta 未实现 Serialize，此处转 Dto）。
#[derive(Debug, Clone, Serialize)]
pub struct NativeMetaDto {
    pub id: String,
    pub name: String,
    /// 峰谷定价预置（平台无预置则 None；前端用于展示与一键填充）。
    pub pricing: Option<PresetPricingDto>,
    /// 按余额币种选择的预置套（当前仅 DeepSeek 同时提供 CNY/USD）。
    pub pricing_by_currency: BTreeMap<String, PresetPricingDto>,
    /// 是否支持套餐变体声明（智谱系订阅套餐：v1 无周限 / v2+ 有周限），
    /// 编辑表单据此决定是否展示变体选择。
    pub supports_plan_variant: bool,
    /// CLI 凭据型平台（订阅四家）：凭据在查询时从本机官方 CLI 的
    /// 登录文件只读获取——编辑表单隐藏 key 输入框并展示提示卡。
    pub uses_cli_credentials: bool,
    /// 双凭据 native 平台（当前仅阿里云余额）：api_key=AccessKey ID、
    /// api_key2=AccessKey Secret——编辑表单渲染必填语义的第二凭据槽。
    pub uses_api_key2: bool,
    /// 控制台直达预置 URL（条目自定义覆盖优先；None = 该平台无预置）。
    pub console_url: Option<String>,
    /// 配置文件中归属该 native id 的用户自定义模型库（只读透出）。
    pub custom_models: Vec<quota_core::CustomModelDef>,
}

/// 峰谷定价预置的 IPC 形状（core `PresetProvider` 的可序列化镜像）。
#[derive(Debug, Clone, Serialize)]
pub struct PresetPricingDto {
    pub currency: String,
    pub timezone_offset_minutes: i32,
    pub windows: Vec<quota_core::PeakWindow>,
    pub default_model: String,
    pub models: Vec<PresetModelDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresetModelDto {
    pub id: String,
    pub display: String,
    /// 计费模式（订阅项无三档价、窗口表达折扣时段，前端据此切换文案）。
    pub plan: quota_core::PlanKind,
    /// 模型级窗口覆盖（None = 继承平台级）。
    pub windows: Option<Vec<quota_core::PeakWindow>>,
    pub peak: quota_core::PriceTier,
    pub off_peak: quota_core::PriceTier,
}

impl PresetPricingDto {
    fn from_preset(p: &quota_core::pricing::PresetProvider) -> Self {
        Self {
            currency: p.currency.into(),
            timezone_offset_minutes: p.timezone_offset_minutes,
            windows: p.windows.clone(),
            default_model: p.default_model.into(),
            models: p
                .models
                .iter()
                .map(|m| PresetModelDto {
                    id: m.id.into(),
                    display: m.display.into(),
                    plan: m.plan,
                    windows: m.windows.clone(),
                    peak: m.peak.clone(),
                    off_peak: m.off_peak.clone(),
                })
                .collect(),
        }
    }
}

// ---- 纯逻辑（可单测） -----------------------------------------------------

/// key 写入策略：`new_api_key` 非空 → vault 加密写入；
/// 空/缺省 → 保留既有密文（红线 3 的「空值 = 保持不变」在 Rust 侧强制）。
/// 无论何种情况，前端传入的 `entry.api_key_enc` 都被忽略（防伪造/错位密文）。
pub fn apply_key_policy(
    entry: &mut ProviderEntry,
    existing: Option<&ProviderEntry>,
    new_api_key: Option<&str>,
    vault: &Vault,
    lang: Lang,
) -> Result<(), String> {
    let trimmed = new_api_key.map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        Some(key) => entry
            .set_api_key(vault, key)
            .map_err(|e| lang.err_encrypt_failed(&e)),
        None => {
            entry.api_key_enc = existing.and_then(|e| e.api_key_enc.clone());
            Ok(())
        }
    }
}

/// 第二凭据槽（{{apiKey2}}）的同款写入策略：非空加密写入，空/缺省保留
/// 既有密文；前端传入的 `api_key2_enc` 同样被忽略。
pub fn apply_key2_policy(
    entry: &mut ProviderEntry,
    existing: Option<&ProviderEntry>,
    new_api_key2: Option<&str>,
    vault: &Vault,
    lang: Lang,
) -> Result<(), String> {
    let trimmed = new_api_key2.map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        Some(key) => entry
            .set_api_key2(vault, key)
            .map_err(|e| lang.err_encrypt_failed(&e)),
        None => {
            entry.api_key2_enc = existing.and_then(|e| e.api_key2_enc.clone());
            Ok(())
        }
    }
}

/// 当前界面语言（按 settings.language 解析，system 折叠为具体语言）。
fn lang_of(state: &AppState) -> Lang {
    Lang::parse(&state.settings.read().unwrap().language)
}

/// 模板是否引用了 `{{apiKey}}`（决定试查时 key 是否必填）。
///
/// 委托 core `uses_api_key`——变量解析（含带空格写法）与 validate/执行期
/// 同一语义，避免本地字面量扫描漂移。
pub fn template_needs_api_key(config: &TemplateConfig) -> bool {
    quota_core::template::uses_api_key(config)
}

/// 模板是否引用了 `{{apiKey2}}`（决定试查时第二凭据是否必填）。
pub fn template_needs_api_key2(config: &TemplateConfig) -> bool {
    quota_core::template::uses_api_key2(config)
}

/// 结果表 → 快照（仅保留有成功数据的条目）。
fn snapshots_from_results(results: &std::collections::HashMap<String, EntryState>) -> Snapshots {
    let entries = results
        .iter()
        .filter_map(|(id, st)| {
            st.data.as_ref().map(|data| {
                (
                    id.clone(),
                    SnapshotEntry {
                        data: data.clone(),
                        at: st.at.unwrap_or_default(),
                    },
                )
            })
        })
        .collect();
    Snapshots { entries }
}

/// 状态变更后的统一收尾：快照落盘 + 托盘重建。
///
/// 快照写盘前按当前 config 过滤：删除/编辑条目时在途查询的迟到结果
/// 不会以孤儿身分落入 cache.json（托盘侧本就按 config 过滤，此处补齐
/// 存储一致性）；config 读盘失败时跳过过滤（保留现状）。
fn after_state_change(app: &AppHandle, state: &AppState) {
    let mut snaps = snapshots_from_results(&state.results.read().unwrap());
    if let Ok(cfg) = AppConfig::load(&state.paths.config()) {
        let live: std::collections::HashSet<&str> =
            cfg.providers.iter().map(|p| p.id.as_str()).collect();
        snaps.entries.retain(|id, _| live.contains(id.as_str()));
    }
    if let Err(e) = snaps.save(&state.paths.snapshot()) {
        eprintln!("快照写入失败：{e}");
    }
    tray::rebuild(app, state);
}

/// 广播单条目配置变更（payload 为条目 id）：前端按条目失效派生缓存，
/// 不再全量失效——其余条目的查询继续沿用各自轮询周期。
fn emit_providers_changed(app: &AppHandle, entry_id: &str) {
    if let Err(e) = app.emit(PROVIDERS_CHANGED_EVENT, entry_id) {
        eprintln!("Provider 变更事件发送失败：{e}");
    }
}

/// 保存前统一校验（upsert 用，纯函数可测）：id/name 非空、模板静态校验、
/// native id 存在性、峰谷定价配置校验（core validate，带字段定位）。
pub fn validate_entry(entry: &ProviderEntry, lang: Lang) -> Result<(), String> {
    if entry.id.trim().is_empty() || entry.name.trim().is_empty() {
        return Err(lang.err_id_name_empty());
    }
    match &entry.kind {
        ProviderKind::Template(t) => {
            quota_core::template::validate(t).map_err(|e| e.to_string())?;
        }
        ProviderKind::Script(s) => {
            quota_core::script::validate(s).map_err(|e| e.to_string())?;
        }
        ProviderKind::Native { provider } => {
            if quota_core::provider::find(provider).is_none() {
                return Err(lang.err_unknown_native(provider));
            }
        }
    }
    if let Some(p) = &entry.pricing {
        quota_core::pricing::validate(p).map_err(|e| lang.err_pricing_invalid(&e))?;
    }
    Ok(())
}

// ---- 命令 -----------------------------------------------------------------

fn export_configuration_at(
    config_path: &std::path::Path,
    export_path: &std::path::Path,
    vault: &Vault,
    history: Option<&[quota_core::HistoryExportRow]>,
    usage_comparison_series: Option<&[quota_core::UsageComparisonSeries]>,
) -> Result<(), String> {
    let config = AppConfig::load(config_path).map_err(|e| e.to_string())?;
    quota_core::export_config_to_path_with_usage(
        &config,
        vault,
        history,
        usage_comparison_series,
        export_path,
    )
    .map_err(|e| e.to_string())
}

fn import_configuration_at(
    export_path: &std::path::Path,
    config_path: &std::path::Path,
    vault: &Vault,
) -> Result<quota_core::TransferBundle, String> {
    quota_core::import_config_to_path(export_path, vault, config_path).map_err(|e| e.to_string())
}

#[cfg(any(target_os = "android", test))]
fn is_android_document_uri(path: &str) -> bool {
    path.starts_with("content://")
}

#[cfg(target_os = "android")]
fn android_transfer_temp(app: &AppHandle, operation: &str) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("无法定位迁移缓存目录：{e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建迁移缓存目录失败：{e}"))?;
    Ok(dir.join(format!(
        "transfer-{operation}-{}-{}.qtray-export",
        std::process::id(),
        now_ms()
    )))
}

#[cfg(target_os = "android")]
fn export_configuration_to_uri(
    app: &AppHandle,
    state: &AppState,
    uri: &str,
    history: Option<&[quota_core::HistoryExportRow]>,
    usage_comparison_series: Option<&[quota_core::UsageComparisonSeries]>,
) -> Result<(), String> {
    use std::io::Write;
    use tauri_plugin_fs::FsExt;

    let temp = android_transfer_temp(app, "export")?;
    let result = (|| {
        export_configuration_at(
            &state.paths.config(),
            &temp,
            &state.vault,
            history,
            usage_comparison_series,
        )?;
        let bytes = std::fs::read(&temp).map_err(|e| format!("读取迁移缓存失败：{e}"))?;
        let path = match uri.parse::<tauri_plugin_fs::FilePath>() {
            Ok(path) => path,
            Err(never) => match never {},
        };
        let mut options = tauri_plugin_fs::OpenOptions::new();
        options.write(true).truncate(true).create(true);
        let mut target = app
            .fs()
            .open(path, options)
            .map_err(|e| format!("打开 Android 导出文档失败：{e}"))?;
        target
            .write_all(&bytes)
            .and_then(|_| target.sync_all())
            .map_err(|e| format!("写入 Android 导出文档失败：{e}"))
    })();
    let _ = std::fs::remove_file(temp);
    result
}

#[cfg(target_os = "android")]
fn import_configuration_from_uri(
    app: &AppHandle,
    state: &AppState,
    uri: &str,
) -> Result<quota_core::TransferBundle, String> {
    use std::io::Read;
    use tauri_plugin_fs::FsExt;

    let temp = android_transfer_temp(app, "import")?;
    let result = (|| {
        let path = match uri.parse::<tauri_plugin_fs::FilePath>() {
            Ok(path) => path,
            Err(never) => match never {},
        };
        let mut options = tauri_plugin_fs::OpenOptions::new();
        options.read(true);
        let mut source = app
            .fs()
            .open(path, options)
            .map_err(|e| format!("打开 Android 导入文档失败：{e}"))?;
        let mut bytes = Vec::new();
        source
            .read_to_end(&mut bytes)
            .map_err(|e| format!("读取 Android 导入文档失败：{e}"))?;
        std::fs::write(&temp, bytes).map_err(|e| format!("写入迁移缓存失败：{e}"))?;
        import_configuration_at(&temp, &state.paths.config(), &state.vault)
    })();
    let _ = std::fs::remove_file(temp);
    result
}

/// 导出完整配置（含查询历史）到用户通过系统对话框选定的路径。
#[tauri::command]
pub fn export_configuration(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    // 历史读取失败降级为不带历史，导出主任务继续
    let history = match state.history.lock().unwrap().export_rows() {
        Ok(rows) => Some(rows),
        Err(e) => {
            eprintln!("导出携带历史失败（将不含历史数据）：{e}");
            None
        }
    };
    let usage_comparison_series = state
        .settings
        .read()
        .unwrap()
        .usage_comparison_series
        .clone();
    #[cfg(target_os = "android")]
    if is_android_document_uri(&path) {
        return export_configuration_to_uri(
            &app,
            &state,
            &path,
            history.as_deref(),
            usage_comparison_series.as_deref(),
        );
    }
    let _ = app;
    export_configuration_at(
        &state.paths.config(),
        std::path::Path::new(&path),
        &state.vault,
        history.as_deref(),
        usage_comparison_series.as_deref(),
    )
}

/// 从迁移包整体替换配置（历史幂等合并），清除旧查询快照并通知所有窗口刷新。
#[tauri::command]
pub fn import_configuration(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<usize, String> {
    #[cfg(target_os = "android")]
    let bundle = if is_android_document_uri(&path) {
        import_configuration_from_uri(&app, &state, &path)?
    } else {
        import_configuration_at(
            std::path::Path::new(&path),
            &state.paths.config(),
            &state.vault,
        )?
    };
    #[cfg(not(target_os = "android"))]
    let bundle = import_configuration_at(
        std::path::Path::new(&path),
        &state.paths.config(),
        &state.vault,
    )?;
    // 迁移包携带的历史行合并进本机历史库（配置已导入成功，失败仅告警）
    if let Some(rows) = &bundle.history
        && !rows.is_empty()
        && let Err(e) = state.history.lock().unwrap().merge_rows(rows)
    {
        eprintln!("导入历史合并失败：{e}");
    }
    if let Err(e) =
        persist_usage_comparison_settings(&state, bundle.usage_comparison_series.clone())
    {
        eprintln!("导入使用统计比较组合失败（配置已导入）：{e}");
    }
    state.results.write().unwrap().clear();
    after_state_change(&app, &state);
    let provider_count = bundle.config.providers.len();
    if let Err(e) = app.emit("configuration-imported", provider_count) {
        eprintln!("配置导入事件发送失败：{e}");
    }
    Ok(provider_count)
}

#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<ProviderEntry>, String> {
    AppConfig::load(&state.paths.config())
        .map(|cfg| cfg.providers)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    entry: ProviderEntry,
    new_api_key: Option<String>,
    new_api_key2: Option<String>,
) -> Result<(), String> {
    let lang = lang_of(&state);
    validate_entry(&entry, lang)?;
    let mut cfg = AppConfig::load(&state.paths.config()).map_err(|e| e.to_string())?;
    let existing = cfg.providers.iter().find(|p| p.id == entry.id).cloned();
    let mut entry = entry;
    apply_key_policy(
        &mut entry,
        existing.as_ref(),
        new_api_key.as_deref(),
        &state.vault,
        lang,
    )?;
    apply_key2_policy(
        &mut entry,
        existing.as_ref(),
        new_api_key2.as_deref(),
        &state.vault,
        lang,
    )?;

    // 编辑保位 / 新增追加（entry 随后 move 进配置，id 先行拷出）
    let entry_id = entry.id.clone();
    match cfg.providers.iter_mut().find(|p| p.id == entry.id) {
        Some(slot) => *slot = entry,
        None => cfg.providers.push(entry),
    }
    cfg.save(&state.paths.config()).map_err(|e| e.to_string())?;

    // 条目已变，作废该条目的旧查询结果（其他条目的 keep-last-good 数据与快照保留）
    state.results.write().unwrap().remove(&entry_id);
    after_state_change(&app, &state);
    // 携带条目 id 广播：主窗按条目失效查询缓存后由挂载的卡片即时重查
    // （卡片常挂载，观察者不随页签/窗口显隐消失），悬停面板经
    // provider-state-changed 回流。不在此处后端补查——前端失效驱动的
    // 查询与后端补查会并发双查同一平台 API。
    emit_providers_changed(&app, &entry_id);
    Ok(())
}

#[tauri::command]
pub fn remove_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let lang = lang_of(&state);
    let mut cfg = AppConfig::load(&state.paths.config()).map_err(|e| e.to_string())?;
    let before = cfg.providers.len();
    cfg.providers.retain(|p| p.id != id);
    if cfg.providers.len() == before {
        return Err(lang.err_entry_not_found(&id));
    }
    cfg.save(&state.paths.config()).map_err(|e| e.to_string())?;
    // 条目已删，历史随删（与快照孤儿过滤语义对齐）；失败仅告警
    if let Err(e) = state.history.lock().unwrap().clear(Some(&id)) {
        eprintln!("清除条目历史失败：{e}");
    }
    let next_series = prune_usage_comparisons_for_provider(
        state
            .settings
            .read()
            .unwrap()
            .usage_comparison_series
            .clone(),
        &id,
    );
    if let Err(e) = persist_usage_comparison_settings(&state, next_series) {
        eprintln!("清理已删除 Provider 的统计比较组合失败：{e}");
    }
    state.results.write().unwrap().remove(&id);
    after_state_change(&app, &state);
    emit_providers_changed(&app, &id);
    Ok(())
}

fn prune_usage_comparisons_for_provider(
    value: Option<Vec<quota_core::UsageComparisonSeries>>,
    provider_id: &str,
) -> Option<Vec<quota_core::UsageComparisonSeries>> {
    value.map(|mut items| {
        items.retain(|item| item.provider_id != provider_id);
        items
    })
}

/// 清空全部用户数据：供应商条目（含凭据密文）、峰谷定价、自定义模型
/// 库与查询历史。GUI 已在二级确认弹窗（5 秒倒数）取得显式确认；
/// settings 设备偏好与主密钥保留；统计比较组合随 Provider 一并清空。
/// 与 upsert/remove 同为同步写命令，主线程串行执行，load→save 之间
/// 无并发写命令插入（并发前提见 reorder_providers 注释）。
#[tauri::command]
pub fn clear_all_data(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut cfg = AppConfig::load(&state.paths.config()).map_err(|e| e.to_string())?;
    cfg.clear_user_data();
    cfg.save(&state.paths.config()).map_err(|e| e.to_string())?;
    if let Err(e) = persist_usage_comparison_settings(&state, Some(Vec::new())) {
        eprintln!("清空统计比较组合失败：{e}");
    }
    // 历史是用户显式要求删除的一部分。直接对磁盘库执行而非托管的
    // 降级实例——启动打开失败时 AppState 持内存库，clear 在其上必然
    // 成功但磁盘 history.db 原样保留，旧历史会在下次成功打开后复活；
    // 磁盘路径打开失败（文件锁等）则如实报错。失败不早退：收尾
    // （results/托盘/快照/事件）照常执行后把错误带出，避免主窗与
    // 托盘停留在半清理态；重试本命令幂等可补清
    let history_err = quota_core::HistoryStore::open(&state.paths.history())
        .and_then(|store| store.clear(None))
        .err()
        .map(|e| format!("查询历史清空失败：{e}"));
    state.results.write().unwrap().clear();
    state.last_peak.write().unwrap().clear();
    // 快照过滤后为空、托盘以无数据状态重建
    after_state_change(&app, &state);
    // 广播配置级变更（与导入同语义）：主窗与悬停面板（独立 WebView，
    // snapshots 缓存 staleTime Infinity）各自全量失效——仅靠调用方
    // invalidateQueries 管不到悬停面板
    if let Err(e) = app.emit("configuration-imported", 0) {
        eprintln!("清空事件发送失败：{e}");
    }
    match history_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// 重排校验（纯函数可测）：ids 必须与现有条目集合完全一致且无重复——
/// 拖拽期间的并发增删会让集合失配，此时拒绝落盘，由前端 refetch 恢复。
fn validate_reorder_ids(existing_ids: &[&str], ids: &[String]) -> bool {
    let mut seen = std::collections::HashSet::new();
    ids.iter().all(|id| seen.insert(id.as_str()))
        && seen.len() == existing_ids.len()
        && existing_ids.iter().all(|id| seen.contains(id))
}

/// 按传入 id 顺序原地重排（纯函数可测；调用方已通过校验）。
/// sort_by_key 稳定排序，等价于把每个条目挪到其 id 在 ids 中的位次。
fn reorder_providers_in_place(providers: &mut [ProviderEntry], ids: &[String]) {
    providers.sort_by_key(|p| ids.iter().position(|id| *id == p.id));
}

/// 按前端给定的完整 id 顺序重排条目（卡片拖拽排序落库）。
/// 顺序影响托盘图标回退（未指定图标时取第一个启用条目）与图标子菜单序，
/// 必须走 after_state_change 重建托盘；各条目数据未变，不动 results/历史。
///
/// 并发前提：本命令与 upsert/remove 同为同步 fn，Tauri 主线程串行执行，
/// load→save 之间不会被另一配置写命令插入；若未来任一写命令改为 async
/// 挪入线程池，需先引入跨命令的配置写锁，否则出现丢失更新竞态。
#[tauri::command]
pub fn reorder_providers(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<(), String> {
    let lang = lang_of(&state);
    let mut cfg = AppConfig::load(&state.paths.config()).map_err(|e| e.to_string())?;
    let existing: Vec<&str> = cfg.providers.iter().map(|p| p.id.as_str()).collect();
    if !validate_reorder_ids(&existing, &ids) {
        return Err(lang.err_reorder_mismatch());
    }
    reorder_providers_in_place(&mut cfg.providers, &ids);
    cfg.save(&state.paths.config()).map_err(|e| e.to_string())?;
    after_state_change(&app, &state);
    if let Err(e) = app.emit(PROVIDERS_REORDERED_EVENT, ()) {
        eprintln!("Provider 重排事件发送失败：{e}");
    }
    Ok(())
}

fn native_meta_dtos(cfg: &AppConfig) -> Vec<NativeMetaDto> {
    quota_core::provider::metas()
        .into_iter()
        .map(|m| {
            let pricing =
                quota_core::pricing::preset(m.id).map(|p| PresetPricingDto::from_preset(&p));
            let pricing_by_currency = if m.id == "deepseek" {
                ["CNY", "USD"]
                    .into_iter()
                    .filter_map(|currency| {
                        quota_core::pricing::preset_with_currency(m.id, currency)
                            .map(|preset| (currency.into(), PresetPricingDto::from_preset(&preset)))
                    })
                    .collect()
            } else {
                BTreeMap::new()
            };
            NativeMetaDto {
                id: m.id.into(),
                name: m.name.into(),
                supports_plan_variant: quota_core::provider::supports_plan_variant(m.id),
                uses_cli_credentials: quota_core::provider::uses_cli_credentials(m.id),
                uses_api_key2: quota_core::provider::uses_api_key2(m.id),
                console_url: m.console_url.map(str::to_string),
                pricing,
                pricing_by_currency,
                custom_models: cfg.custom_models.get(m.id).cloned().unwrap_or_default(),
            }
        })
        .collect()
}

#[tauri::command]
pub fn list_native_metas(state: State<'_, AppState>) -> Result<Vec<NativeMetaDto>, String> {
    AppConfig::load(&state.paths.config())
        .map(|cfg| native_meta_dtos(&cfg))
        .map_err(|e| e.to_string())
}

/// 静态校验模板 JSON 文本（结构错误与校验错误统一为字段定位 Dto）。
#[tauri::command]
pub fn validate_template(config_json: String) -> Result<(), TemplateErrorDto> {
    let config: TemplateConfig =
        serde_json::from_str(&config_json).map_err(|e| TemplateErrorDto {
            field: "(json)".into(),
            reason: e.to_string(),
        })?;
    quota_core::template::validate(&config).map_err(|e| match e {
        TemplateError::Validation { field, reason } => TemplateErrorDto { field, reason },
    })
}

/// 模板试查：真实走一次完整查询链路（vault 加密 → 引擎 → HTTP），不落持久状态。
#[tauri::command]
pub async fn test_template(
    state: State<'_, AppState>,
    config_json: String,
    api_key: Option<String>,
    api_key2: Option<String>,
    base_url: Option<String>,
) -> Result<QueryOutcome, String> {
    let lang = lang_of(&state);
    let config: TemplateConfig =
        serde_json::from_str(&config_json).map_err(|e| lang.err_template_json(&e))?;
    let key = match api_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(k) => k.to_string(),
        None if template_needs_api_key(&config) => {
            return Ok(QueryOutcome {
                ok: false,
                data: None,
                error: Some(ErrorInfo {
                    kind: "deterministic".into(),
                    message: lang.err_template_needs_key(),
                    detail: None,
                }),
                at: None,
            });
        }
        None => "-".into(),
    };
    // 第二凭据槽：引用 {{apiKey2}} 而未填 → 同款确定性引导（缺省占位 "-"
    // 长度 < 4 不进脱敏收集，与主 key 一致）
    let key2 = match api_key2.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(k) => Some(k.to_string()),
        None if template_needs_api_key2(&config) => {
            return Ok(QueryOutcome {
                ok: false,
                data: None,
                error: Some(ErrorInfo {
                    kind: "deterministic".into(),
                    message: lang.err_template_needs_key2(),
                    detail: None,
                }),
                at: None,
            });
        }
        None => None,
    };

    let mut entry = ProviderEntry {
        id: TEMPLATE_TEST_ID.into(),
        name: "模板试查".into(),
        kind: ProviderKind::Template(Box::new(config)),
        enabled: true,
        api_key_enc: None,
        api_key2_enc: None,
        base_url,
        pricing: None,
        plan_variant: PlanVariant::Auto,
        use_proxy: false,
        console_url: None,
    };
    entry
        .set_api_key(&state.vault, &key)
        .map_err(|e| lang.err_encrypt_failed(&e))?;
    if let Some(k2) = key2 {
        entry
            .set_api_key2(&state.vault, &k2)
            .map_err(|e| lang.err_encrypt_failed(&e))?;
    }
    entry.base_url = entry.base_url.filter(|u| !u.trim().is_empty());

    // clone（Arc 浅拷贝）后立即释放读锁，避免 guard 跨 await 破坏 Send
    let engine = state.engine.read().unwrap().clone();
    let outcome = match engine.query(&state.vault, &entry).await {
        Ok(data) => QueryOutcome {
            ok: true,
            data: Some(data),
            error: None,
            at: Some(now_ms()),
        },
        Err(e) => QueryOutcome {
            ok: false,
            data: None,
            error: Some(ErrorInfo::from_query_error(&e)),
            at: None,
        },
    };
    Ok(outcome)
}

/// 脚本试查用临时条目 id（不落盘，仅构造引擎入参）。
const SCRIPT_TEST_ID: &str = "script-test";

/// 静态校验脚本配置 JSON（干跑：假变量替换 + request() 产物形状）。
#[tauri::command]
pub fn validate_script(config_json: String) -> Result<(), TemplateErrorDto> {
    let config: ScriptConfig =
        serde_json::from_str(&config_json).map_err(|e| TemplateErrorDto {
            field: "(json)".into(),
            reason: e.to_string(),
        })?;
    quota_core::script::validate(&config).map_err(|e| TemplateErrorDto {
        field: e.field,
        reason: e.reason,
    })
}

/// 脚本试查：真实走一次完整查询链路（vault 加密 → 沙箱 → 引擎 → HTTP），
/// 不落持久状态。镜像 test_template 的 key 缺省语义。
#[tauri::command]
pub async fn test_script(
    state: State<'_, AppState>,
    config_json: String,
    api_key: Option<String>,
    api_key2: Option<String>,
    base_url: Option<String>,
) -> Result<QueryOutcome, String> {
    let lang = lang_of(&state);
    let config: ScriptConfig =
        serde_json::from_str(&config_json).map_err(|e| lang.err_template_json(&e))?;
    let key = match api_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(k) => k.to_string(),
        None if quota_core::script::uses_api_key(&config) => {
            return Ok(QueryOutcome {
                ok: false,
                data: None,
                error: Some(ErrorInfo {
                    kind: "deterministic".into(),
                    message: lang.err_template_needs_key(),
                    detail: None,
                }),
                at: None,
            });
        }
        None => "-".into(),
    };
    let key2 = match api_key2.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(k) => Some(k.to_string()),
        None if quota_core::script::uses_api_key2(&config) => {
            return Ok(QueryOutcome {
                ok: false,
                data: None,
                error: Some(ErrorInfo {
                    kind: "deterministic".into(),
                    message: lang.err_template_needs_key2(),
                    detail: None,
                }),
                at: None,
            });
        }
        None => None,
    };

    let mut entry = ProviderEntry {
        id: SCRIPT_TEST_ID.into(),
        name: "脚本试查".into(),
        kind: ProviderKind::Script(Box::new(config)),
        enabled: true,
        api_key_enc: None,
        api_key2_enc: None,
        base_url,
        pricing: None,
        plan_variant: PlanVariant::Auto,
        use_proxy: false,
        console_url: None,
    };
    entry
        .set_api_key(&state.vault, &key)
        .map_err(|e| lang.err_encrypt_failed(&e))?;
    if let Some(k2) = key2 {
        entry
            .set_api_key2(&state.vault, &k2)
            .map_err(|e| lang.err_encrypt_failed(&e))?;
    }
    entry.base_url = entry.base_url.filter(|u| !u.trim().is_empty());

    // clone（Arc 浅拷贝）后立即释放读锁，避免 guard 跨 await 破坏 Send
    let engine = state.engine.read().unwrap().clone();
    let outcome = match engine.query(&state.vault, &entry).await {
        Ok(data) => QueryOutcome {
            ok: true,
            data: Some(data),
            error: None,
            at: Some(now_ms()),
        },
        Err(e) => QueryOutcome {
            ok: false,
            data: None,
            error: Some(ErrorInfo::from_query_error(&e)),
            at: None,
        },
    };
    Ok(outcome)
}

/// 「低余额提醒」事件负载（两端共用）：查询成功且任一窗口已用百分比
/// 达到设置阈值时随查询结果广播；前端消息中心按 provider_id 去重入列。
#[derive(Clone, Serialize)]
struct LowBalanceEvent<'a> {
    provider_id: &'a str,
    name: &'a str,
    /// 已用百分比（0-100，取数据中最高的窗口；前端展示时取整）。
    percent: f64,
}

/// 低余额判定（纯函数）：任一窗口已用百分比 ≥ 阈值时返回
/// `Some(最高达标百分比)`，否则 None。百分比语义与前端卡片高亮共用
/// core [`quota_core::used_percent`]（`display.ts usedPercent` 的镜像）。
pub(crate) fn low_balance_breach(
    data: &[quota_core::UsageData],
    threshold_percent: u8,
) -> Option<f64> {
    data.iter()
        .filter_map(quota_core::used_percent)
        .filter(|p| *p >= f64::from(threshold_percent))
        .fold(None::<f64>, |acc, p| Some(acc.map_or(p, |m| m.max(p))))
}

/// 低余额提醒的边沿触发判定（纯函数）：「已用 ≥ 阈值」是持续状态而非
/// 事件，直接广播会随轮询周期重复打扰（应用后台时即重复系统通知）。
/// 四态见枚举成员；回落与数据不足（breach=None）同途清除登记——数据
/// 恢复且仍达标会重新提醒一次，可接受。与 update-available/ready 的
/// 会话防重同口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LowBalanceEdge {
    /// 首次达标：广播事件并按平台补发系统通知，随后登记条目 id。
    Notify,
    /// 持续达标（已登记）或未达标且无登记：不打扰。
    Silent,
    /// 曾达标现回落/数据不足：清除登记（下次达标重新提醒），不广播。
    Reset,
}

pub(crate) fn low_balance_edge(previously_notified: bool, breach: Option<f64>) -> LowBalanceEdge {
    match (previously_notified, breach) {
        (false, Some(_)) => LowBalanceEdge::Notify,
        (true, Some(_)) => LowBalanceEdge::Silent,
        (true, None) => LowBalanceEdge::Reset,
        (false, None) => LowBalanceEdge::Silent,
    }
}

/// 查询单条目并落入共享结果表：成功更新结果与快照并重建托盘；失败按
/// 双轨分类透出，结果表保留最后一次成功数据（keep-last-good 数据源）。
/// 完成后广播 provider-state-changed（悬停面板等只读视图回流）。
async fn refetch_and_store(app: &AppHandle, id: String) -> Result<QueryOutcome, String> {
    let state = app.state::<AppState>();
    let lang = lang_of(&state);
    let cfg = AppConfig::load(&state.paths.config()).map_err(|e| e.to_string())?;
    let entry = cfg
        .providers
        .iter()
        .find(|p| p.id == id && p.enabled)
        .ok_or_else(|| lang.err_entry_not_enabled(&id))?
        .clone();

    if let ProviderKind::Native { provider } = &entry.kind
        && mobile_cli_provider_blocked(std::env::consts::OS, provider)
    {
        let info = ErrorInfo {
            kind: "deterministic".into(),
            message: lang.err_mobile_cli_credentials(),
            detail: None,
        };
        let outcome = {
            let mut results = state.results.write().unwrap();
            let stored = results.entry(id.clone()).or_default();
            stored.error = Some(info.clone());
            QueryOutcome {
                ok: false,
                data: stored.data.clone(),
                error: Some(info),
                at: stored.at,
            }
        };
        after_state_change(app, &state);
        let _ = app.emit("provider-state-changed", &id);
        return Ok(outcome);
    }

    // 代理端口对账自愈：条目开代理而运行态端口丢失（启动加载抖动回退
    // 默认值等）时从磁盘恢复，避免"未配置代理端口"引导错误假阳性。
    // 正常态（内存有端口）仅一次读锁即返回，无磁盘开销。
    if entry.use_proxy {
        crate::state::reconcile_proxy_from_disk(&state);
    }

    let engine = state.engine.read().unwrap().clone();
    let result = engine.query(&state.vault, &entry).await;
    // 在途查询迟到复核：查询期间条目可能已被删除/清空（clear/remove 是
    // 同步写命令，本函数是 async 池线程，load→save 窗口外仍有网络在途
    // 窗口）——迟到结果不得写回，否则孤儿结果残留 results 且向已清空
    // 的历史库写入新行（擦除承诺被打破）。复核含 enabled：禁用条目的
    // 结果同样不该写回。config 读失败（IO 抖动）时宁可放行写入——
    // 条目大概率仍在，丢正常数据比残留风险更重
    let still_live = AppConfig::load(&state.paths.config())
        .map(|cfg| cfg.providers.iter().any(|p| p.id == id && p.enabled))
        .unwrap_or(true);
    if !still_live {
        return Err(format!("条目已删除或已禁用，查询结果丢弃：{id}"));
    }
    let outcome = {
        let mut results = state.results.write().unwrap();
        match result {
            Ok(data) => {
                let at = now_ms();
                results.insert(
                    id.clone(),
                    EntryState {
                        data: Some(data.clone()),
                        at: Some(at),
                        error: None,
                    },
                );
                QueryOutcome {
                    ok: true,
                    data: Some(data),
                    error: None,
                    at: Some(at),
                }
            }
            Err(e) => {
                let info = ErrorInfo::from_query_error(&e);
                let st = results.entry(id.clone()).or_default();
                st.error = Some(info.clone());
                QueryOutcome {
                    ok: false,
                    data: st.data.clone(),
                    error: Some(info),
                    at: st.at,
                }
            }
        }
    };
    // M5：成功查询写入历史库（非关键数据，失败仅告警不阻断主链路；
    // 在结果表写锁外执行，避免锁内做磁盘 IO）
    if outcome.ok
        && let (Some(data), Some(at)) = (outcome.data.as_ref(), outcome.at)
        && let Err(e) = state.history.lock().unwrap().record(&id, data, at)
    {
        eprintln!("历史记录写入失败：{e}");
    }
    // 低余额提醒（两端共用）：成功查询后按设置阈值边沿触发——首次达标
    // 广播 low-balance 并按平台补发系统通知（Android 后台走
    // notify_background，桌面主窗不可见走 notify_desktop）；持续达标静默，
    // 回落/数据不足清除登记（下次达标重新提醒）。前端消息中心另按条目
    // id 去重入列，重复广播不叠加。
    if outcome.ok
        && let Some(data) = outcome.data.as_ref()
    {
        let threshold = state.settings.read().unwrap().low_balance_threshold_percent;
        let breach = low_balance_breach(data, threshold);
        let notify = {
            let mut notified = crate::state::LOW_BALANCE_NOTIFIED.lock().unwrap();
            match low_balance_edge(notified.contains(&id), breach) {
                LowBalanceEdge::Notify => {
                    notified.insert(id.clone());
                    true
                }
                LowBalanceEdge::Reset => {
                    notified.remove(&id);
                    false
                }
                LowBalanceEdge::Silent => false,
            }
        };
        if notify && let Some(percent) = breach {
            let _ = app.emit(
                "low-balance",
                LowBalanceEvent {
                    provider_id: &id,
                    name: &entry.name,
                    percent,
                },
            );
            let lang = lang_of(&state);
            let title = lang.low_balance_notify_title();
            let body = lang.low_balance_notify_body(&entry.name, percent.round() as u32);
            #[cfg(target_os = "android")]
            crate::update_ctl::notify_background(app, &state, &title, &body);
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            crate::update_ctl::notify_desktop(app, &state, &title, &body);
        }
    }
    after_state_change(app, &state);
    let _ = app.emit("provider-state-changed", &id);
    Ok(outcome)
}

/// 查询单条目（IPC 命令入口）：实现在 [`refetch_and_store`]。
#[tauri::command]
pub async fn query_provider(app: AppHandle, id: String) -> Result<QueryOutcome, String> {
    refetch_and_store(&app, id).await
}

/// 读取结果表中的单条当前状态，不发起网络请求。
/// 悬停面板与主窗口共享后端结果，避免两个 WebView 对同一平台重复查询。
#[tauri::command]
pub fn get_provider_state(state: State<'_, AppState>, id: String) -> QueryOutcome {
    let results = state.results.read().unwrap();
    match results.get(&id) {
        Some(entry) => QueryOutcome {
            ok: entry.error.is_none(),
            data: entry.data.clone(),
            error: entry.error.clone(),
            at: entry.at,
        },
        None => QueryOutcome {
            ok: true,
            data: None,
            error: None,
            at: None,
        },
    }
}

fn get_history_from_store(
    store: &quota_core::HistoryStore,
    id: &str,
    from_ms: u64,
) -> Result<Vec<quota_core::HistoryPoint>, String> {
    store.range(id, from_ms).map_err(|e| e.to_string())
}

/// 读取单条 Provider 自指定时刻起的全部历史窗口，不触发平台查询。
#[tauri::command]
pub fn get_history(
    state: State<'_, AppState>,
    id: String,
    from_ms: u64,
) -> Result<Vec<quota_core::HistoryPoint>, String> {
    get_history_from_store(&state.history.lock().unwrap(), &id, from_ms)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.read().unwrap().clone()
}

/// 设置局部更新的 IPC 形状：仅覆盖提交的字段（外层 Some），其余保持
/// 后端现值。供标题栏快切主题/语言、悬停面板切换图标源等单字段入口
/// 使用——前端不再基于可能陈旧的缓存做全量提交（历史 bug：陈旧缓存
/// 把代理端口等设置整体抹回默认值）。
///
/// `tray_icon_entry_id` / `update_proxy_port` / `update_proxy_host` 为
/// 双层 Option：外层 Some 内层 None = 显式清空（恢复自动/不代理）；
/// 字段缺省 = 不动。三者标注 `double_option`——serde 对
/// `Option<Option<T>>` 的默认行为会把 JSON null 也折叠为外层 None
/// （清空与不动不可区分）。
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct SettingsPatch {
    pub refresh_interval_minutes: Option<u32>,
    pub low_balance_threshold_percent: Option<u8>,
    pub autostart: Option<bool>,
    pub language: Option<String>,
    pub theme: Option<String>,
    pub ring_units_per_circle: Option<f64>,
    #[serde(default, with = "double_option")]
    pub tray_icon_entry_id: Option<Option<String>>,
    pub update_check_enabled: Option<bool>,
    #[serde(default, with = "double_option")]
    pub update_proxy_port: Option<Option<u16>>,
    #[serde(default, with = "double_option")]
    pub update_proxy_host: Option<Option<String>>,
    pub update_auto_download: Option<bool>,
    pub notifications_enabled: Option<bool>,
    pub background_refresh_enabled: Option<bool>,
    pub background_refresh_interval_minutes: Option<u32>,
    pub usage_comparison_series: Option<Vec<quota_core::UsageComparisonSeries>>,
}

/// 双层 Option 反序列化：委托内层 `Option<T>` 的标准反序列化——
/// JSON null 得内层 None（显式清空），有值得内层 Some，类型错误正常
/// 透出；外层再包 Some。字段缺省不进此函数（serde default 路径）。
mod double_option {
    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: serde::Deserialize<'de>,
        D: serde::Deserializer<'de>,
    {
        <Option<T> as serde::Deserialize>::deserialize(deserializer).map(Some)
    }
}

/// 应用 patch 到现值：None 字段不动，Some 覆盖（纯函数，可单测）。
pub fn apply_settings_patch(base: &mut Settings, patch: &SettingsPatch) {
    if let Some(v) = patch.refresh_interval_minutes {
        base.refresh_interval_minutes = v;
    }
    if let Some(v) = patch.low_balance_threshold_percent {
        base.low_balance_threshold_percent = v;
    }
    if let Some(v) = patch.autostart {
        base.autostart = v;
    }
    if let Some(v) = patch.language.clone() {
        base.language = v;
    }
    if let Some(v) = patch.theme.clone() {
        base.theme = v;
    }
    if let Some(v) = patch.ring_units_per_circle {
        base.ring_units_per_circle = v;
    }
    if let Some(v) = patch.tray_icon_entry_id.clone() {
        base.tray_icon_entry_id = v;
    }
    if let Some(v) = patch.update_check_enabled {
        base.update_check_enabled = v;
    }
    if let Some(v) = patch.update_proxy_port {
        base.update_proxy_port = v;
    }
    if let Some(v) = patch.update_proxy_host.clone() {
        base.update_proxy_host = v;
    }
    if let Some(v) = patch.update_auto_download {
        base.update_auto_download = v;
    }
    if let Some(v) = patch.notifications_enabled {
        base.notifications_enabled = v;
    }
    if let Some(v) = patch.background_refresh_enabled {
        base.background_refresh_enabled = v;
    }
    if let Some(v) = patch.background_refresh_interval_minutes {
        base.background_refresh_interval_minutes = v;
    }
    if let Some(v) = patch.usage_comparison_series.clone() {
        base.usage_comparison_series = Some(v);
    }
}

fn persist_usage_comparison_settings(
    state: &AppState,
    value: Option<Vec<quota_core::UsageComparisonSeries>>,
) -> Result<(), String> {
    let mut settings = state.settings.read().unwrap().clone();
    settings.usage_comparison_series = value;
    settings.sanitize();
    let lang = Lang::parse(&settings.language);
    settings
        .save(&state.paths.settings())
        .map_err(|e| lang.err_settings_save(&e))?;
    *state.settings.write().unwrap() = settings;
    Ok(())
}

/// 设置落盘的共用副作用核心（save_settings / patch_settings 共用）。
/// 顺序约定：磁盘为权威状态——
/// 1. 先落盘（失败则内存不动，前端展示错误，三方一致）；
/// 2. 落盘成功后同步内存；
/// 3. 托盘按新阈值重建（阈值变更即时反映，不受后续自启失败影响）；
/// 4. 自启系统注册失败：回滚磁盘与内存的 autostart 意图为旧值（保证
///    重按「保存」会真正重试注册，而非跳过比较后假成功），其余设置保留。
fn persist_settings(
    app: &AppHandle,
    state: &AppState,
    mut settings: Settings,
) -> Result<(), String> {
    let lang = Lang::parse(&settings.language);
    settings.sanitize();
    let old_autostart = state.settings.read().unwrap().autostart;

    let (old_proxy_port, old_proxy_host) = {
        let s = state.settings.read().unwrap();
        (s.update_proxy_port, s.update_proxy_host.clone())
    };
    settings
        .save(&state.paths.settings())
        .map_err(|e| lang.err_settings_save(&e))?;
    *state.settings.write().unwrap() = settings.clone();
    // Android：后台刷新调度随设置落盘即时同步（开关/周期变更生效，
    // UPDATE 策略见 background.rs；失败仅日志，不影响保存结果）。
    // 置于 autostart 分支之前——其失败路径 return Err 提前退出，后置
    // 会让已落盘的后台刷新开关等下次保存/冷启动才同步调度
    #[cfg(target_os = "android")]
    crate::background::schedule_background_work(state);
    tray::rebuild(app, state); // 阈值/语言/主题/每圈单位变化即时反映
    // 网络代理（主机/端口）变更即时生效：热重建查询引擎（读锁内查询
    // 继续用旧客户端跑完，写锁仅在换新实例的瞬间持有）
    if (old_proxy_port != settings.update_proxy_port
        || old_proxy_host != settings.update_proxy_host)
        && let Err(e) = crate::state::rebuild_engine(state)
    {
        eprintln!("代理变更后重建查询引擎失败：{e}");
    }

    if old_autostart != settings.autostart {
        // 便携形态禁止自启动：注册表项指向 U 盘路径会在介质移除后
        // 残留为无效启动项；前端体验层禁用之外的后端硬门禁
        if settings.autostart && state.mode.is_portable() {
            return Err(lang.err_autostart_portable());
        }
        if let Err(e) = apply_autostart(app, settings.autostart, lang) {
            // 回滚 autostart 意图（磁盘 + 内存）：保持「重按保存即重试」语义
            settings.autostart = old_autostart;
            if let Err(io) = settings.save(&state.paths.settings()) {
                eprintln!("自启失败后回滚 settings.json 失败：{io}");
            }
            *state.settings.write().unwrap() = settings;
            return Err(lang.err_autostart_apply(&e));
        }
    }
    Ok(())
}

/// GUI 全量设置表单不编辑 CLI 可旁路写入的字段；保存前以磁盘值覆盖陈旧前端值，
/// 避免常驻 GUI 在 CLI 导入后把比较组合或更新节流时间戳写回旧状态。
fn merge_cli_owned_settings(incoming: &mut Settings, disk: &Settings) {
    incoming.update_last_check = disk.update_last_check;
    incoming.usage_comparison_series = disk.usage_comparison_series.clone();
}

/// 全量保存设置（设置对话框「保存」按钮）。
#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: Settings,
) -> Result<(), String> {
    let disk = Settings::load(&state.paths.settings());
    merge_cli_owned_settings(&mut settings, &disk);
    persist_settings(&app, &state, settings)
}

/// 局部更新设置：后端读现值 → 应用 patch → 走统一的落盘副作用。
#[tauri::command]
pub fn patch_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<(), String> {
    // 磁盘是跨进程权威状态：CLI 可能在 GUI 常驻期间更新比较组合或节流时间戳。
    let mut current = Settings::load(&state.paths.settings());
    apply_settings_patch(&mut current, &patch);
    persist_settings(&app, &state, current)
}

/// 前端推送解析后的实际主题（theme context 解析三态的结果）。
///
/// 为什么走前端推送而非 Rust 侧监听 `WindowEvent::ThemeChanged`：
/// 该事件反映窗口当前主题，在前端 `setTheme` 强制 light/dark 后不再随
/// 系统变化，语义与「托盘圆环该用什么配色」不完全一致；前端 matchMedia
/// 是 system 跟随的统一真源（主动设置与系统变化都汇入同一路径），
/// 实现成本与跨平台一致性均更优。主题推送若缺失，托盘停留浅色
/// （`AppState::resolved_theme` 初始 false），影响仅限图标配色。
#[tauri::command]
pub fn set_resolved_theme(
    app: AppHandle,
    state: State<'_, AppState>,
    theme: String,
) -> Result<(), String> {
    let dark = match theme.as_str() {
        "dark" => true,
        "light" => false,
        _ => return Ok(()), // 未知值忽略（前端契约内不出现）
    };
    let changed = {
        let mut guard = state.resolved_theme.write().unwrap();
        let changed = *guard != dark;
        *guard = dark;
        changed
    }; // 写锁先释放——rebuild 内部要读同字段
    if changed {
        tray::rebuild(&app, &state);
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn apply_autostart(app: &AppHandle, enable: bool, lang: Lang) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    if enable {
        autolaunch
            .enable()
            .map_err(|e| lang.err_autostart_toggle(true, &e))
    } else {
        autolaunch
            .disable()
            .map_err(|e| lang.err_autostart_toggle(false, &e))
    }
}

/// 启动渲染用快照（与 cache.json 同形状）。
#[tauri::command]
pub fn get_snapshots(
    state: State<'_, AppState>,
) -> Result<BTreeMap<String, SnapshotEntry>, String> {
    Ok(snapshots_from_results(&state.results.read().unwrap()).entries)
}

// ---- 便携启动门控（BootGate） ----------------------------------------------

/// 启动状态：前端首屏查询——ready=false 时渲染便携首启安全确认页。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootStateDto {
    pub ready: bool,
    /// 便携首启待确认（ready=false 时的唯一成因）。
    pub pending_portable_init: bool,
    /// 前端壳层平台；Android 必须使用触摸优先交互，不能依赖 hover。
    pub platform: &'static str,
}

fn runtime_platform_for(target_os: &str) -> &'static str {
    if target_os == "android" {
        "android"
    } else {
        "desktop"
    }
}

pub(crate) fn mobile_cli_provider_blocked(target_os: &str, provider_id: &str) -> bool {
    target_os == "android" && quota_core::provider::uses_cli_credentials(provider_id)
}

fn desktop_update_commands_supported(target_os: &str) -> bool {
    !matches!(target_os, "android" | "ios")
}

/// 更新「检测」命令的支持面：桌面全家 + Android（2026-08-29 手动检测
/// 口径——进更新页自动检一次 + 手动按钮，无调度器）。与
/// [`desktop_update_commands_supported`]（安装/打开目录，永久桌面）分层。
fn update_check_supported(target_os: &str) -> bool {
    target_os != "ios"
}

fn ensure_desktop_update_commands(lang: Lang) -> Result<(), String> {
    if desktop_update_commands_supported(std::env::consts::OS) {
        Ok(())
    } else {
        Err(lang.err_mobile_update_unsupported())
    }
}

fn ensure_update_check_supported(lang: Lang) -> Result<(), String> {
    if update_check_supported(std::env::consts::OS) {
        Ok(())
    } else {
        Err(lang.err_mobile_update_unsupported())
    }
}

#[tauri::command]
pub fn get_boot_state(app: AppHandle) -> BootStateDto {
    // AppState 已托管 = 启动完成；BootGate.pending 有值 = 待确认
    let ready = app.try_state::<crate::state::AppState>().is_some();
    let pending = app
        .try_state::<crate::state::BootGate>()
        .is_some_and(|g| g.pending.lock().unwrap().is_some());
    BootStateDto {
        ready,
        pending_portable_init: pending,
        platform: runtime_platform_for(std::env::consts::OS),
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn apply_autostart(_app: &AppHandle, _enable: bool, lang: Lang) -> Result<(), String> {
    Err(lang.err_mobile_autostart_unsupported())
}

/// 便携首启确认：创建主密钥（用户已在 Web 确认页显式接受固定安全
/// 提示）→ 补齐 AppState/托盘/悬停窗/调度器。
///
/// 并发契约：`BootGate.pending` 以锁内 take 作一次性认领——并发的
/// 第二个调用拿不到 mode 且 AppState 未就绪时静默 Ok（跟随第一个
/// 调用的结果，杜绝双装配：setup_surfaces 二次执行会因固定窗口
/// label 冲突弹「初始化失败」误伤成功初始化）；认领后中途失败则
/// 回填 pending 允许重试。
///
/// 必须 async：Windows 上 WebView2 IPC 回调在主线程触发，同步命令
/// 即在主线程 IPC 调用栈内执行，且 run_on_main_thread 对主线程调用方
/// 是**同步直执**而非异步入队（tauri-runtime-wry send_user_message 的
/// 主线程分支）——栈内同步建 WebView2 会等一个需要主线程泵消息才能
/// 完成的初始化，自等待死锁（P0：确认后按钮灰死）。async 命令跑在
/// 线程池，run_on_main_thread 变为真正异步入队，主线程退栈后执行。
#[tauri::command]
pub async fn confirm_portable_init(app: AppHandle) -> Result<(), String> {
    if app.try_state::<crate::state::AppState>().is_some() {
        return Ok(());
    }
    let gate = app
        .try_state::<crate::state::BootGate>()
        .ok_or("启动门控不存在：非便携首启场景")?;
    let mode = match gate.pending.lock().unwrap().take() {
        Some(mode) => mode,
        None => {
            // 认领失败：pending=None 且 AppState 未就绪（开头幂等分支
            // 未命中）只可能是另一并发确认 in-flight——静默跟随其结果。
            // 成功则 AppState 随后可见；失败则 pending 已回填可重试
            return Ok(());
        }
    };
    let result = confirm_portable_init_claimed(&app, &mode);
    if result.is_err() {
        // 回填门控：失败可重试（重试走完整流程，幂等分支此时不命中）
        *gate.pending.lock().unwrap() = Some(mode);
    }
    result
}

/// 认领后的确认主体（同步；被 async 命令在线程池调用）。失败路径
/// 回滚本次新建的 key 与 marker——门控期 key 必然不存在，删除安全；
/// marker 仅回滚本次新建的（便携 zip 自带的 marker 不动）——维持
/// 「确认失败不残留敏感/形态文件」，用户重试或取消都回到干净态。
fn confirm_portable_init_claimed(
    app: &AppHandle,
    mode: &quota_core::RuntimeMode,
) -> Result<(), String> {
    let quota_core::RuntimeMode::Portable { root } = mode else {
        return Err("门控内形态非便携：状态异常".into());
    };
    // 确认后落 marker（包内未带或显式 --portable 首次进入）：
    // 形态选择持久化，后续无参数启动不再询问
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or("无法定位可执行文件所在目录")?;
    let marker_created = if !quota_core::has_portable_marker(&exe_dir) {
        std::fs::write(exe_dir.join(quota_core::PORTABLE_MARKER), "")
            .map_err(|e| format!("便携标记写入失败：{e}"))?;
        true
    } else {
        false
    };
    let key_path = quota_core::portable_key_path(root);
    let init = || -> Result<(), String> {
        // 建钥（此时才产生敏感文件）：Vault::open 生成并落盘
        // （create_new 防覆盖；非 rename 原子，存在短暂的 0 字节窗口）
        quota_core::Vault::open(&quota_core::FileStore::new(key_path.clone()))
            .map_err(|e| format!("便携主密钥创建失败：{e}"))?;
        let state = crate::state::AppState::init(mode.clone())?;
        app.manage(state);
        Ok(())
    }();
    if init.is_err() {
        let _ = std::fs::remove_file(&key_path);
        if marker_created {
            let _ = std::fs::remove_file(exe_dir.join(quota_core::PORTABLE_MARKER));
        }
        return init;
    }
    // 窗口/托盘创建经主线程任务队列异步执行（setup_surfaces 的线程
    // 亲和性注释）：命令立即返回，前端 refetch 即见 ready 进入主界面
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        if let Err(e) = crate::setup_surfaces(&handle) {
            use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
            // 非阻塞 show + 回调退出：blocking_show 禁止在主线程调用
            // （插件文档：会冻结事件循环直至对话框关闭）
            let for_exit = handle.clone();
            handle
                .dialog()
                .message(format!("QuotaTray 初始化失败：\n{e}"))
                .kind(MessageDialogKind::Error)
                .title("QuotaTray")
                .show(move |_| {
                    for_exit.exit(1);
                });
        }
    });
    if let Err(e) = scheduled {
        // 调度失败 ≈ 事件循环已死（进程正在退出），本会话无托盘/调度器
        // 的半态无可救药：弹窗后终止（key 已建，重启即正常启动）
        use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
        app.dialog()
            .message(format!("QuotaTray 初始化失败：\n主线程调度失败：{e}"))
            .kind(MessageDialogKind::Error)
            .title("QuotaTray")
            .blocking_show();
        app.exit(1);
    }
    Ok(())
}

/// 便携首启取消：只清理本会话产生的 WebView2 缓存并退出。**不删整个
/// Data**——门控再现时 Data 内可能仍有既有 config.json/history.db
/// （如用户按密钥损坏指引删 key 重来、跨机拷贝漏拷 key），整删会连带
/// 销毁密文配置与历史，超出「取消则不写入任何敏感文件」的授权。
///
/// 并发取舍：与 confirm（池线程）并发的窄窗口内，confirm 可能已写入
/// key/marker 后本命令才执行——此时仅清 WebView2 退出，key 保留
/// （用户确已点击过确认，下次启动直接进入已初始化态属合理结果）；
/// 正常 UI 两按钮共用 busy 互斥，该窗口仅 devtools 注入可达。
#[tauri::command]
pub fn cancel_portable_init(app: AppHandle) -> Result<(), String> {
    if let Some(gate) = app.try_state::<crate::state::BootGate>()
        && let Some(quota_core::RuntimeMode::Portable { root }) =
            gate.pending.lock().unwrap().clone()
    {
        // WebView2 进程存活期间句柄锁定，删除尽力而为
        let _ = std::fs::remove_dir_all(root.join("WebView2"));
    }
    app.exit(0);
    Ok(())
}

/// 打开更新下载目录（zip 形态手动更新引导：下载后由用户
/// 退出应用解压覆盖，不提供自动安装）。走 opener 插件而非裸进程名，
/// 避免 CreateProcess 搜索序歧义。
#[tauri::command]
pub fn open_update_dir(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    ensure_desktop_update_commands(lang_of(&state))?;
    use tauri_plugin_opener::OpenerExt;
    let dir = crate::update_ctl::installer_dir();
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("打开下载目录失败：{e}"))
}

/// 打开控制台直达 URL（余额卡片「访问控制台」）。scheme 校验在 Rust 侧
/// 收口（仅 http/https）；走 Rust 侧 opener 而非前端插件直调——capability
/// 的 opener scope 锁定 GitHub 仓库单 URL，且模板条目的自定义 URL 是
/// 任意域名无法枚举白名单，自定义 command 不受前端 capability 约束。
#[tauri::command]
pub fn open_console_url(app: AppHandle, url: String) -> Result<(), String> {
    if let Some(reason) = console_url_rejected_reason(&url) {
        return Err(reason.into());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("打开控制台失败：{e}"))
}

/// 控制台 URL 安全校验：仅放行 `http(s)://` 形态（scheme 大小写不敏感，
/// RFC 3986；拒绝 file:/javascript:/裸 scheme/单斜杠畸形等），先 trim——
/// 与前端 isValidConsoleUrlInput 同口径。
fn console_url_rejected_reason(url: &str) -> Option<&'static str> {
    if let Some((scheme, _)) = url.trim().split_once("://")
        && matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
    {
        None
    } else {
        Some("控制台地址仅支持 http/https 链接")
    }
}

// ---- 更新检测（core::update 的薄封装） -------------------------------------

/// 当前更新状态（版本 / 上次检测 / 新版本信息 / 最近错误）。
#[tauri::command]
pub fn get_update_state(
    state: State<'_, AppState>,
) -> Result<crate::update_ctl::UpdateStateDto, String> {
    Ok(crate::update_ctl::dto_of(
        &state.update_ctl.read().unwrap(),
        state.mode.is_portable(),
    ))
}

/// 手动检测（设置页「立即检查」）：不受节流限制，检测后重建托盘菜单
/// （新版本信息行即时出现；移动端 no-op）。Android 放行——资产选择由
/// core `flavor_for` 编译期分流至 APK，无 WoA 误匹配；自动下载/就绪
/// 广播联动仅桌面（移动端手动检测口径，无调度器）。
#[tauri::command]
pub async fn check_update_now(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::update_ctl::UpdateStateDto, String> {
    let lang = lang_of(&state);
    ensure_update_check_supported(lang)?;
    let proxy = crate::update_ctl::proxy_url(&state);
    let http = quota_core::http::ReqwestHttpClient::new_with_proxy(
        std::time::Duration::from_secs(10),
        proxy.as_deref(),
    )
    .map_err(|e| lang.err_update_client(&e))?;
    let inner = crate::update_ctl::run_check(&state, &http).await;
    tray::rebuild(&app, &state);
    // 检测后联动：探测恢复广播 + 自动下载（后台执行，不阻塞本命令返回）；
    // 两者均为桌面语义（托盘消息、NSIS 自动安装链），移动端不编译
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    crate::update_ctl::post_check(&app, &state);
    // 移动端联动：发现新版本且本会话未广播过时推送 update-available
    // （前端消息中心红点/卡片）；同版本重复检测由后端登记短路。
    // iOS 无通知链（notify_available_once 为 android-only）
    #[cfg(target_os = "android")]
    crate::update_ctl::notify_available_once(&app, &state);
    Ok(crate::update_ctl::dto_of(&inner, state.mode.is_portable()))
}

/// 下载安装包到 %TEMP%/QuotaTray/Downloads 并记录进状态表，返回完整路径。
#[tauri::command]
pub async fn download_update(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let lang = lang_of(&state);
    ensure_desktop_update_commands(lang)?;
    crate::update_ctl::download_installer(&app, &state, lang).await
}

/// 运行已下载的安装包（NSIS 向导由用户交互完成）。启动成功后应用自动
/// 退出——覆盖安装需先解锁自身文件；留 400ms 让 IPC 响应送达前端。
/// zip 形态拒绝：普通 ARM64 Preview 与 Portable 都走手动覆盖引导。
#[tauri::command]
pub fn install_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let lang = lang_of(&state);
    ensure_desktop_update_commands(lang)?;
    let selector = quota_core::AssetSelector::for_runtime(
        quota_core::update::arch_label(),
        state.mode.is_portable(),
    );
    if selector.requires_manual_update() {
        return Err(lang.err_update_install_portable());
    }
    crate::update_ctl::run_installer(&state, lang)?;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        app.exit(0);
    });
    Ok(())
}

/// Android 下载入口：下载 APK 更新包并写入用户经系统文档选择器
/// （SAF）选定的 `content://` 位置。语义与桌面 [`download_update`]
/// （落盘 %TEMP% 返回路径）分流——移动端无应用外可写的固定目录，
/// 保存位置由用户选定；进度事件复用 `update-download-progress`。
/// content URI 不入状态表：SAF 授权随会话，离开更新页重下即可（约 18MB）。
#[tauri::command]
pub async fn download_update_to_uri(
    app: AppHandle,
    state: State<'_, AppState>,
    // 参数名 path 与 export_configuration 同惯例（前端统一传文档位置）
    path: String,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        download_apk_to_uri(&app, &state, &path).await
    }
    #[cfg(not(target_os = "android"))]
    {
        // 桌面编译目标无 SAF 写入；该入口仅移动端前端渲染，防御性拒绝
        let lang = lang_of(&state);
        let _ = (app, path);
        Err(lang.err_android_only_update_download())
    }
}

/// Android 安装入口：以系统安装器打开已保存的 APK（`content://` URI，
/// 通常来自 [`download_update_to_uri`] 的前端留存）。
/// `Ok(false)` 语义见 [`open_apk_impl`]——前端据此降级为手动安装引导。
#[tauri::command]
pub fn open_downloaded_apk(path: String, state: State<'_, AppState>) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        let lang = lang_of(&state);
        open_apk_impl(&path, &lang)
    }
    #[cfg(not(target_os = "android"))]
    {
        let lang = lang_of(&state);
        let _ = path;
        Err(lang.err_android_only_update_download())
    }
}

/// Android 拉起安装器的实现（`open_downloaded_apk` 的 cfg 内核）。
/// `Ok(false)` = 系统无安装器可处理（裁剪 ROM），非桥故障——前端据此
/// 降级为手动安装引导。
#[cfg(target_os = "android")]
fn open_apk_impl(uri: &str, lang: &Lang) -> Result<bool, String> {
    if !is_android_document_uri(uri) {
        return Err(lang.err_update_uri_invalid());
    }
    crate::apk_install::open_apk(uri)
}

/// Android：打开本应用的「允许安装未知应用」系统授权页（更新页提示行
/// 的次出路——Android 8~15 上授权放行安装；API 36 实证未声明自安装权限
/// 时该页开关置灰，主出路为文件管理器打开已保存 APK）。
/// `Ok(false)` = API 26 以下系统无此页面，前端降级为纯文案引导。
#[tauri::command]
pub fn open_install_consent(state: State<'_, AppState>) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        let _ = &state;
        crate::apk_install::open_install_consent()
    }
    #[cfg(not(target_os = "android"))]
    {
        let lang = lang_of(&state);
        let _ = state;
        Err(lang.err_android_only_update_download())
    }
}

/// Android SAF 下载写入的实现（`download_update_to_uri` 的 cfg 内核）。
#[cfg(target_os = "android")]
async fn download_apk_to_uri(
    app: &AppHandle,
    state: &State<'_, AppState>,
    uri: &str,
) -> Result<(), String> {
    // download_with_progress 是 trait 方法，android cfg 分支需显式引入
    use quota_core::update::AssetDownloader as _;

    let lang = lang_of(state);
    if !is_android_document_uri(uri) {
        return Err(lang.err_update_uri_invalid());
    }
    let info = state.update_ctl.read().unwrap().info.clone();
    let Some(info) = info else {
        return Err(lang.err_update_not_checked());
    };
    let Some(url) = info.asset_url else {
        return Err(lang.err_update_no_asset());
    };
    // asset_name 与 asset_url 同源（downloadable 判定），None 属防御分支
    let Some(name) = info.asset_name else {
        return Err(lang.err_update_no_asset());
    };
    if !crate::update_ctl::validate_asset_name(&name) {
        return Err(lang.err_update_bad_asset());
    }
    let reporter = crate::update_ctl::TauriProgressReporter { app };
    let downloader = quota_core::update::ReqwestAssetDownloader::try_with_proxy(
        crate::update_ctl::proxy_url(state).as_deref(),
    )
    .map_err(|e| lang.err_update_client(&e))?;
    let bytes = downloader
        .download_with_progress(&url, &reporter)
        .await
        .map_err(|e| lang.err_update_download(&e))?;
    write_apk_to_uri(app, uri, &bytes, &lang)
}

/// APK 字节写入 SAF 文档 URI（模式同 `export_configuration_to_uri`：
/// 字节不回传 WebView，Rust 侧直接落盘用户选定位置）。
#[cfg(target_os = "android")]
fn write_apk_to_uri(app: &AppHandle, uri: &str, bytes: &[u8], lang: &Lang) -> Result<(), String> {
    use std::io::Write;
    use tauri_plugin_fs::FsExt;

    let path = match uri.parse::<tauri_plugin_fs::FilePath>() {
        Ok(path) => path,
        Err(never) => match never {},
    };
    let mut options = tauri_plugin_fs::OpenOptions::new();
    options.write(true).truncate(true).create(true);
    let mut target = app
        .fs()
        .open(path, options)
        .map_err(|e| lang.err_update_write_uri(&e))?;
    target
        .write_all(bytes)
        .and_then(|_| target.sync_all())
        .map_err(|e| lang.err_update_write_uri(&e))
}

// ---- 系统通知（消息中心二阶：权限 / 前后台 / 渠道） -------------------------

/// 通知权限状态查询：Android 13+ 的 POST_NOTIFICATIONS 运行时权限
/// （serde 小写串 "granted"/"denied"/"prompt"）；桌面无运行时权限概念恒 granted。
#[tauri::command]
pub fn get_notification_permission(app: AppHandle) -> String {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_notification::NotificationExt;
        app.notification()
            .permission_state()
            .map(|s| s.to_string())
            // 桥故障按未授权处理（保守：通知不发，设置页仍可引导授权）
            .unwrap_or_else(|e| {
                eprintln!("通知权限状态查询失败：{e}");
                "denied".into()
            })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        "granted".into()
    }
}

/// 请求通知运行时权限（Android 13+ 弹系统对话框；用户曾拒绝后系统不再
/// 弹、直接返回 denied——前端据此改为引导跳系统设置页）。桌面恒 granted。
#[tauri::command]
pub fn request_notification_permission(app: AppHandle) -> String {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_notification::NotificationExt;
        app.notification()
            .request_permission()
            .map(|s| s.to_string())
            .unwrap_or_else(|e| {
                eprintln!("通知权限请求失败：{e}");
                "denied".into()
            })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        "granted".into()
    }
}

/// 跳系统「应用通知设置」页（Android 13+ 拒绝过权限后的唯一出路）。
/// `Ok(true)` = 已发起跳转；桌面确定性拒绝。
#[tauri::command]
pub fn open_notification_settings(state: State<'_, AppState>) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        let _ = &state;
        crate::notification_android::open_notification_settings()
    }
    #[cfg(not(target_os = "android"))]
    {
        let lang = lang_of(&state);
        Err(lang.err_android_only_notification())
    }
}

/// 前后台状态同步（Android 消息通知的发射条件）：前端 visibilitychange
/// 驱动写入；通知发射点读它决定「入列红点」还是「补发系统通知」。
/// 桌面调用无害（不消费）。Relaxed 足够：布尔提示，无跨字段不变式。
/// 首次调用同时置校准位——WorkManager 冷启动（无前端）据此把未校准
/// 的乐观初值视为后台，否则冷启动通知恒不发。
#[tauri::command]
pub fn set_app_foreground(foreground: bool, _state: State<'_, AppState>) {
    crate::state::APP_FOREGROUND.store(foreground, std::sync::atomic::Ordering::Relaxed);
    crate::state::APP_FOREGROUND_CALIBRATED.store(true, std::sync::atomic::Ordering::Relaxed);
}

// ---- 契约测试 -------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_document_uri_is_distinguished_from_desktop_path() {
        assert!(is_android_document_uri(
            "content://com.android.providers.documents/document/1"
        ));
        assert!(!is_android_document_uri(
            "C:\\Users\\demo\\backup.qtray-export"
        ));
        assert!(!is_android_document_uri("/tmp/backup.qtray-export"));
    }

    #[test]
    fn boot_platform_only_marks_android_as_mobile_shell() {
        assert_eq!(runtime_platform_for("android"), "android");
        assert_eq!(runtime_platform_for("windows"), "desktop");
        assert_eq!(runtime_platform_for("linux"), "desktop");
    }

    /// 契约：低余额判定——百分比语义与前端卡片高亮一致（used_percent
    /// 镜像），阈值含边界（=），多窗口取最高，数据不足不触发。
    #[test]
    fn low_balance_breach_contract() {
        let window =
            |used: Option<f64>, total: Option<f64>, unit: Option<&str>| quota_core::UsageData {
                used,
                total,
                unit: unit.map(Into::into),
                ..Default::default()
            };
        // 低于阈值不触发
        let healthy = vec![window(Some(50.0), Some(100.0), None)];
        assert_eq!(low_balance_breach(&healthy, 80), None);
        // 达到阈值触发（边界 = 阈值也算）
        let edge = vec![window(Some(80.0), Some(100.0), None)];
        assert_eq!(low_balance_breach(&edge, 80), Some(80.0));
        // 多窗口：任一达标即触发，取最高达标值
        let multi = vec![
            window(Some(50.0), Some(100.0), None),
            window(Some(92.0), None, Some("%")),
        ];
        assert_eq!(low_balance_breach(&multi, 80), Some(92.0));
        // 余额型（无 total、非 %）数据不足 → 不触发
        let balance_only = vec![window(Some(3.0), None, Some("CNY"))];
        assert_eq!(low_balance_breach(&balance_only, 80), None);
        // 空数据不触发
        assert_eq!(low_balance_breach(&[], 80), None);
    }

    /// 契约：低余额边沿触发——首次达标通知、持续达标（含百分比上升）
    /// 静默、回落/数据不足清除登记、未达标无登记不动；回落后再达标
    /// 重新通知（防后台轮询每周期重复系统通知）。
    #[test]
    fn low_balance_edge_contract() {
        use LowBalanceEdge::{Notify, Reset, Silent};
        assert_eq!(
            low_balance_edge(false, Some(85.0)),
            Notify,
            "首次达标 → 通知"
        );
        assert_eq!(
            low_balance_edge(true, Some(85.0)),
            Silent,
            "持续达标不重复打扰"
        );
        assert_eq!(
            low_balance_edge(true, Some(91.0)),
            Silent,
            "持续达标（百分比上升）同样静默——前端卡片另随查询刷新"
        );
        assert_eq!(
            low_balance_edge(true, None),
            Reset,
            "回落/数据不足 → 清除登记"
        );
        assert_eq!(low_balance_edge(false, None), Silent, "未达标无登记不动");
        // 回落后再达标：登记已清除 → 重新通知
        assert_eq!(low_balance_edge(false, Some(80.0)), Notify);
    }

    #[test]
    fn android_blocks_native_providers_that_require_desktop_cli_files() {
        assert!(mobile_cli_provider_blocked("android", "claude"));
        assert!(mobile_cli_provider_blocked("android", "codex"));
        assert!(!mobile_cli_provider_blocked("android", "deepseek"));
        assert!(!mobile_cli_provider_blocked("windows", "claude"));
    }

    #[test]
    fn desktop_update_commands_are_blocked_on_mobile_targets() {
        assert!(!desktop_update_commands_supported("android"));
        assert!(!desktop_update_commands_supported("ios"));
        assert!(desktop_update_commands_supported("windows"));
        assert!(desktop_update_commands_supported("linux"));
    }

    /// 契约：守卫分层——更新「检测」对 Android 放行（2026-08-29 手动检测
    /// 口径），安装/打开目录仍永久桌面（Play 红线的代码面表达）。
    #[test]
    fn update_check_guard_is_looser_than_install_guard_on_android() {
        assert!(update_check_supported("android"));
        assert!(!update_check_supported("ios"));
        assert!(update_check_supported("windows"));
        assert!(update_check_supported("linux"));
        // 分层差异本身即契约：检测放行 ≠ 安装命令放行
        assert_ne!(
            update_check_supported("android"),
            desktop_update_commands_supported("android")
        );
    }
    use quota_core::{InMemoryStore, UsageData};

    fn entry(id: &str) -> ProviderEntry {
        ProviderEntry {
            id: id.into(),
            name: "测试".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            api_key2_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
            console_url: None,
        }
    }

    fn transfer_path(tag: &str, name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quota-desktop-transfer-{tag}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    /// AI 调试契约：返回真实存在的 CLI 绝对路径；开发同目录优先，安装包资源兜底。
    /// 文件名按平台拼接 EXE_SUFFIX（Linux 无后缀），保证跨矩阵确定性。
    #[test]
    fn resolves_existing_quota_cli_path() {
        let cli_name = format!("quota{}", std::env::consts::EXE_SUFFIX);
        let current_exe = transfer_path(
            "assist-cli",
            &format!("app/quota-desktop{}", std::env::consts::EXE_SUFFIX),
        );
        let dev_cli = current_exe.with_file_name(&cli_name);
        std::fs::create_dir_all(current_exe.parent().unwrap()).unwrap();
        std::fs::write(&dev_cli, b"dev-cli").unwrap();

        let resource_dir = transfer_path("assist-cli", "resources");
        let bundled_cli = resource_dir.join("bin").join(&cli_name);
        std::fs::create_dir_all(bundled_cli.parent().unwrap()).unwrap();
        std::fs::write(&bundled_cli, b"bundled-cli").unwrap();

        assert_eq!(
            resolve_quota_cli_path_from(&current_exe, Some(&resource_dir)).unwrap(),
            dev_cli.canonicalize().unwrap()
        );
        std::fs::remove_file(&dev_cli).unwrap();
        assert_eq!(
            resolve_quota_cli_path_from(&current_exe, Some(&resource_dir)).unwrap(),
            bundled_cli.canonicalize().unwrap()
        );
    }

    /// 契约：PATH 回退排除 Windows 系统目录（System32 自带同名 NTFS 配额
    /// 工具 quota.exe，不得被当作本软件 CLI 拼进 Agent 提示词）。
    #[test]
    #[cfg(windows)]
    fn system_dirs_are_excluded_from_path_fallback() {
        assert!(is_system_dir(std::path::Path::new("C:\\Windows\\System32")));
        assert!(is_system_dir(std::path::Path::new("C:\\WINDOWS\\SYSWOW64")));
        assert!(!is_system_dir(std::path::Path::new("D:\\Tools\\bin")));
        assert!(!is_system_dir(std::path::Path::new(
            "D:\\Apps\\System32Tools"
        )));
    }

    /// 重排校验契约：ids 与现有条目集合一致且无重复才放行——并发增删、
    /// 缺项/多项/重复/空表一律拒绝，由前端 refetch 恢复真实状态。
    #[test]
    fn validate_reorder_ids_accepts_exact_match_only() {
        let existing = ["a", "b", "c"];
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(validate_reorder_ids(&existing, &ids(&["c", "a", "b"])));
        assert!(validate_reorder_ids(&existing, &ids(&["a", "b", "c"])));
        assert!(!validate_reorder_ids(&existing, &ids(&["a", "b"])));
        assert!(!validate_reorder_ids(
            &existing,
            &ids(&["a", "b", "c", "d"])
        ));
        assert!(!validate_reorder_ids(&existing, &ids(&["a", "b", "d"])));
        assert!(!validate_reorder_ids(&existing, &ids(&["a", "a", "b"])));
        assert!(!validate_reorder_ids(&[], &ids(&["a"])));
        assert!(validate_reorder_ids(&[], &ids(&[])));
    }

    /// 重排契约：每个条目挪到其 id 在 ids 中的位次（稳定、全量、可逆）。
    #[test]
    fn reorder_providers_in_place_moves_entries_to_given_order() {
        let mut providers = vec![entry("a"), entry("b"), entry("c")];
        let ids = vec!["c".to_string(), "a".to_string(), "b".to_string()];
        reorder_providers_in_place(&mut providers, &ids);
        let order: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(order, ["c", "a", "b"]);

        // 同序重排幂等（no-op）；随后的显式逆置换才恢复原序
        reorder_providers_in_place(&mut providers, &ids);
        let order: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(order, ["c", "a", "b"]);
        let back = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        reorder_providers_in_place(&mut providers, &back);
        let order: Vec<&str> = providers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(order, ["a", "b", "c"]);
    }

    /// 单条目重排为恒等操作（卡片数为 1 时前端不会发起，防御性契约）。
    #[test]
    fn reorder_providers_in_place_single_entry_is_noop() {
        let mut providers = vec![entry("only")];
        reorder_providers_in_place(&mut providers, &["only".to_string()]);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "only");
    }

    #[test]
    fn history_query_helper_filters_provider_and_start_time() {
        let store = quota_core::HistoryStore::open_in_memory().unwrap();
        store
            .merge_rows(&[
                quota_core::HistoryExportRow {
                    provider_id: "p1".into(),
                    window_key: "five_hour".into(),
                    sampled_at: 1_000,
                    used: Some(10.0),
                    remaining: Some(90.0),
                    total: Some(100.0),
                    unit: Some("%".into()),
                },
                quota_core::HistoryExportRow {
                    provider_id: "p1".into(),
                    window_key: "five_hour".into(),
                    sampled_at: 2_000,
                    used: Some(20.0),
                    remaining: Some(80.0),
                    total: Some(100.0),
                    unit: Some("%".into()),
                },
                quota_core::HistoryExportRow {
                    provider_id: "p2".into(),
                    window_key: "balance".into(),
                    sampled_at: 3_000,
                    used: None,
                    remaining: Some(42.0),
                    total: None,
                    unit: Some("CNY".into()),
                },
            ])
            .unwrap();

        let points = get_history_from_store(&store, "p1", 1_500).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].sampled_at, 2_000);
        assert_eq!(points[0].window_key, "five_hour");
    }

    #[test]
    fn transfer_helpers_rewrap_credentials_for_destination_vault() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let source_path = transfer_path("roundtrip", "source.json");
        let target_path = transfer_path("roundtrip", "target.json");
        let bundle = transfer_path("roundtrip", "backup.qtray-export");
        let mut source_entry = entry("p1");
        source_entry
            .set_api_key(&source_vault, "sk-desktop-transfer")
            .unwrap();
        AppConfig {
            providers: vec![source_entry],
            custom_models: Default::default(),
        }
        .save(&source_path)
        .unwrap();

        let comparison = vec![quota_core::UsageComparisonSeries {
            provider_id: "p1".into(),
            window_key: "w0".into(),
            color_slot: 0,
        }];
        export_configuration_at(
            &source_path,
            &bundle,
            &source_vault,
            None,
            Some(&comparison),
        )
        .unwrap();
        let imported = import_configuration_at(&bundle, &target_path, &target_vault).unwrap();
        assert_eq!(imported.config.providers.len(), 1);
        assert_eq!(imported.usage_comparison_series, Some(comparison));
        assert_eq!(
            imported.config.providers[0]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            "sk-desktop-transfer"
        );
        assert_eq!(AppConfig::load(&target_path).unwrap(), imported.config);
        let _ = std::fs::remove_dir_all(source_path.parent().unwrap());
    }

    /// 契约：迁移包携带历史行（v2 信封），导出→导入后由调用方合并进
    /// 目标库（IPC 层 import_configuration 接线）。
    #[test]
    fn transfer_helpers_carry_history_rows() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let source_path = transfer_path("history", "source.json");
        let target_path = transfer_path("history", "target.json");
        let bundle = transfer_path("history", "backup.qtray-export");
        AppConfig {
            providers: vec![entry("p1")],
            custom_models: Default::default(),
        }
        .save(&source_path)
        .unwrap();

        let rows = vec![quota_core::HistoryExportRow {
            provider_id: "p1".into(),
            window_key: "five_hour".into(),
            sampled_at: 1_700_000_000_000,
            used: Some(10.0),
            remaining: Some(90.0),
            total: Some(100.0),
            unit: Some("%".into()),
        }];
        export_configuration_at(&source_path, &bundle, &source_vault, Some(&rows), None).unwrap();
        let imported = import_configuration_at(&bundle, &target_path, &target_vault).unwrap();
        assert_eq!(imported.history.as_deref(), Some(rows.as_slice()));

        // 模拟 IPC 层合并：临时历史库 merge 后可查
        let db = transfer_path("history", "history.db");
        let store = quota_core::HistoryStore::open(&db).unwrap();
        store
            .merge_rows(imported.history.as_deref().unwrap())
            .unwrap();
        assert_eq!(store.range("p1", 0).unwrap().len(), 1);
        // Windows 上句柄存活时删除会失败，先释放再清理
        drop(store);
        let _ = std::fs::remove_dir_all(source_path.parent().unwrap());
        let _ = std::fs::remove_dir_all(target_path.parent().unwrap());
    }

    #[test]
    fn transfer_helper_failure_preserves_existing_configuration() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let config_path = transfer_path("failure", "config.json");
        let bundle = transfer_path("failure", "bad.qtray-export");
        let existing = AppConfig {
            providers: vec![entry("keep")],
            custom_models: Default::default(),
        };
        existing.save(&config_path).unwrap();
        std::fs::write(&bundle, b"not a transfer package").unwrap();

        assert!(import_configuration_at(&bundle, &config_path, &vault).is_err());
        assert_eq!(AppConfig::load(&config_path).unwrap(), existing);
        let _ = std::fs::remove_dir_all(config_path.parent().unwrap());
    }

    /// 契约：新 key 加密写入；密文非明文且以 v1: 开头。
    #[test]
    fn key_policy_replaces_with_ciphertext() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut e = entry("p1");
        let existing = None;
        apply_key_policy(&mut e, existing, Some("sk-plain"), &vault, Lang::Zh).unwrap();
        let enc = e.api_key_enc.expect("应有密文");
        assert!(enc.starts_with("v1:"));
        assert!(!enc.contains("sk-plain"), "密文不得含明文");
        assert_eq!(vault.decrypt(&enc, "p1").unwrap(), "sk-plain");
    }

    /// 契约：空 key / 缺省 key → 保留旧密文不变；前端传入的 api_key_enc 被忽略。
    #[test]
    fn key_policy_empty_keeps_existing_and_ignores_frontend_ciphertext() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut old = entry("p1");
        old.set_api_key(&vault, "sk-old").unwrap();
        let old_enc = old.api_key_enc.clone();

        let mut e = entry("p1");
        e.api_key_enc = Some("v1:forged-by-frontend".into()); // 伪造密文应被忽略
        apply_key_policy(&mut e, Some(&old), Some("   "), &vault, Lang::Zh).unwrap();
        assert_eq!(e.api_key_enc, old_enc, "空 key 应保留旧密文");

        apply_key_policy(&mut e, Some(&old), None, &vault, Lang::Zh).unwrap();
        assert_eq!(e.api_key_enc, old_enc, "缺省 key 应保留旧密文");
    }

    /// 契约：新条目 + 空 key → 无密文（后续查询报"未配置 API key"确定性错误）。
    #[test]
    fn key_policy_new_entry_without_key_is_none() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut e = entry("new");
        e.api_key_enc = Some("v1:forged".into());
        apply_key_policy(&mut e, None, Some(""), &vault, Lang::Zh).unwrap();
        assert!(e.api_key_enc.is_none());
    }

    /// 契约：第二凭据槽与主 key 同策略——非空写入、空/缺省保留、
    /// 前端伪造 api_key2_enc 被忽略。
    #[test]
    fn key2_policy_mirrors_primary_key_policy() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut old = entry("p1");
        old.set_api_key2(&vault, "1024").unwrap();
        let old_enc2 = old.api_key2_enc.clone();

        let mut e = entry("p1");
        e.api_key2_enc = Some("v1:forged-2".into());
        apply_key2_policy(&mut e, Some(&old), Some("  "), &vault, Lang::Zh).unwrap();
        assert_eq!(e.api_key2_enc, old_enc2, "空第二凭据应保留旧密文");

        apply_key2_policy(&mut e, Some(&old), None, &vault, Lang::Zh).unwrap();
        assert_eq!(e.api_key2_enc, old_enc2, "缺省第二凭据应保留旧密文");

        apply_key2_policy(&mut e, None, Some("2048"), &vault, Lang::Zh).unwrap();
        assert_ne!(e.api_key2_enc, old_enc2, "非空第二凭据应加密写入");
        assert!(
            e.credentials(&vault).is_err(),
            "未配主 key 时解密仍应失败（主 key 必填语义不变）"
        );
    }

    /// 契约：validate_entry——合法条目通过；未知 native、非法峰谷逐一拦截（双语前缀）。
    #[test]
    fn validate_entry_checks_native_and_pricing() {
        assert!(validate_entry(&entry("p1"), Lang::Zh).is_ok());

        let mut unknown = entry("p1");
        unknown.kind = ProviderKind::Native {
            provider: "nope".into(),
        };
        let err = validate_entry(&unknown, Lang::Zh).unwrap_err();
        assert!(err.contains("未知的预置平台"), "{err}");

        // 跨日窗口被拒，错误文案带峰谷前缀与字段定位
        let mut bad_pricing = entry("p1");
        bad_pricing.pricing = Some(quota_core::PricingConfig {
            windows: Some(vec![quota_core::PeakWindow {
                days: vec![quota_core::pricing::Weekday::Mon],
                start: "22:00".into(),
                end: "06:00".into(),
            }]),
            ..Default::default()
        });
        for lang in [Lang::Zh, Lang::En] {
            let err = validate_entry(&bad_pricing, lang).unwrap_err();
            let prefix = match lang {
                Lang::Zh => "峰谷定价配置无效",
                Lang::En => "Invalid peak pricing",
            };
            assert!(err.starts_with(prefix), "{lang:?}: {err}");
            assert!(err.contains("windows[0].start/end"), "{lang:?}: {err}");
        }

        // 合法峰谷配置通过（起止改为同日 09:00–12:00）
        let mut ok_pricing = bad_pricing;
        if let Some(w) = ok_pricing.pricing.as_mut().unwrap().windows.as_mut() {
            w[0].start = "09:00".into();
            w[0].end = "12:00".into();
        }
        assert!(validate_entry(&ok_pricing, Lang::Zh).is_ok());
    }

    /// 契约：native 元信息 DTO 携带控制台直达 URL（双站域名分立），
    /// 前端据此渲染「访问控制台」入口。
    #[test]
    fn native_metas_carry_console_url() {
        let metas = native_meta_dtos(&AppConfig::default());
        let find = |id: &str| metas.iter().find(|m| m.id == id).unwrap();
        assert_eq!(
            find("siliconflow").console_url.as_deref(),
            Some("https://cloud.siliconflow.cn/")
        );
        assert_eq!(
            find("siliconflow_global").console_url.as_deref(),
            Some("https://cloud.siliconflow.com/")
        );
        assert!(metas.iter().all(|m| m.console_url.is_some()));
    }

    /// 契约：open_console_url 的 scheme 白名单——仅 http/https 放行
    /// （大小写不敏感、容忍首尾空白，与前端校验同口径），
    /// file:/javascript:/无 scheme 一律拒绝（防借应用拉起本地程序）。
    #[test]
    fn console_url_scheme_whitelist() {
        assert!(console_url_rejected_reason("https://cloud.siliconflow.cn/").is_none());
        assert!(console_url_rejected_reason("http://example.com/console").is_none());
        assert!(console_url_rejected_reason("HTTPS://EXAMPLE.COM").is_none());
        assert!(console_url_rejected_reason("  https://example.com  ").is_none());
        assert!(console_url_rejected_reason("file:///C:/Windows/system.ini").is_some());
        assert!(console_url_rejected_reason("javascript:alert(1)").is_some());
        assert!(console_url_rejected_reason("cloud.siliconflow.cn").is_some());
        assert!(console_url_rejected_reason("").is_some());
        // 裸 scheme（无 ://）与单斜杠畸形形态拒绝
        assert!(console_url_rejected_reason("https").is_some());
        assert!(console_url_rejected_reason("https:/example.com").is_some());
    }

    /// 契约：list_native_metas 携带峰谷预置——deepseek 有（三模型/默认 flash/
    /// UTC+8 双窗口），Kimi 开放平台、Kimi Code 与智谱系各站有预置，
    /// 聚合与无预置平台为 None；
    /// 订阅项 DTO 带 plan/windows 字段（前端类型镜像依据）。
    #[test]
    fn native_metas_carry_pricing_preset() {
        let mut cfg = AppConfig::default();
        cfg.custom_models.insert(
            "deepseek".into(),
            vec![quota_core::CustomModelDef {
                id: "flash".into(),
                display: "V4 Flash（自算）".into(),
                peak: Some(quota_core::PriceTier {
                    output: Some(9.1),
                    ..Default::default()
                }),
                ..Default::default()
            }],
        );
        let metas = native_meta_dtos(&cfg);
        let ds = metas.iter().find(|m| m.id == "deepseek").unwrap();
        let p = ds.pricing.as_ref().expect("deepseek 应有峰谷预置");
        assert_eq!(p.currency, "CNY");
        assert_eq!(p.timezone_offset_minutes, 480);
        assert_eq!(p.default_model, "flash");
        assert_eq!(p.models.len(), 3);
        assert_eq!(p.windows.len(), 2);
        // 序列化形状（前端 types.ts 镜像的依据）
        let j = serde_json::to_value(p).unwrap();
        assert_eq!(j["models"][0]["id"], "flash");
        assert_eq!(j["models"][0]["peak"]["cache_hit_input"], 0.1);
        assert_eq!(j["models"][0]["plan"], "pay_as_you_go");
        assert_eq!(ds.pricing_by_currency["CNY"].currency, "CNY");
        assert_eq!(ds.pricing_by_currency["USD"].currency, "USD");
        assert_eq!(ds.custom_models.len(), 1);
        assert_eq!(ds.custom_models[0].display, "V4 Flash（自算）");

        // 有预置的平台（新批次）
        for id in [
            "kimi_cn",
            "kimi_global",
            "kimi_code_cn",
            "kimi_code_global",
            "zhipu_api",
            "zai_api",
            "zhipu",
            "zai",
        ] {
            let m = metas.iter().find(|m| m.id == id).unwrap();
            assert!(m.pricing.is_some(), "{id} 应有预置");
        }
        // 智谱/Z.ai 通用 API：默认模型为按量计费，不混入订阅项，
        // 也不展示 Coding Plan 套餐变体。
        for id in ["zhipu_api", "zai_api"] {
            let m = metas.iter().find(|m| m.id == id).unwrap();
            let p = m.pricing.as_ref().unwrap();
            assert_eq!(p.default_model, "glm-5.3");
            assert!(
                p.models.iter().all(|model| {
                    serde_json::to_value(model).unwrap()["plan"] == "pay_as_you_go"
                })
            );
            assert!(!m.supports_plan_variant);
        }
        // Kimi Code 是独立订阅 Provider：默认模型即订阅额度，且不展示
        // 智谱专属的套餐变体选择。
        for id in ["kimi_code_cn", "kimi_code_global"] {
            let m = metas.iter().find(|m| m.id == id).unwrap();
            let p = m.pricing.as_ref().unwrap();
            assert_eq!(p.default_model, "coding-plan");
            assert_eq!(
                serde_json::to_value(&p.models[0]).unwrap()["plan"],
                "subscription"
            );
            assert!(!m.supports_plan_variant);
        }
        // 智谱订阅项：plan=subscription、模型级窗口
        let zhipu = metas.iter().find(|m| m.id == "zhipu").unwrap();
        let p = zhipu.pricing.as_ref().unwrap();
        let coding = p
            .models
            .iter()
            .find(|m| m.id == "coding-plan")
            .expect("智谱应含 Coding Plan 订阅项");
        assert_eq!(
            serde_json::to_value(coding).unwrap()["plan"],
            "subscription"
        );
        assert_eq!(coding.windows.as_ref().map(Vec::len), Some(1));

        // 套餐变体支持标记：仅智谱系
        assert!(!ds.supports_plan_variant);
        let zai = metas.iter().find(|m| m.id == "zai").unwrap();
        assert!(zai.supports_plan_variant);
        assert!(
            !metas
                .iter()
                .find(|m| m.id == "openrouter")
                .unwrap()
                .supports_plan_variant
        );

        // 聚合平台与无预置平台为 None
        for other in metas.iter().filter(|m| {
            ![
                "deepseek",
                "kimi_cn",
                "kimi_global",
                "kimi_code_cn",
                "kimi_code_global",
                "zhipu_api",
                "zai_api",
                "zhipu",
                "zai",
            ]
            .contains(&m.id.as_str())
        }) {
            assert!(other.pricing.is_none(), "{} 不应有预置", other.id);
        }
    }

    /// 契约：{{apiKey}} 出现在 url/headers/body 任一处即需要 key。
    #[test]
    fn template_needs_api_key_detection() {
        let base = r#"{"request":{"url":"https://a.com"},"extract":{"remaining":"$.a"}}"#;
        let no_need: TemplateConfig = serde_json::from_str(base).unwrap();
        assert!(!template_needs_api_key(&no_need));

        let in_url: TemplateConfig = serde_json::from_str(
            r#"{"request":{"url":"https://a.com?k={{apiKey}}"},"extract":{"remaining":"$.a"}}"#,
        )
        .unwrap();
        assert!(template_needs_api_key(&in_url));

        let in_header: TemplateConfig = serde_json::from_str(
            r#"{"request":{"url":"https://a.com","headers":{"Authorization":"Bearer {{apiKey}}"}},"extract":{"remaining":"$.a"}}"#,
        )
        .unwrap();
        assert!(template_needs_api_key(&in_header));

        // 带空格写法与执行期同语义（第 2 轮审查 P2 的回归锁定）
        let spaced: TemplateConfig = serde_json::from_str(
            r#"{"request":{"url":"https://a.com","headers":{"Authorization":"Bearer {{ apiKey }}"}},"extract":{"remaining":"$.a"}}"#,
        )
        .unwrap();
        assert!(template_needs_api_key(&spaced), "带空格写法应识别");
    }

    /// 契约：结果表 → 快照只含有成功数据的条目（错误态不进快照）。
    #[test]
    fn snapshots_keep_only_successful_entries() {
        let mut results = std::collections::HashMap::new();
        results.insert(
            "ok".into(),
            EntryState {
                data: Some(vec![UsageData {
                    remaining: Some(1.0),
                    ..Default::default()
                }]),
                at: Some(42),
                error: None,
            },
        );
        results.insert(
            "failed".into(),
            EntryState {
                data: Some(vec![UsageData::default()]),
                at: Some(1),
                error: Some(ErrorInfo {
                    kind: "transient".into(),
                    message: "x".into(),
                    detail: None,
                }),
            },
        );
        results.insert(
            "never".into(),
            EntryState {
                error: Some(ErrorInfo {
                    kind: "deterministic".into(),
                    message: "y".into(),
                    detail: None,
                }),
                ..Default::default()
            },
        );
        let snaps = snapshots_from_results(&results);
        // "failed" 仍有成功数据（keep-last-good）→ 保留；"never" 无数据 → 丢弃
        assert!(snaps.entries.contains_key("ok"));
        assert!(snaps.entries.contains_key("failed"));
        assert!(!snaps.entries.contains_key("never"));
        assert_eq!(snaps.entries["ok"].at, 42);
    }

    /// 契约：patch 只覆盖提交的字段，其余保持现值——单字段快切入口
    /// 不得把未提交的设置（代理端口等）抹回默认。
    #[test]
    fn settings_patch_overrides_only_submitted_fields() {
        let mut base = Settings {
            update_proxy_port: Some(7897),
            update_proxy_host: Some("192.168.1.5".into()),
            theme: "light".into(),
            language: "zh".into(),
            tray_icon_entry_id: Some("A1B2C3".into()),
            ring_units_per_circle: 500.0,
            ..Settings::default()
        };
        // 仅提交主题，其余字段缺省
        apply_settings_patch(
            &mut base,
            &SettingsPatch {
                theme: Some("dark".into()),
                ..SettingsPatch::default()
            },
        );
        assert_eq!(base.theme, "dark", "提交字段被覆盖");
        assert_eq!(base.update_proxy_port, Some(7897), "未提交字段保持现值");
        assert_eq!(
            base.update_proxy_host,
            Some("192.168.1.5".into()),
            "未提交字段保持现值"
        );
        assert_eq!(base.language, "zh");
        assert_eq!(base.tray_icon_entry_id, Some("A1B2C3".into()));
        assert_eq!(base.ring_units_per_circle, 500.0);

        // 双层 Option：显式清空（Some(None)）与不动（缺省）可区分
        apply_settings_patch(
            &mut base,
            &SettingsPatch {
                tray_icon_entry_id: Some(None),
                update_proxy_port: Some(None),
                update_proxy_host: Some(None),
                ..SettingsPatch::default()
            },
        );
        assert_eq!(base.tray_icon_entry_id, None, "Some(None) 显式清空");
        assert_eq!(base.update_proxy_port, None, "Some(None) 显式清空");
        assert_eq!(base.update_proxy_host, None, "Some(None) 显式清空");
        assert_eq!(base.theme, "dark", "其余字段仍不动");

        // host 覆盖提交（Android 指向电脑代理的主路径）
        apply_settings_patch(
            &mut base,
            &SettingsPatch {
                update_proxy_host: Some(Some("10.0.0.2".into())),
                ..SettingsPatch::default()
            },
        );
        assert_eq!(base.update_proxy_host, Some("10.0.0.2".into()));
    }

    /// 契约：serde 边界上 JSON null → Some(None)（显式清空）、字段缺省 →
    /// 外层 None（不动）、类型错误正常透出——裸 `Option<Option<T>>` 的
    /// serde 默认行为会把 null 也折叠为外层 None，两者不可区分，故双层
    /// 字段必须走 double_option。
    #[test]
    fn settings_patch_serde_boundary() {
        let p: SettingsPatch = serde_json::from_str(
            r#"{"update_proxy_port": 7897, "update_proxy_host": "192.168.1.5", "tray_icon_entry_id": null}"#,
        )
        .unwrap();
        assert_eq!(p.update_proxy_port, Some(Some(7897)), "有值 → 覆盖");
        assert_eq!(
            p.update_proxy_host,
            Some(Some("192.168.1.5".into())),
            "有值 → 覆盖"
        );
        assert_eq!(p.tray_icon_entry_id, Some(None), "JSON null → 显式清空");

        let p: SettingsPatch = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(p.theme, Some("dark".into()));
        assert_eq!(p.tray_icon_entry_id, None, "字段缺省 → 不动");
        assert_eq!(p.update_proxy_port, None, "字段缺省 → 不动");
        assert_eq!(p.update_proxy_host, None, "字段缺省 → 不动");

        let p: SettingsPatch = serde_json::from_str(r#"{"update_proxy_host": null}"#).unwrap();
        assert_eq!(p.update_proxy_host, Some(None), "JSON null → 显式清空");

        assert!(
            serde_json::from_str::<SettingsPatch>(r#"{"update_proxy_port": "abc"}"#).is_err(),
            "类型错误透出而非静默清空"
        );
        assert!(
            serde_json::from_str::<SettingsPatch>(r#"{"update_proxy_host": 123}"#).is_err(),
            "类型错误透出而非静默清空"
        );
    }

    /// 契约：notifications_enabled 可经 patch 提交（与前端 SettingsPatch
    /// 类型字段集镜像，缺省字段静默丢弃的回归防线）。
    #[test]
    fn settings_patch_notifications_enabled() {
        let mut base = Settings::default();
        assert!(base.notifications_enabled, "默认开启");
        apply_settings_patch(
            &mut base,
            &SettingsPatch {
                notifications_enabled: Some(false),
                ..SettingsPatch::default()
            },
        );
        assert!(!base.notifications_enabled, "提交 false → 关闭");

        let p: SettingsPatch = serde_json::from_str(r#"{"notifications_enabled": true}"#).unwrap();
        assert_eq!(p.notifications_enabled, Some(true));
    }

    #[test]
    fn settings_patch_usage_comparison_replaces_with_explicit_empty_or_values() {
        let mut base = Settings::default();
        apply_settings_patch(
            &mut base,
            &SettingsPatch {
                usage_comparison_series: Some(vec![quota_core::UsageComparisonSeries {
                    provider_id: "p1".into(),
                    window_key: "w1".into(),
                    color_slot: 0,
                }]),
                ..SettingsPatch::default()
            },
        );
        assert_eq!(base.usage_comparison_series.as_ref().unwrap().len(), 1);

        let patch: SettingsPatch =
            serde_json::from_str(r#"{"usage_comparison_series":[]}"#).unwrap();
        apply_settings_patch(&mut base, &patch);
        assert_eq!(base.usage_comparison_series, Some(Vec::new()));
    }

    #[test]
    fn full_settings_save_preserves_cli_owned_disk_fields() {
        let mut incoming = Settings {
            update_last_check: Some(10),
            usage_comparison_series: Some(Vec::new()),
            theme: "dark".into(),
            ..Settings::default()
        };
        let disk = Settings {
            update_last_check: Some(20),
            usage_comparison_series: Some(vec![quota_core::UsageComparisonSeries {
                provider_id: "p1".into(),
                window_key: "w1".into(),
                color_slot: 2,
            }]),
            ..Settings::default()
        };

        merge_cli_owned_settings(&mut incoming, &disk);

        assert_eq!(incoming.update_last_check, Some(20));
        assert_eq!(
            incoming.usage_comparison_series,
            disk.usage_comparison_series
        );
        assert_eq!(incoming.theme, "dark", "GUI 表单拥有的字段保持提交值");
    }

    #[test]
    fn removing_provider_prunes_only_its_usage_comparisons() {
        let items = Some(vec![
            quota_core::UsageComparisonSeries {
                provider_id: "p1".into(),
                window_key: "w1".into(),
                color_slot: 0,
            },
            quota_core::UsageComparisonSeries {
                provider_id: "p2".into(),
                window_key: "w2".into(),
                color_slot: 1,
            },
        ]);

        let pruned = prune_usage_comparisons_for_provider(items, "p1").unwrap();
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].provider_id, "p2");
        assert_eq!(prune_usage_comparisons_for_provider(None, "p1"), None);
    }

    /// 契约：后台刷新设置可经 patch 提交（C 项设置页保存路径）。
    #[test]
    fn settings_patch_background_refresh() {
        let mut base = Settings::default();
        assert!(!base.background_refresh_enabled, "默认关闭");
        apply_settings_patch(
            &mut base,
            &SettingsPatch {
                background_refresh_enabled: Some(true),
                background_refresh_interval_minutes: Some(60),
                ..SettingsPatch::default()
            },
        );
        assert!(base.background_refresh_enabled);
        assert_eq!(base.background_refresh_interval_minutes, 60);

        let p: SettingsPatch = serde_json::from_str(
            r#"{"background_refresh_enabled": true, "background_refresh_interval_minutes": 120}"#,
        )
        .unwrap();
        assert_eq!(p.background_refresh_enabled, Some(true));
        assert_eq!(p.background_refresh_interval_minutes, Some(120));
    }
}
/// 将用户预览过的无凭据 AI 诊断包原子写入保存对话框选定路径。
#[tauri::command]
pub fn write_assist_package(path: String, contents: String) -> Result<(), String> {
    quota_core::update::write_atomic_bytes(std::path::Path::new(&path), contents.as_bytes())
        .map_err(|e| format!("写入 AI 诊断包失败：{e}"))
}

fn resolve_quota_cli_path_from(
    current_exe: &std::path::Path,
    resource_dir: Option<&std::path::Path>,
) -> Result<std::path::PathBuf, String> {
    let file_name = format!("quota{}", std::env::consts::EXE_SUFFIX);
    let mut candidates = vec![current_exe.with_file_name(&file_name)];
    if let Some(resources) = resource_dir {
        candidates.push(resources.join("bin").join(&file_name));
        candidates.push(resources.join(&file_name));
    }
    if let Some(path_value) = std::env::var_os("PATH") {
        candidates.extend(
            std::env::split_paths(&path_value)
                // Windows 系统目录自带同名 quota.exe（NTFS 配额工具），
                // 与本项目 CLI 无关且会拼进外部 Agent 的 PowerShell 提示词，排除
                .filter(|dir| !is_system_dir(dir))
                .map(|dir| dir.join(&file_name)),
        );
    }
    for candidate in &candidates {
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .map_err(|e| format!("解析 quota CLI 路径失败：{e}"));
        }
    }
    Err(format!(
        "未找到 quota CLI 可执行文件；已检查：{}",
        candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("；")
    ))
}

/// PATH 条目是否为 Windows 系统目录（System32/SysWOW64，大小写不敏感）。
fn is_system_dir(dir: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        let text = dir.to_string_lossy().to_ascii_lowercase();
        text.ends_with("\\system32") || text.ends_with("\\syswow64")
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        false
    }
}

/// 返回当前开发版或安装包内真实存在的 quota CLI 绝对路径。
#[tauri::command]
pub fn resolve_quota_cli_path(app: AppHandle) -> Result<String, String> {
    let current_exe = std::env::current_exe().map_err(|e| format!("读取当前程序路径失败：{e}"))?;
    let resource_dir = app.path().resource_dir().ok();
    resolve_quota_cli_path_from(&current_exe, resource_dir.as_deref())
        .map(|path| path.to_string_lossy().into_owned())
}
