//! IPC 命令层：core API 的薄封装 + 托盘/快照副作用。
//!
//! 安全约定（红线 3）：`upsert_provider` 忽略前端传入的密文字段，
//! key 只走「写入专用」通道——`new_api_key` 非空加密落盘，空/缺省保留旧密文，
//! 明文 key 永不回传前端。

use std::collections::BTreeMap;

use quota_core::template::{TemplateConfig, TemplateError};
use quota_core::{AppConfig, ProviderEntry, ProviderKind, Vault};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::settings::Settings;
use crate::snapshot::{SnapshotEntry, Snapshots};
use crate::state::{AppState, EntryState, ErrorInfo, QueryOutcome, now_ms};
use crate::tray;

/// 模板试查临时条目 id（AAD 绑定值，试查不落任何持久状态）。
const TEMPLATE_TEST_ID: &str = "template-test";

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
) -> Result<(), String> {
    let trimmed = new_api_key.map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        Some(key) => entry
            .set_api_key(vault, key)
            .map_err(|e| format!("凭据加密失败：{e}")),
        None => {
            entry.api_key_enc = existing.and_then(|e| e.api_key_enc.clone());
            Ok(())
        }
    }
}

/// 模板是否引用了 `{{apiKey}}`（决定试查时 key 是否必填）。
pub fn template_needs_api_key(config: &TemplateConfig) -> bool {
    let marker = "{{apiKey}}";
    config.request.url.contains(marker)
        || config.request.headers.values().any(|v| v.contains(marker))
        || config
            .request
            .body
            .as_deref()
            .is_some_and(|b| b.contains(marker))
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
fn after_state_change(app: &AppHandle, state: &AppState) {
    let snaps = snapshots_from_results(&state.results.read().unwrap());
    if let Err(e) = snaps.save(&state.paths.snapshot()) {
        eprintln!("快照写入失败：{e}");
    }
    tray::rebuild(app, state);
}

// ---- 命令 -----------------------------------------------------------------

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
    if entry.id.trim().is_empty() || entry.name.trim().is_empty() {
        return Err("id 与名称不能为空".into());
    }
    // 保存前校验：模板静态校验（带字段定位）、native id 存在性
    match &entry.kind {
        ProviderKind::Template(t) => {
            quota_core::template::validate(t).map_err(|e| e.to_string())?;
        }
        ProviderKind::Native { provider } => {
            if quota_core::provider::find(provider).is_none() {
                return Err(format!("未知的预置平台 id：{provider}"));
            }
        }
    }

    let mut cfg = AppConfig::load(&state.paths.config()).map_err(|e| e.to_string())?;
    let existing = cfg.providers.iter().find(|p| p.id == entry.id).cloned();
    let mut entry = entry;
    apply_key_policy(
        &mut entry,
        existing.as_ref(),
        new_api_key.as_deref(),
        &state.vault,
    )?;

    // 编辑保位 / 新增追加
    match cfg.providers.iter_mut().find(|p| p.id == entry.id) {
        Some(slot) => *slot = entry,
        None => cfg.providers.push(entry),
    }
    cfg.save(&state.paths.config()).map_err(|e| e.to_string())?;

    // 条目已变，旧查询结果作废
    state.results.write().unwrap().clear();
    after_state_change(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn remove_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let mut cfg = AppConfig::load(&state.paths.config()).map_err(|e| e.to_string())?;
    let before = cfg.providers.len();
    cfg.providers.retain(|p| p.id != id);
    if cfg.providers.len() == before {
        return Err(format!("条目 {id} 不存在"));
    }
    cfg.save(&state.paths.config()).map_err(|e| e.to_string())?;
    state.results.write().unwrap().remove(&id);
    after_state_change(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn list_native_metas() -> Vec<NativeMetaDto> {
    quota_core::provider::metas()
        .into_iter()
        .map(|m| NativeMetaDto {
            id: m.id.into(),
            name: m.name.into(),
        })
        .collect()
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
    let config: TemplateConfig =
        serde_json::from_str(&config_json).map_err(|e| format!("模板 JSON 解析失败：{e}"))?;
    let key = match api_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(k) => k.to_string(),
        None if template_needs_api_key(&config) => {
            return Ok(QueryOutcome {
                ok: false,
                data: None,
                error: Some(ErrorInfo {
                    kind: "deterministic".into(),
                    message: "模板引用了 {{apiKey}}，试查前请填写 API key".into(),
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
    };
    entry
        .set_api_key(&state.vault, &key)
        .map_err(|e| format!("凭据加密失败：{e}"))?;
    entry.base_url = entry.base_url.filter(|u| !u.trim().is_empty());

    let outcome = match state.engine.query(&state.vault, &entry).await {
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

/// 查询单条目：成功更新结果表与快照并重建托盘；失败按双轨分类透出，
/// 结果表保留最后一次成功数据（keep-last-good 数据源）。
#[tauri::command]
pub async fn query_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<QueryOutcome, String> {
    let cfg = AppConfig::load(&state.paths.config()).map_err(|e| e.to_string())?;
    let entry = cfg
        .providers
        .iter()
        .find(|p| p.id == id && p.enabled)
        .ok_or_else(|| format!("条目 {id} 不存在或未启用"))?
        .clone();

    let result = state.engine.query(&state.vault, &entry).await;
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
    after_state_change(&app, &state);
    Ok(outcome)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.read().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    let mut settings = settings;
    settings.sanitize();
    let autostart_changed = {
        let mut cur = state.settings.write().unwrap();
        let changed = cur.autostart != settings.autostart;
        *cur = settings.clone();
        changed
    };
    if autostart_changed {
        apply_autostart(&app, settings.autostart)?;
    }
    settings
        .save(&state.paths.settings())
        .map_err(|e| e.to_string())?;
    tray::rebuild(&app, &state); // 阈值变化影响告警图标
    Ok(())
}

fn apply_autostart(app: &AppHandle, enable: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    if enable {
        autolaunch
            .enable()
            .map_err(|e| format!("开启开机自启失败：{e}"))
    } else {
        autolaunch
            .disable()
            .map_err(|e| format!("关闭开机自启失败：{e}"))
    }
}

/// 启动渲染用快照（与 cache.json 同形状）。
#[tauri::command]
pub fn get_snapshots(
    state: State<'_, AppState>,
) -> Result<BTreeMap<String, SnapshotEntry>, String> {
    Ok(snapshots_from_results(&state.results.read().unwrap()).entries)
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
        }
    }

    /// 契约：新 key 加密写入；密文非明文且以 v1: 开头。
    #[test]
    fn key_policy_replaces_with_ciphertext() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut e = entry("p1");
        let existing = None;
        apply_key_policy(&mut e, existing, Some("sk-plain"), &vault).unwrap();
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
        apply_key_policy(&mut e, Some(&old), Some("   "), &vault).unwrap();
        assert_eq!(e.api_key_enc, old_enc, "空 key 应保留旧密文");

        apply_key_policy(&mut e, Some(&old), None, &vault).unwrap();
        assert_eq!(e.api_key_enc, old_enc, "缺省 key 应保留旧密文");
    }

    /// 契约：新条目 + 空 key → 无密文（后续查询报"未配置 API key"确定性错误）。
    #[test]
    fn key_policy_new_entry_without_key_is_none() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut e = entry("new");
        e.api_key_enc = Some("v1:forged".into());
        apply_key_policy(&mut e, None, Some(""), &vault).unwrap();
        assert!(e.api_key_enc.is_none());
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
                }),
            },
        );
        results.insert(
            "never".into(),
            EntryState {
                error: Some(ErrorInfo {
                    kind: "deterministic".into(),
                    message: "y".into(),
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
}
