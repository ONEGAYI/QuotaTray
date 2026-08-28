//! 更新检测控制器：状态表、检测执行、安装包下载/运行与每分钟调度。
//!
//! 业务逻辑（版本比较/release 解析/节流判定）在 core::update；本模块只做
//! GUI 侧编排——状态表维护、settings 节流时间戳落盘、下载到系统临时
//! 目录（`%TEMP%/QuotaTray/Downloads`）、运行安装包（NSIS 向导由用户
//! 交互完成，应用随后退出解锁自身文件）、常驻调度（每分钟 wake 读设置
//! 判定，设置变更自然生效免任务重启）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use quota_core::http::{HttpClient, ReqwestHttpClient};
use quota_core::update::{
    self, AssetDownloader, DownloadProgress, DownloadProgressReporter, ReqwestAssetDownloader,
    UpdateStatus, VERSION,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

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

/// 已下载安装包的记录：路径 + 对应 release 资产名（检测到不同版本资产
/// 时据此失效，避免对着旧安装包提供「立即安装」入口）。
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadedInstaller {
    pub path: String,
    pub asset_name: String,
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
    /// `last_error` 的详情（如限流 403 的 GitHub message）——主文案保持
    /// 简短，详情由前端悬停展示；非状态类错误无详情（None）。
    pub last_error_detail: Option<String>,
    pub downloaded: Option<DownloadedInstaller>,
}

/// `get_update_state` 的 IPC 返回形状（含当前版本与运行形态）。
#[derive(Debug, Clone, Serialize)]
pub struct UpdateStateDto {
    pub current_version: &'static str,
    /// 运行架构标签（x64 / ARM64，编译期确定，与 CLI --version 共用）。
    pub platform: &'static str,
    /// 便携形态（数据根存在 portable.key；安装版恒 false）。
    pub portable: bool,
    /// 当前构建使用 zip 手动覆盖更新（x64 Portable 与两种 ARM64 Preview）。
    pub manual_update: bool,
    pub last_check: Option<u64>,
    pub available: Option<AvailableInfo>,
    pub last_error: Option<String>,
    /// `last_error` 的详情（悬停展示；None = 无详情）。
    pub last_error_detail: Option<String>,
    /// 后端自记录的已下载安装包路径；能否安装以其与当前 available
    /// 资产匹配为准（检测到不同版本时由失效逻辑清空）。
    pub downloaded_path: Option<String>,
}

pub fn dto_of(inner: &UpdateCtlState, portable: bool) -> UpdateStateDto {
    let selector = update::AssetSelector::for_runtime(update::arch_label(), portable);
    UpdateStateDto {
        current_version: VERSION,
        platform: update::arch_label(),
        portable,
        manual_update: selector.requires_manual_update(),
        last_check: inner.last_check,
        available: inner.info.clone(),
        last_error: inner.last_error.clone(),
        last_error_detail: inner.last_error_detail.clone(),
        downloaded_path: inner.downloaded.as_ref().map(|d| d.path.clone()),
    }
}

/// 安装包下载目录：`%TEMP%/QuotaTray/Downloads`。临时目录语义——一次性
/// 安装包随系统清理自然回收，丢失重下即可恢复。
pub fn installer_dir() -> PathBuf {
    std::env::temp_dir().join("QuotaTray").join("Downloads")
}

/// 检测成功后已下载记录的去留：新版本资产名与已下载一致才保留
/// （换版本/已最新/无资产 → 清空）。检测失败不走此函数——网络故障
/// 不应丢已下载状态（重连补检后可直接安装，无需重下）。
fn carry_downloaded(
    prev: Option<DownloadedInstaller>,
    new_info: Option<&AvailableInfo>,
) -> Option<DownloadedInstaller> {
    match (prev, new_info) {
        (Some(d), Some(info)) if info.asset_name.as_deref() == Some(d.asset_name.as_str()) => {
            Some(d)
        }
        _ => None,
    }
}

/// 前端监听的安装包下载进度事件。
pub const DOWNLOAD_PROGRESS_EVENT: &str = "update-download-progress";
/// 自动调度完成检测后推送完整状态，已打开的设置页可立即刷新。
pub const UPDATE_STATE_EVENT: &str = "update-state-changed";

/// 更新通道代理 URL（settings 端口 → `http://127.0.0.1:{port}`；None = 直连）。
/// 检测与下载共用同一设置项。
pub(crate) fn proxy_url(state: &AppState) -> Option<String> {
    quota_core::update::proxy_url_of(state.settings.read().unwrap().update_proxy_port)
}

/// 检测错误的悬停详情：仅状态类错误（[`update::UpdateError::HttpStatus`]）
/// 携带响应体 message，其余（网络/解析）无详情。
fn error_detail(e: &update::UpdateError) -> Option<String> {
    match e {
        update::UpdateError::HttpStatus { detail, .. } => detail.clone(),
        _ => None,
    }
}

struct TauriProgressReporter<'a> {
    app: &'a AppHandle,
}

