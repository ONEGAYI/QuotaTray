//! 更新检测控制器：状态表、检测执行、安装包下载与每分钟调度。
//!
//! 业务逻辑（版本比较/release 解析/节流判定）在 core::update；本模块只做
//! GUI 侧编排——状态表维护、settings 节流时间戳落盘、下载到系统下载
//! 目录、常驻调度（每分钟 wake 读设置判定，设置变更自然生效免任务重启）。

use std::time::Duration;

use quota_core::http::{HttpClient, ReqwestHttpClient};
use quota_core::update::{self, AssetDownloader, ReqwestAssetDownloader, UpdateStatus, VERSION};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::i18n::Lang;
use crate::state::{AppState, now_ms};
use crate::tray;

/// 有新版本时的展示信息（IPC 形状；`asset_url` 仅后端下载用，前端可忽略）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AvailableInfo {
    pub version: String,
    pub html_url: String,
    pub notes: Option<String>,
    pub asset_name: Option<String>,
    pub asset_size: Option<u64>,
    /// release 是否带可下载安装包（无则引导去发布页）。
    pub downloadable: bool,
    /// 安装包直链（None = downloadable=false）。
    pub asset_url: Option<String>,
}

/// 更新检测的展示状态。
///
/// 节流判定的权威源是 `settings.update_last_check`（磁盘），
/// 此处 `last_check` 是展示镜像，两者在 [`run_check`] 中同步更新。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpdateCtlState {
    pub last_check: Option<u64>,
    pub info: Option<AvailableInfo>,
    pub last_error: Option<String>,
}

/// `get_update_state` 的 IPC 返回形状（含当前版本）。
#[derive(Debug, Clone, Serialize)]
pub struct UpdateStateDto {
    pub current_version: &'static str,
    pub last_check: Option<u64>,
    pub available: Option<AvailableInfo>,
    pub last_error: Option<String>,
}

pub fn dto_of(inner: &UpdateCtlState) -> UpdateStateDto {
    UpdateStateDto {
        current_version: VERSION,
        last_check: inner.last_check,
        available: inner.info.clone(),
        last_error: inner.last_error.clone(),
    }
}

/// 执行一次检测（http 注入便于测试）：更新状态表 + settings 节流时间戳
/// 落盘。托盘重建留给调用方（手动检测与调度任务都重建，时机各自掌控）。
/// 检测失败（网络/解析）记入 `last_error` 而非中断——自动场景静默可查。
pub async fn run_check(state: &AppState, http: &dyn HttpClient) -> UpdateCtlState {
    let now = now_ms();
    let inner = match update::check_update(http, VERSION).await {
        Ok(UpdateStatus::Available {
            version,
            html_url,
            notes,
            asset,
        }) => UpdateCtlState {
            last_check: Some(now),
            info: Some(AvailableInfo {
                version,
                html_url,
                notes,
                asset_name: asset.as_ref().map(|a| a.name.clone()),
                asset_size: asset.as_ref().map(|a| a.size),
                downloadable: asset.is_some(),
                asset_url: asset.map(|a| a.browser_download_url),
            }),
            last_error: None,
        },
        // 无 release / 已最新：清掉旧的新版本信息（跨版本状态不残留）
        Ok(_) => UpdateCtlState {
            last_check: Some(now),
            info: None,
            last_error: None,
        },
        Err(e) => UpdateCtlState {
            last_check: Some(now),
            info: None,
            last_error: Some(e.to_string()),
        },
    };

    // settings 节流时间戳落盘（磁盘权威顺序：clone → 改 → save 成功 → 写回内存）
    {
        let mut s = state.settings.read().unwrap().clone();
        s.update_last_check = Some(now);
        match s.save(&state.paths.settings()) {
            Ok(()) => *state.settings.write().unwrap() = s,
            // 非关键失败：仅节流精度回退（下次可能多检一次），不阻断
            Err(e) => eprintln!("更新检测时间戳写入失败：{e}"),
        }
    }
    *state.update_ctl.write().unwrap() = inner.clone();
    inner
}

/// 下载安装包到系统下载目录（原子写），返回完整路径。
/// 前提：状态表里已有「可下载的新版本」（先检测后下载）。
pub async fn download_installer(state: &AppState, lang: Lang) -> Result<String, String> {
    let info = state.update_ctl.read().unwrap().info.clone();
    let Some(info) = info else {
        return Err(lang.err_update_not_checked());
    };
    let Some(url) = info.asset_url else {
        return Err(lang.err_update_no_asset());
    };
    let bytes = ReqwestAssetDownloader::new()
        .download(&url)
        .await
        .map_err(|e| lang.err_update_download(&e))?;
    let name = info
        .asset_name
        .unwrap_or_else(|| "QuotaTray-setup.exe".into());
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| lang.err_update_no_dir())?;
    let path = dir.join(&name);
    update::write_atomic_bytes(&path, &bytes).map_err(|e| lang.err_update_save(&e))?;
    Ok(path.to_string_lossy().into_owned())
}

