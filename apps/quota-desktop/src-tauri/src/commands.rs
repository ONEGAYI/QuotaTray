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
) -> Result<(), String> {
    let config = AppConfig::load(config_path).map_err(|e| e.to_string())?;
    quota_core::export_config_to_path(&config, vault, history, export_path)
        .map_err(|e| e.to_string())
}

fn import_configuration_at(
    export_path: &std::path::Path,
    config_path: &std::path::Path,
    vault: &Vault,
) -> Result<quota_core::TransferBundle, String> {
    quota_core::import_config_to_path(export_path, vault, config_path).map_err(|e| e.to_string())
}

/// 导出完整配置（含查询历史）到用户通过系统对话框选定的路径。
#[tauri::command]
pub fn export_configuration(state: State<'_, AppState>, path: String) -> Result<(), String> {
    // 历史读取失败降级为不带历史，导出主任务继续
    let history = match state.history.lock().unwrap().export_rows() {
        Ok(rows) => Some(rows),
        Err(e) => {
            eprintln!("导出携带历史失败（将不含历史数据）：{e}");
            None
        }
    };
    export_configuration_at(
        &state.paths.config(),
        std::path::Path::new(&path),
        &state.vault,
        history.as_deref(),
    )
}

/// 从迁移包整体替换配置（历史幂等合并），清除旧查询快照并通知所有窗口刷新。
#[tauri::command]
pub fn import_configuration(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<usize, String> {
    let bundle = import_configuration_at(
        std::path::Path::new(&path),
        &state.paths.config(),
        &state.vault,
    )?;
    // 迁移包携带的历史行合并进本机历史库（配置已导入成功，失败仅告警）
    if let Some(rows) = &bundle.history {
        if !rows.is_empty() {
            if let Err(e) = state.history.lock().unwrap().merge_rows(rows) {
                eprintln!("导入历史合并失败：{e}");
            }
        }
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
    state.results.write().unwrap().remove(&id);
    after_state_change(&app, &state);
    emit_providers_changed(&app, &id);
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

    let mut entry = ProviderEntry {
        id: TEMPLATE_TEST_ID.into(),
        name: "模板试查".into(),
        kind: ProviderKind::Template(Box::new(config)),
        enabled: true,
        api_key_enc: None,
        base_url,
        pricing: None,
        plan_variant: PlanVariant::Auto,
        use_proxy: false,
    };
    entry
        .set_api_key(&state.vault, &key)
        .map_err(|e| lang.err_encrypt_failed(&e))?;
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

    let mut entry = ProviderEntry {
        id: SCRIPT_TEST_ID.into(),
        name: "脚本试查".into(),
        kind: ProviderKind::Script(Box::new(config)),
        enabled: true,
        api_key_enc: None,
        base_url,
        pricing: None,
        plan_variant: PlanVariant::Auto,
        use_proxy: false,
    };
    entry
        .set_api_key(&state.vault, &key)
        .map_err(|e| lang.err_encrypt_failed(&e))?;
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

    // 代理端口对账自愈：条目开代理而运行态端口丢失（启动加载抖动回退
    // 默认值等）时从磁盘恢复，避免"未配置代理端口"引导错误假阳性。
    // 正常态（内存有端口）仅一次读锁即返回，无磁盘开销。
    if entry.use_proxy {
        crate::state::reconcile_proxy_from_disk(&state);
    }

    let engine = state.engine.read().unwrap().clone();
    let result = engine.query(&state.vault, &entry).await;
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
    if outcome.ok {
        if let (Some(data), Some(at)) = (outcome.data.as_ref(), outcome.at) {
            if let Err(e) = state.history.lock().unwrap().record(&id, data, at) {
                eprintln!("历史记录写入失败：{e}");
            }
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
/// `tray_icon_entry_id` / `update_proxy_port` 为双层 Option：
/// 外层 Some 内层 None = 显式清空（恢复自动/不代理）；字段缺省 = 不动。
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct SettingsPatch {
    pub refresh_interval_minutes: Option<u32>,
    pub low_balance_threshold_percent: Option<u8>,
    pub autostart: Option<bool>,
    pub language: Option<String>,
    pub theme: Option<String>,
    pub ring_units_per_circle: Option<f64>,
    pub tray_icon_entry_id: Option<Option<String>>,
    pub update_check_enabled: Option<bool>,
    pub update_check_time: Option<String>,
    pub update_proxy_port: Option<Option<u16>>,
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
    if let Some(v) = patch.update_check_time.clone() {
        base.update_check_time = v;
    }
    if let Some(v) = patch.update_proxy_port {
        base.update_proxy_port = v;
    }
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

    let old_proxy_port = state.settings.read().unwrap().update_proxy_port;
    settings
        .save(&state.paths.settings())
        .map_err(|e| lang.err_settings_save(&e))?;
    *state.settings.write().unwrap() = settings.clone();
    tray::rebuild(app, state); // 阈值/语言/主题/每圈单位变化即时反映
    // 网络代理端口变更即时生效：热重建查询引擎（读锁内查询继续用旧
    // 客户端跑完，写锁仅在换新实例的瞬间持有）
    if old_proxy_port != settings.update_proxy_port {
        if let Err(e) = crate::state::rebuild_engine(state) {
            eprintln!("代理变更后重建查询引擎失败：{e}");
        }
    }

    if old_autostart != settings.autostart {
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

/// 全量保存设置（设置对话框「保存」按钮）。
#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    persist_settings(&app, &state, settings)
}

/// 局部更新设置：后端读现值 → 应用 patch → 走统一的落盘副作用。
#[tauri::command]
pub fn patch_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<(), String> {
    let mut current = state.settings.read().unwrap().clone();
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

// ---- 更新检测（core::update 的薄封装） -------------------------------------

/// 当前更新状态（版本 / 上次检测 / 新版本信息 / 最近错误）。
#[tauri::command]
pub fn get_update_state(
    state: State<'_, AppState>,
) -> Result<crate::update_ctl::UpdateStateDto, String> {
    Ok(crate::update_ctl::dto_of(&state.update_ctl.read().unwrap()))
}

/// 手动检测（设置页「立即检查」）：不受节流限制，检测后重建托盘菜单
/// （新版本信息行即时出现）。
#[tauri::command]
pub async fn check_update_now(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::update_ctl::UpdateStateDto, String> {
    let lang = lang_of(&state);
    let proxy = crate::update_ctl::proxy_url(&state);
    let http = quota_core::http::ReqwestHttpClient::new_with_proxy(
        std::time::Duration::from_secs(10),
        proxy.as_deref(),
    )
    .map_err(|e| lang.err_update_client(&e))?;
    let inner = crate::update_ctl::run_check(&state, &http).await;
    tray::rebuild(&app, &state);
    Ok(crate::update_ctl::dto_of(&inner))
}

/// 下载安装包到 %TEMP%/QuotaTray/Downloads 并记录进状态表，返回完整路径。
#[tauri::command]
pub async fn download_update(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let lang = lang_of(&state);
    crate::update_ctl::download_installer(&app, &state, lang).await
}

/// 运行已下载的安装包（NSIS 向导由用户交互完成）。启动成功后应用自动
/// 退出——覆盖安装需先解锁自身文件；留 400ms 让 IPC 响应送达前端。
#[tauri::command]
pub fn install_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let lang = lang_of(&state);
    crate::update_ctl::run_installer(&state, lang)?;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        app.exit(0);
    });
    Ok(())
}

// ---- 契约测试 -------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
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
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
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

        export_configuration_at(&source_path, &bundle, &source_vault, None).unwrap();
        let imported = import_configuration_at(&bundle, &target_path, &target_vault).unwrap();
        assert_eq!(imported.config.providers.len(), 1);
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
        export_configuration_at(&source_path, &bundle, &source_vault, Some(&rows)).unwrap();
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
        assert_eq!(base.language, "zh");
        assert_eq!(base.tray_icon_entry_id, Some("A1B2C3".into()));
        assert_eq!(base.ring_units_per_circle, 500.0);

        // 双层 Option：显式清空（Some(None)）与不动（缺省）可区分
        apply_settings_patch(
            &mut base,
            &SettingsPatch {
                tray_icon_entry_id: Some(None),
                update_proxy_port: Some(None),
                ..SettingsPatch::default()
            },
        );
        assert_eq!(base.tray_icon_entry_id, None, "Some(None) 显式清空");
        assert_eq!(base.update_proxy_port, None, "Some(None) 显式清空");
        assert_eq!(base.theme, "dark", "其余字段仍不动");
    }
}