impl DownloadProgressReporter for TauriProgressReporter<'_> {
    fn report(&self, progress: DownloadProgress) {
        // 窗口未打开或退出途中没有监听者都不应中断下载。
        let _ = self.app.emit(DOWNLOAD_PROGRESS_EVENT, progress);
    }
}

/// 执行一次检测（http 注入便于测试）：更新状态表 + settings 节流时间戳
/// 落盘。托盘重建留给调用方（手动检测与调度任务都重建，时机各自掌控）。
/// 检测失败（网络/解析）记入 `last_error` 而非中断——自动场景静默可查。
pub async fn run_check(state: &AppState, http: &dyn HttpClient) -> UpdateCtlState {
    let now = now_ms();
    let prev_downloaded = state.update_ctl.read().unwrap().downloaded.clone();
    // 资产选择按架构 × 运行形态分流，绝不跨形态回退。
    let selector =
        update::AssetSelector::for_runtime(update::arch_label(), state.mode.is_portable());
    let mut inner = match update::check_update(http, VERSION, selector).await {
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
            last_error_detail: None,
            downloaded: None,
        },
        // 无 release / 已最新：清掉旧的新版本信息（跨版本状态不残留）
        Ok(_) => UpdateCtlState {
            last_check: Some(now),
            info: None,
            last_error: None,
            last_error_detail: None,
            downloaded: None,
        },
        // 检测失败保留已下载记录：网络故障不应丢状态（成功路径在下方
        // 统一按资产名重判）。主文案用 Display（简短），限流等原因
        // detail 单独携带——前端主文字不变，悬停展示完整信息。
        Err(e) => UpdateCtlState {
            last_check: Some(now),
            info: None,
            last_error: Some(e.to_string()),
            last_error_detail: error_detail(&e),
            downloaded: prev_downloaded.clone(),
        },
    };
    if inner.last_error.is_none() {
        inner.downloaded = carry_downloaded(prev_downloaded, inner.info.as_ref());
    }

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

/// 下载安装包（进度推送给前端）并落盘记录，返回完整路径。
/// 前提：状态表里已有「可下载的新版本」（先检测后下载）。
pub async fn download_installer(
    app: &AppHandle,
    state: &AppState,
    lang: Lang,
) -> Result<String, String> {
    let info = state.update_ctl.read().unwrap().info.clone();
    let Some(info) = info else {
        return Err(lang.err_update_not_checked());
    };
    let Some(url) = info.asset_url else {
        return Err(lang.err_update_no_asset());
    };
    let reporter = TauriProgressReporter { app };
    let downloader = ReqwestAssetDownloader::try_with_proxy(proxy_url(state).as_deref())
        .map_err(|e| lang.err_update_client(&e))?;
    let bytes = downloader
        .download_with_progress(&url, &reporter)
        .await
        .map_err(|e| lang.err_update_download(&e))?;
    // asset_name 与 asset_url 同源（downloadable 判定），None 属防御分支
    let Some(name) = info.asset_name else {
        return Err(lang.err_update_no_asset());
    };
    save_installer(state, &name, &bytes, lang)
}