/// 常驻调度：每分钟 wake 一次读设置，`due_check`（首启 ≥24h 节流或每日
/// 到点）为真则检测并重建托盘。设置变更（开关/时刻/手动检测过的节流
/// 时间戳）在下个 wake 自然生效——无需重启任务。
pub fn spawn_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            {
                let state = app.state::<AppState>();
                let (enabled, last, time) = {
                    let s = state.settings.read().unwrap();
                    (
                        s.update_check_enabled,
                        s.update_last_check,
                        s.update_check_time.clone(),
                    )
                };
                if quota_core::update::due_check(enabled, last, &time, now_ms()) {
                    if let Ok(http) = ReqwestHttpClient::new(Duration::from_secs(10)) {
                        run_check(&state, &http).await;
                        tray::rebuild(&app, &state);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use async_trait::async_trait;
    use quota_core::http::{HttpError, HttpRequest, HttpResponse};
    use std::collections::HashMap;

    /// 按 URL 子串路由的 mock（CLI RouteHttp 同款）。
    struct RouteHttp {
        routes: Vec<(&'static str, u16, String)>,
    }

    #[async_trait]
    impl HttpClient for RouteHttp {
        async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
            for (frag, status, body) in &self.routes {
                if req.url.contains(frag) {
                    return Ok(HttpResponse {
                        status: *status,
                        body: body.clone(),
                    });
                }
            }
            Err(HttpError::Network("no route".into()))
        }
    }

    /// AppState 依赖 keyring/reqwest，测试以「最小伪状态」验证 run_check 的
    /// 状态表更新语义（节流落盘与 rebuild 副作用属集成面，靠冒烟覆盖）：
    /// 此处以纯数据验证 check_update → UpdateCtlState 的映射。
    #[tokio::test]
    async fn status_mapping_semantics() {
        let mk = |status: UpdateStatus| match status {
            UpdateStatus::Available {
                version,
                html_url,
                notes,
                asset,
            } => UpdateCtlState {
                last_check: Some(1),
                info: Some(AvailableInfo {
                    version,
                    html_url,
                    notes,
                    asset_name: asset.as_ref().map(|a| a.name.clone()),
                    asset_size: asset.as_ref().map(|a| a.size),
                    downloadable: asset.is_some(),
                    asset_url: asset.map(|a| a.browser_download_url),
                }),
                last_error: None,
            },
            _ => UpdateCtlState {
                last_check: Some(1),
                info: None,
                last_error: None,
            },
        };
        // 有资产的新版本 → downloadable
        let s = mk(UpdateStatus::Available {
            version: "0.2.0".into(),
            html_url: "u".into(),
            notes: None,
            asset: Some(quota_core::update::ReleaseAsset {
                name: "setup.exe".into(),
                browser_download_url: "dl".into(),
                size: 7,
            }),
        });
        assert!(s.info.as_ref().is_some_and(|i| i.downloadable));
        // 无资产 → 不可下载
        let s = mk(UpdateStatus::Available {
            version: "0.2.0".into(),
            html_url: "u".into(),
            notes: None,
            asset: None,
        });
        assert!(s.info.as_ref().is_some_and(|i| !i.downloadable));
        // NoRelease/UpToDate → 无 info
        assert!(mk(UpdateStatus::NoRelease).info.is_none());
        assert!(mk(UpdateStatus::UpToDate).info.is_none());
    }

    /// 契约：run_check 端到端更新状态表 + settings 节流时间戳落盘。
    /// AppState 构造绕不开 keyring（生产 store），用系统集成桩：
    /// AppState 字段全 pub，直接手工组装（engine/vault 用最小真实现）。
    #[tokio::test]
    async fn run_check_updates_state_and_settings_timestamp() {
        // 手工组装 AppState：InMemoryStore + 默认引擎（不触网）
        let dir = std::env::temp_dir().join(format!("qt-updctl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = crate::state::DataPaths::new(Some(dir.clone())).unwrap();
        let vault = quota_core::Vault::open(&quota_core::InMemoryStore::new()).unwrap();
        let engine = quota_core::QueryEngine::with_default_client().unwrap();
        let settings = Settings::default();
        settings.save(&paths.settings()).unwrap();
        let state = AppState {
            engine,
            vault,
            paths,
            settings: std::sync::RwLock::new(settings),
            results: std::sync::RwLock::new(HashMap::new()),
            last_hover_refresh_ms: std::sync::atomic::AtomicU64::new(0),
            resolved_theme: std::sync::RwLock::new(false),
            update_ctl: std::sync::RwLock::new(UpdateCtlState::default()),
        };

        // 无 release（404）→ last_check 记录、无 info、无错误
        let http = RouteHttp {
            routes: vec![("releases/latest", 404, "".into())],
        };
        let inner = run_check(&state, &http).await;
        assert!(inner.last_check.is_some(), "检测过即记录时间戳");
        assert!(inner.info.is_none());
        assert!(inner.last_error.is_none());
        assert!(
            state.settings.read().unwrap().update_last_check.is_some(),
            "settings 节流时间戳已落盘并写回内存"
        );
        assert!(
            Settings::load(&state.paths.settings())
                .update_last_check
                .is_some(),
            "时间戳真实落盘"
        );

        // 网络失败 → last_error 记录
        let http = RouteHttp { routes: vec![] };
        let inner = run_check(&state, &http).await;
        assert!(
            inner.last_error.is_some(),
            "网络失败进 last_error 而非 panic"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