/// release 资产名写入侧校验：必须是纯文件名（不含路径分隔符/盘符
/// 冒号，杜绝 `..\` 上跳与 NTFS ADS 形态）且以 `.exe`/`.zip` 结尾
/// （zip = 便携形态更新资产）——防恶意资产名使落盘位置逃出下载目录
/// （运行安装侧另有 exe-only 的 validate_installer_path）。
fn validate_asset_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && (lower.ends_with(".exe") || lower.ends_with(".zip"))
}

/// 安装包字节落盘到 [`installer_dir`]（原子写）并记录进状态表。
fn save_installer(
    state: &AppState,
    name: &str,
    bytes: &[u8],
    lang: Lang,
) -> Result<String, String> {
    if !validate_asset_name(name) {
        return Err(lang.err_update_bad_asset());
    }
    let dir = installer_dir();
    std::fs::create_dir_all(&dir).map_err(|e| lang.err_update_mkdir(&e))?;
    // 纵深防御：%TEMP% 为用户态任意进程可写区，目录若被预置为指向
    // 他处的 symlink/junction，写入会跟随逃逸——检测到即拒绝
    if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(lang.err_update_unsafe_dir());
    }
    let path = dir.join(name);
    update::write_atomic_bytes(&path, bytes).map_err(|e| lang.err_update_save(&e))?;
    let path_str = path.to_string_lossy().into_owned();
    state.update_ctl.write().unwrap().downloaded = Some(DownloadedInstaller {
        path: path_str.clone(),
        asset_name: name.to_string(),
    });
    Ok(path_str)
}

/// 安装包路径防御校验：必须直接位于下载目录内且为 `.exe`。路径由后端
/// 自己拼装，此处兜底防御状态表异常值被运行。
fn validate_installer_path(path: &Path) -> bool {
    path.parent() == Some(installer_dir().as_path())
        && path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

/// 运行已下载的安装包（NSIS 向导由用户交互完成）。覆盖安装需先解锁
/// 自身文件，调用方（install_update 命令）在启动成功后退出应用。
/// 文件已丢失（如临时目录被系统清理）时清空记录并报错——前端刷新后
/// 自动回到「下载安装包」状态。
pub fn run_installer(state: &AppState, lang: Lang) -> Result<(), String> {
    let downloaded = state.update_ctl.read().unwrap().downloaded.clone();
    let Some(d) = downloaded else {
        return Err(lang.err_update_not_downloaded());
    };
    let path = PathBuf::from(&d.path);
    if !validate_installer_path(&path) || !path.is_file() {
        state.update_ctl.write().unwrap().downloaded = None;
        return Err(lang.err_update_installer_missing());
    }
    std::process::Command::new(&path)
        .spawn()
        .map_err(|e| lang.err_update_run(&e))?;
    Ok(())
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
                    if let Ok(http) = ReqwestHttpClient::new_with_proxy(
                        Duration::from_secs(10),
                        proxy_url(&state).as_deref(),
                    ) {
                        let inner = run_check(&state, &http).await;
                        let _ =
                            app.emit(UPDATE_STATE_EVENT, dto_of(&inner, state.mode.is_portable()));
                        tray::rebuild(&app, &state);
                    }
                }
                // 峰谷标签过期兜底：任一启用条目峰/谷翻转才重建并广播（内部自比对）
                tray::rebuild_on_peak_flip(&app, &state);
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

    /// 手工组装最小 AppState（AppState 依赖 keyring，测试绕开生产构造）。
    fn sandbox_state(dir: &Path) -> AppState {
        let paths = crate::state::DataPaths::new(Some(dir.to_path_buf())).unwrap();
        let vault = quota_core::Vault::open(&quota_core::InMemoryStore::new()).unwrap();
        let engine = quota_core::QueryEngine::with_default_client().unwrap();
        AppState {
            mode: quota_core::RuntimeMode::Installed {
                data_dir: Some(dir.to_path_buf()),
            },
            engine: std::sync::RwLock::new(engine),
            vault,
            paths,
            settings: std::sync::RwLock::new(Settings::default()),
            results: std::sync::RwLock::new(HashMap::new()),
            resolved_theme: std::sync::RwLock::new(false),
            update_ctl: std::sync::RwLock::new(UpdateCtlState::default()),
            last_peak: std::sync::RwLock::new(HashMap::new()),
            // 更新调度测试不消费历史，内存库即可
            history: std::sync::Mutex::new(quota_core::HistoryStore::open_in_memory().unwrap()),
        }
    }

    #[async_trait]
    impl HttpClient for RouteHttp {
        async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
            for (frag, status, body) in &self.routes {
                if req.url.contains(frag) {
                    return Ok(HttpResponse {
                        status: *status,
                        body: body.clone(),
                        raw: Vec::new(),
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
                last_error_detail: None,
                downloaded: None,
            },
            _ => UpdateCtlState {
                last_check: Some(1),
                info: None,
                last_error: None,
                last_error_detail: None,
                downloaded: None,
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

    /// 契约：便携形态的检测只命中 portable zip——同一 release 同时含
    /// setup.exe 也不回退（形态分流，命名契约见 core::update）。
    #[tokio::test]
    async fn run_check_portable_mode_selects_portable_zip() {
        let dir = std::env::temp_dir().join(format!("qt-updctl-port-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut state = sandbox_state(&dir);
        let data_root = dir.join("Data");
        std::fs::create_dir_all(&data_root).unwrap();
        state.mode = quota_core::RuntimeMode::Portable {
            root: data_root.clone(),
        };

        let zip =
            update::expected_asset_name("9.9.9", update::arch_label(), update::Flavor::PortableZip);
        let setup =
            update::expected_asset_name("9.9.9", update::arch_label(), update::Flavor::SetupExe);
        let body = format!(
            r#"{{"tag_name":"v9.9.9","html_url":"u","assets":[
                {{"name":"{setup}","browser_download_url":"https://x/setup","size":1}},
                {{"name":"{zip}","browser_download_url":"https://x/zip","size":2}}
            ]}}"#
        );
        let http = RouteHttp {
            routes: vec![("releases/latest", 200, body)],
        };
        let inner = run_check(&state, &http).await;
        assert_eq!(
            inner.info.as_ref().and_then(|i| i.asset_name.as_deref()),
            Some(zip.as_str()),
            "便携形态命中 portable zip 而非 setup.exe"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 契约：run_check 端到端更新状态表 + settings 节流时间戳落盘；
    /// 已下载记录在检测失败时保留、成功且无匹配资产时清空。
    #[tokio::test]
    async fn run_check_updates_state_and_settings_timestamp() {
        let dir = std::env::temp_dir().join(format!("qt-updctl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = sandbox_state(&dir);
        state
            .settings
            .read()
            .unwrap()
            .save(&state.paths.settings())
            .unwrap();

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

        // 预置已下载记录：检测失败（网络）→ 保留
        let downloaded = DownloadedInstaller {
            path: installer_dir()
                .join("setup.exe")
                .to_string_lossy()
                .into_owned(),
            asset_name: "setup.exe".into(),
        };
        state.update_ctl.write().unwrap().downloaded = Some(downloaded.clone());
        let http = RouteHttp { routes: vec![] };
        let inner = run_check(&state, &http).await;
        assert!(
            inner.last_error.is_some(),
            "网络失败进 last_error 而非 panic"
        );
        assert_eq!(inner.downloaded, Some(downloaded), "网络失败不丢已下载记录");

        // 成功且无新版本（404）→ 已下载记录清空
        let http = RouteHttp {
            routes: vec![("releases/latest", 404, "".into())],
        };
        let inner = run_check(&state, &http).await;
        assert!(inner.last_error.is_none());
        assert_eq!(inner.downloaded, None, "已最新时旧安装包记录失效");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：状态类检测错误（限流 403）——主文案保持简短 Display，
    /// 响应体 message 作为 detail 进状态表与 DTO；成功检测后两者一并清空。
    #[tokio::test]
    async fn run_check_status_error_carries_detail() {
        let dir = std::env::temp_dir().join(format!("qt-updctl-det-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = sandbox_state(&dir);

        let http = RouteHttp {
            routes: vec![(
                "releases/latest",
                403,
                r#"{"message":"API rate limit exceeded for 1.2.3.4."}"#.into(),
            )],
        };
        let inner = run_check(&state, &http).await;
        assert_eq!(
            inner.last_error.as_deref(),
            Some("网络错误：HTTP 403"),
            "主文案与历史 Display 一致"
        );
        assert_eq!(
            inner.last_error_detail.as_deref(),
            Some("API rate limit exceeded for 1.2.3.4.")
        );
        let dto = dto_of(&inner, false);
        assert_eq!(dto.last_error_detail, inner.last_error_detail, "DTO 透传");

        // 成功检测（404 无 release）后错误与详情一并清空
        let http = RouteHttp {
            routes: vec![("releases/latest", 404, "".into())],
        };
        let inner = run_check(&state, &http).await;
        assert_eq!(inner.last_error, None);
        assert_eq!(inner.last_error_detail, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：下载目录固定为系统临时目录下 QuotaTray/Downloads。
    #[test]
    fn installer_dir_contract() {
        assert_eq!(
            installer_dir(),
            std::env::temp_dir().join("QuotaTray").join("Downloads")
        );
    }

    /// 契约：安装包路径校验——仅下载目录内直接的 .exe 放行。
    #[test]
    fn validate_installer_path_contract() {
        let dir = installer_dir();
        assert!(validate_installer_path(
            &dir.join("QuotaTray_0.4.0_x64-setup.exe")
        ));
        assert!(
            validate_installer_path(&dir.join("setup.EXE")),
            "扩展名大小写不敏感"
        );
        assert!(!validate_installer_path(&dir.join("payload.zip")));
        assert!(
            !validate_installer_path(&dir.join("sub").join("setup.exe")),
            "嵌套子目录不放行"
        );
        assert!(
            !validate_installer_path(&dir.join("../evil.exe")),
            "越出下载目录不放行"
        );
    }

    /// 契约：检测成功后已下载记录仅在资产名一致时保留。
    #[test]
    fn carry_downloaded_contract() {
        let d = || {
            Some(DownloadedInstaller {
                path: "p".into(),
                asset_name: "setup.exe".into(),
            })
        };
        let info = |name: Option<&str>| {
            name.map(|n| AvailableInfo {
                version: "0.2.0".into(),
                html_url: "u".into(),
                notes: None,
                asset_name: Some(n.into()),
                asset_size: None,
                downloadable: true,
                asset_url: Some("dl".into()),
            })
        };
        assert!(
            carry_downloaded(d(), info(Some("setup.exe")).as_ref()).is_some(),
            "同资产保留"
        );
        assert!(
            carry_downloaded(d(), info(Some("other.exe")).as_ref()).is_none(),
            "换版本资产清空"
        );
        assert!(carry_downloaded(d(), None).is_none(), "无新版本清空");
        assert!(
            carry_downloaded(None, info(Some("setup.exe")).as_ref()).is_none(),
            "本来就没有记录"
        );
        assert!(
            carry_downloaded(d(), info(None).as_ref()).is_none(),
            "新版本无资产名（downloadable=false）同样清空"
        );
    }

    /// 契约：资产名仅放行纯文件名 .exe/.zip（zip = 便携更新资产）——
    /// 路径分隔符/盘符冒号/ADS 形态/其他扩展名一律拒绝（写入侧防御，
    /// 运行安装侧见 exe-only 路径校验）。
    #[test]
    fn validate_asset_name_contract() {
        assert!(validate_asset_name("QuotaTray_0.4.1_x64-setup.exe"));
        assert!(validate_asset_name("setup.EXE"), "扩展名大小写不敏感");
        assert!(
            validate_asset_name("QuotaTray_0.7.0_x64-portable.zip"),
            "zip = 便携更新资产放行"
        );
        assert!(!validate_asset_name(""));
        assert!(!validate_asset_name("..\\..\\evil.exe"), "反斜杠上跳");
        assert!(!validate_asset_name("a/b.exe"), "POSIX 分隔符");
        assert!(!validate_asset_name("C:x.exe"), "盘符冒号");
        assert!(!validate_asset_name("setup.exe:ads"), "NTFS ADS 冒号");
        assert!(!validate_asset_name("setup.rar"), "非 exe/zip 拒绝");
        assert!(!validate_asset_name(".."), "目录上跳");
    }

    /// 契约：恶意/非法资产名在落盘前被拒——不产生文件也不写状态。
    #[test]
    fn save_installer_rejects_bad_asset_name() {
        let dir = std::env::temp_dir().join(format!("qt-savebad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = sandbox_state(&dir);
        for bad in ["..\\..\\evil.exe", "a/b.exe", "evil.rar"] {
            assert!(
                save_installer(&state, bad, b"x", Lang::Zh).is_err(),
                "{bad} 应被拒绝"
            );
            assert!(
                !installer_dir().join(bad).exists(),
                "{bad} 不得落盘（越界路径本就不应存在）"
            );
        }
        assert!(
            state.update_ctl.read().unwrap().downloaded.is_none(),
            "拒绝时不写状态表"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：安装包字节落盘到下载目录（原子写语义由 core 覆盖）并记录
    /// 路径 + 资产名进状态表。
    #[test]
    fn save_installer_writes_and_records() {
        let dir = std::env::temp_dir().join(format!("qt-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = sandbox_state(&dir);
        let name = format!("qt-test-{}.exe", std::process::id());
        let path = save_installer(&state, &name, b"hello", Lang::Zh).unwrap();
        let expected = installer_dir().join(&name);
        assert_eq!(PathBuf::from(&path), expected, "落盘到下载目录");
        assert_eq!(std::fs::read(&expected).unwrap(), b"hello");
        let d = state.update_ctl.read().unwrap().downloaded.clone().unwrap();
        assert_eq!(d.path, path);
        assert_eq!(d.asset_name, name);
        std::fs::remove_file(&expected).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：dto_of 透传已下载路径。
    #[test]
    fn dto_of_exposes_downloaded_path() {
        let mut inner = UpdateCtlState::default();
        assert!(dto_of(&inner, false).downloaded_path.is_none());
        inner.downloaded = Some(DownloadedInstaller {
            path: "p".into(),
            asset_name: "setup.exe".into(),
        });
        assert_eq!(dto_of(&inner, false).downloaded_path.as_deref(), Some("p"));
    }

    /// 契约：DTO 携带编译期架构标签（与 core arch_label 一致）并透传便携形态。
    #[test]
    fn dto_of_carries_platform_and_portable() {
        let inner = UpdateCtlState::default();
        let dto = dto_of(&inner, false);
        assert_eq!(dto.platform, update::arch_label());
        assert_eq!(dto.current_version, VERSION);
        assert!(!dto.portable, "安装形态透传 false");
        assert!(dto_of(&inner, true).portable, "便携形态透传 true");
        assert_eq!(
            dto.manual_update,
            update::AssetSelector::installed().requires_manual_update()
        );
        assert!(dto_of(&inner, true).manual_update, "便携 zip 必须手动覆盖");
    }

    /// 契约：安装包文件丢失或路径越界时，run_installer 清记录并报错
    /// （不进入 spawn 分支——文件不存在时提前返回）。
    #[test]
    fn run_installer_clears_record_on_missing_or_invalid() {
        let dir = std::env::temp_dir().join(format!("qt-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = sandbox_state(&dir);

        // 记录存在但文件已被清理（临时目录场景）
        state.update_ctl.write().unwrap().downloaded = Some(DownloadedInstaller {
            path: installer_dir()
                .join("qt-missing.exe")
                .to_string_lossy()
                .into_owned(),
            asset_name: "qt-missing.exe".into(),
        });
        assert!(run_installer(&state, Lang::Zh).is_err(), "文件丢失应报错");
        assert!(
            state.update_ctl.read().unwrap().downloaded.is_none(),
            "记录被清，前端回到可重下状态"
        );

        // 路径越界（不在下载目录内）：同样清记录并报错
        state.update_ctl.write().unwrap().downloaded = Some(DownloadedInstaller {
            path: "C:\\Windows\\notepad.exe".into(),
            asset_name: "notepad.exe".into(),
        });
        assert!(run_installer(&state, Lang::Zh).is_err(), "越界路径应报错");
        assert!(state.update_ctl.read().unwrap().downloaded.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
