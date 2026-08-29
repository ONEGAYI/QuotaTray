//! 更新检测控制器：状态表、检测执行、安装包下载/运行与每分钟调度。
//!
//! 业务逻辑（版本比较/release 解析/节流判定）在 core::update；本模块只做
//! GUI 侧编排——状态表维护、settings 节流时间戳落盘、下载到系统临时
//! 目录（`%TEMP%/QuotaTray/Downloads`）、静默运行安装包（NSIS
//! `/S /UPDATE /R`：无向导 UI、跳过重装交互、装完自动重启应用，应用
//! 随即退出解锁自身文件）、检测后联动（自动下载 + 就绪广播 + 系统通知）、
//! 启动惰性清理、常驻调度（每分钟 wake 读设置判定，设置变更自然生效
//! 免任务重启）。

use std::path::{Path, PathBuf};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::time::Duration;

use quota_core::http::HttpClient;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use quota_core::http::ReqwestHttpClient;

use quota_core::update::{
    self, AssetDownloader, DownloadProgress, DownloadProgressReporter, ReqwestAssetDownloader,
    UpdateStatus, VERSION, is_stale_installer,
};
use serde::Serialize;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri::Manager;
use tauri::{AppHandle, Emitter};

use crate::i18n::Lang;
use crate::state::{AppState, now_ms};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
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
    /// 「更新就绪」已广播过的资产名（None = 未广播）。会话内防重复：
    /// 自动下载完成与探测恢复各广播一次，换版本资产时随之失效。
    /// 纯后端联动状态，不进 DTO。
    pub ready_notified: Option<String>,
    /// 「发现新版本」已广播过的版本号（None = 未广播；移动端专用，
    /// 桌面不消费恒 None）。会话内防重复：进更新页反复手动检测不
    /// 重复打扰，换新版本时随 [`run_check`] 的登记迁移自然失效。
    /// 纯后端联动状态，不进 DTO。
    pub available_notified: Option<String>,
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
/// 消息中心系统通知渠道 id（Android，`setup_surfaces` 创建、发射点引用）。
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const MESSAGES_CHANNEL_ID: &str = "quotatray-messages";
/// 自动调度完成检测后推送完整状态，已打开的设置页可立即刷新。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub const UPDATE_STATE_EVENT: &str = "update-state-changed";
/// 「更新就绪」事件：安装包落盘（自动下载完成或重启后探测恢复）且本
/// 会话未广播过时推送，前端消息中心红点与消息卡片据此生成。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub const UPDATE_READY_EVENT: &str = "update-ready";

/// [`UPDATE_READY_EVENT`] 的负载：新版本号（消息卡片展示与安装确认用）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub struct UpdateReadyEvent {
    pub version: String,
}

/// 「发现新版本」事件（移动端）：手动检测（进更新页/按钮）发现可用
/// 版本且本会话未广播过时推送，前端消息中心红点与卡片据此生成。
/// 与桌面 [`UPDATE_READY_EVENT`]（安装包已下载完成、卡片直连静默安装）
/// 语义不同：移动端无自动下载，卡片动作是引导到设置·更新页手动下载。
#[cfg(any(target_os = "android", target_os = "ios"))]
pub const UPDATE_AVAILABLE_EVENT: &str = "update-available";

/// [`UPDATE_AVAILABLE_EVENT`] 的负载：可用版本号（与桌面
/// [`UpdateReadyEvent`] 同形，前端入列共用同一 merge 通道）。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub struct UpdateAvailableEvent {
    pub version: String,
}

/// 可用广播判定（纯函数，移动端消费）：返回 Some(version) 表示应广播。
/// 会话内同版本只广播一次（进更新页反复手动检测不重复打扰）；无新
/// 版本不广播。与桌面 [`should_notify_ready`] 的防重口径一致。
/// 全平台编译以便 host 单测（桌面无调用方，见 cfg_attr）。
#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
pub fn should_notify_available(inner: &UpdateCtlState) -> Option<String> {
    let info = inner.info.as_ref()?;
    if inner.available_notified.as_deref() == Some(info.version.as_str()) {
        return None;
    }
    Some(info.version.clone())
}

/// 「发现新版本」单次广播（移动端）：判定通过后先登记再推送事件
/// （先置位防并发重复，口径同桌面 [`notify_ready_once`]）；应用在后台
/// 且通知开关开启时补发系统通知（前台只入列红点，不打扰）。
/// iOS 无通知链（无渠道/权限桥），本函数与通知文案均为 android-only。
#[cfg(target_os = "android")]
pub fn notify_available_once(app: &AppHandle, state: &AppState) {
    let version = {
        let inner = state.update_ctl.read().unwrap();
        let Some(version) = should_notify_available(&inner) else {
            return;
        };
        version
    };
    state.update_ctl.write().unwrap().available_notified = Some(version.clone());
    let _ = app.emit(
        UPDATE_AVAILABLE_EVENT,
        UpdateAvailableEvent {
            version: version.clone(),
        },
    );
    let lang = Lang::parse(&state.settings.read().unwrap().language);
    notify_background(
        app,
        state,
        &lang.update_available_notify_title(),
        &lang.update_available_notify_body(&version),
    );
}

/// 后台补发系统通知（Android，消息中心二阶）：仅当应用在后台（由前端
/// visibilitychange 经 `set_app_foreground` 同步）且 `notifications_enabled`
/// 开启时发送；权限未授予时由系统静默丢弃（不显式判定）。发送失败仅
/// 日志不阻断——前端消息中心红点仍在。
#[cfg(target_os = "android")]
pub(crate) fn notify_background(app: &AppHandle, state: &AppState, title: &str, body: &str) {
    if !state.settings.read().unwrap().notifications_enabled {
        return;
    }
    if state
        .app_foreground
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app
        .notification()
        .builder()
        .channel_id(MESSAGES_CHANNEL_ID)
        .title(title)
        .body(body)
        .show()
    {
        eprintln!("系统通知发送失败：{e}");
    }
}

/// 主窗不可见时补发系统通知（桌面收口，与 Android [`notify_background`]
/// 对称）：仅当主窗不可见（托盘常驻常态——桌面语义下对应移动端
/// 「应用在后台」）且 `notifications_enabled` 开启时发送。发送失败
/// （Windows 通知权限/AUMID 异常场景）仅日志不阻断——红点仍在。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn notify_desktop(app: &AppHandle, state: &AppState, title: &str, body: &str) {
    if !state.settings.read().unwrap().notifications_enabled {
        return;
    }
    let main_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if main_visible {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        eprintln!("系统通知发送失败：{e}");
    }
}

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

/// 下载进度 → 前端事件的桥（桌面落盘下载与 Android SAF 写入共用）。
pub(crate) struct TauriProgressReporter<'a> {
    pub(crate) app: &'a AppHandle,
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
    let prev = state.update_ctl.read().unwrap().clone();
    let prev_downloaded = prev.downloaded.clone();
    let prev_ready_notified = prev.ready_notified.clone();
    let prev_available_notified = prev.available_notified.clone();
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
            ready_notified: None,
            available_notified: None,
        },
        // 无 release / 已最新：清掉旧的新版本信息（跨版本状态不残留）
        Ok(_) => UpdateCtlState {
            last_check: Some(now),
            info: None,
            last_error: None,
            last_error_detail: None,
            downloaded: None,
            ready_notified: None,
            available_notified: None,
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
            // 检测失败沿用旧广播状态（已提醒过的不重复提醒）
            ready_notified: prev_ready_notified.clone(),
            available_notified: prev_available_notified.clone(),
        },
    };
    if inner.last_error.is_none() {
        inner.downloaded = carry_downloaded(prev_downloaded, inner.info.as_ref());
        // 就绪广播状态跟随已下载记录：同资产保留（不重复广播），
        // 记录失效（换版本/已最新）则一并清空
        inner.ready_notified = match (&inner.downloaded, &prev_ready_notified) {
            (Some(d), Some(n)) if n == &d.asset_name => Some(n.clone()),
            _ => None,
        };
        // 「发现新版本」广播登记跟随 available：同版本保留（进更新页
        // 反复检测不重复打扰），换版本/已最新则失效
        inner.available_notified = inner
            .info
            .as_ref()
            .and_then(|i| prev_available_notified.filter(|n| n == &i.version));
        // 探测恢复：已下载记录为空但下载目录存在同名资产文件（应用重启
        // 后内存态丢失、安装失败/稍后安装后重开的场景）——磁盘文件即
        // 状态，恢复记录免重新下载；就绪广播由 post_check 统一判定
        if inner.downloaded.is_none()
            && let Some(name) = inner
                .info
                .as_ref()
                .and_then(|i| i.asset_name.as_deref())
                .filter(|n| validate_asset_name(n))
                .filter(|n| installer_dir().join(n).is_file())
        {
            inner.downloaded = Some(DownloadedInstaller {
                path: installer_dir().join(name).to_string_lossy().into_owned(),
                asset_name: name.to_string(),
            });
        }
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
/// 冒号，杜绝 `..\` 上跳与 NTFS ADS 形态）且以 `.exe`/`.zip`/`.apk`
/// 结尾（zip = 便携/WoA 形态更新资产，apk = Android 更新资产）——防
/// 恶意资产名使落盘位置逃出下载目录（运行安装侧另有 exe-only 的
/// validate_installer_path）。
pub(crate) fn validate_asset_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.is_empty()
        && !name.contains(['/', '\\', ':'])
        && (lower.ends_with(".exe") || lower.ends_with(".zip") || lower.ends_with(".apk"))
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

/// 静默运行已下载的安装包（NSIS `/S /UPDATE /R`，行为实证于 Tauri 2
/// 官方 NSIS 模板）：
///
/// - `/S`：全静默（无向导 UI）；安装器遇到运行中的本应用会直接结束
///   当前用户进程（`CheckIfAppIsRunning` 宏的 `IfSilent` 分支）。
/// - `/UPDATE`：升级语义——重装确认页直接放行（裸 `/S` 遇已安装版本
///   走 nsDialogs 静默未定义行为）、保留快捷方式与自启项、跳过 WebView2
///   重装。必须携带。
/// - `/R`：静默模式下安装成功后自动重启应用（用户态 `RunAsUser`）。
///
/// 覆盖安装需先解锁自身文件，调用方（install_update 命令）在启动成功后
/// 退出应用——应用先走 Tauri 正常退出流程（托盘图标移除、状态收尾）
/// 优于被安装器强杀，安装器的 kill 仅是竞态兜底。
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
        .args(["/S", "/UPDATE", "/R"])
        .spawn()
        .map_err(|e| lang.err_update_run(&e))?;
    Ok(())
}

/// 启动惰性清理：删除下载目录中「版本不严格新于当前运行版本」的资产
/// 文件（core `is_stale_installer` 契约：只清本命名空间，契约外文件不动）。
///
/// 该策略同时满足 issue #58 的保留诉求——安装成功（新版启动清旧包）、
/// 安装失败与稍后安装（旧版运行中，新包版本更高 → 保留）。单项删除
/// 失败仅告警不阻断启动（Temp 语义本就允许系统兜底回收）。
pub fn cleanup_stale_installers() {
    let dir = installer_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return; // 目录不存在（从未下载过）是常态
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_stale_installer(&name, VERSION)
            && let Err(e) = std::fs::remove_file(entry.path())
        {
            eprintln!("旧安装包清理失败（{name}）：{e}");
        }
    }
}

/// 自动下载判定（纯函数，便于契约测试）：开关开启、安装版（zip 形态
/// 维持「打开目录手动覆盖」引导）、尚未下载、当前版本可下载。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn should_auto_download(
    auto_enabled: bool,
    manual_update: bool,
    info: Option<&AvailableInfo>,
    downloaded: bool,
) -> bool {
    auto_enabled && !manual_update && !downloaded && info.is_some_and(|i| i.downloadable)
}

/// 就绪广播判定（纯函数）：返回 Some(version) 表示应广播。
///
/// 门禁与 [`should_auto_download`] 同口径——zip 手动覆盖形态（便携 /
/// ARM64 Preview）不广播：其「现在安装」必然被 install_update 的形态
/// 拒绝，消息卡片只会给出永远无效的重试引导，正确入口是设置页的
/// 「打开下载目录」手动覆盖流程。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn should_notify_ready(manual_update: bool, inner: &UpdateCtlState) -> Option<String> {
    if manual_update {
        return None;
    }
    let d = inner.downloaded.as_ref()?;
    if inner.ready_notified.as_deref() == Some(d.asset_name.as_str()) {
        return None;
    }
    // 版本号取自当前 available（与 downloaded 同源——carry/探测恢复均
    // 以资产名一致为前提）；取不到时不广播（保守不误发）
    inner
        .info
        .as_ref()
        .filter(|i| i.asset_name.as_deref() == Some(d.asset_name.as_str()))
        .map(|i| i.version.clone())
}

/// 「更新就绪」单次广播：[`should_notify_ready`] 判定通过时，先登记
/// 广播状态（防与手动检测并发的双重系统通知）再推送事件；主窗不可见
/// 时补发系统通知。覆盖自动下载完成与重启后探测恢复两个来源。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn notify_ready_once(app: &AppHandle, state: &AppState) {
    let manual_update =
        update::AssetSelector::for_runtime(update::arch_label(), state.mode.is_portable())
            .requires_manual_update();
    let (version, asset_name) = {
        let inner = state.update_ctl.read().unwrap();
        let Some(version) = should_notify_ready(manual_update, &inner) else {
            return;
        };
        (
            version,
            inner.downloaded.as_ref().unwrap().asset_name.clone(),
        )
    };
    // 先置位后广播：与手动检测命令并发时，后到者读到已置位的
    // ready_notified 直接短路——前端 mergeMessage 本就去重，此处防的是
    // 用户可见的重复系统通知
    state.update_ctl.write().unwrap().ready_notified = Some(asset_name);
    let _ = app.emit(
        UPDATE_READY_EVENT,
        UpdateReadyEvent {
            version: version.clone(),
        },
    );
    // 主窗不可见（托盘常驻常态）且通知开关开启时补系统通知；正文自带
    // 「打开主窗」引导（通知点击唤主窗的平台支持不一，不依赖之）。
    // 开关消费与低余额提醒共口（notify_desktop），桌面关开关后不再打扰。
    let lang = Lang::parse(&state.settings.read().unwrap().language);
    notify_desktop(
        app,
        state,
        &lang.update_ready_title(),
        &lang.update_ready_body(&version),
    );
}

/// 检测后的统一联动（调度器与手动检测命令在 [`run_check`] 之后调用）：
///
/// 1. 探测恢复的就绪广播（同步，微秒级）；
/// 2. 自动下载（后台任务，不阻塞检测返回——下载动辄数秒到数分钟），
///    完成后再广播就绪。
///
/// 手动下载（设置页按钮）不走本函数：用户正看进度条，按钮态即反馈，
/// 不重复打扰。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn post_check(app: &AppHandle, state: &AppState) {
    notify_ready_once(app, state);
    let manual_update =
        update::AssetSelector::for_runtime(update::arch_label(), state.mode.is_portable())
            .requires_manual_update();
    let (auto_enabled, info, downloaded) = {
        let s = state.settings.read().unwrap();
        let inner = state.update_ctl.read().unwrap();
        (
            s.update_auto_download,
            inner.info.clone(),
            inner.downloaded.is_some(),
        )
    };
    if !should_auto_download(auto_enabled, manual_update, info.as_ref(), downloaded) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let lang = Lang::parse(&state.settings.read().unwrap().language);
        if let Err(e) = download_installer(&app, &state, lang).await {
            // 自动下载失败静默（与自动检测同口径）：下个检测周期重试，
            // 手动入口（设置页）不受影响
            eprintln!("自动下载更新包失败：{e}");
            return;
        }
        notify_ready_once(&app, &state);
    });
}

/// 常驻调度：每分钟 wake 一次读设置，距上次检测 ≥ 轮询间隔
/// （`POLL_INTERVAL_MS`，5 分钟）即检测并重建托盘——应用运行期间持续
/// 轮询新版本（wake 粒度 1 分钟，实际间隔 5~6 分钟）。手动「立即检查」
/// 与失败检测同样落盘节流时间戳，设置变更在下个 wake 自然生效——无需
/// 重启任务。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn spawn_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            {
                let state = app.state::<AppState>();
                let (enabled, last) = {
                    let s = state.settings.read().unwrap();
                    (s.update_check_enabled, s.update_last_check)
                };
                if quota_core::update::should_check_within(
                    enabled,
                    last,
                    now_ms(),
                    quota_core::update::POLL_INTERVAL_MS,
                ) && let Ok(http) = ReqwestHttpClient::new_with_proxy(
                    Duration::from_secs(10),
                    proxy_url(&state).as_deref(),
                ) {
                    let inner = run_check(&state, &http).await;
                    let _ = app.emit(UPDATE_STATE_EVENT, dto_of(&inner, state.mode.is_portable()));
                    tray::rebuild(&app, &state);
                    // 检测后联动：探测恢复广播 + 自动下载（内部自 spawn）
                    post_check(&app, &state);
                }
                // 峰谷标签过期兜底：任一启用条目峰/谷翻转才重建并广播（内部自比对）
                tray::rebuild_on_peak_flip(&app, &state);
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
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
            app_foreground: std::sync::atomic::AtomicBool::new(true),
            low_balance_notified: std::sync::Mutex::new(std::collections::HashSet::new()),
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
                ready_notified: None,
                available_notified: None,
            },
            _ => UpdateCtlState {
                last_check: Some(1),
                info: None,
                last_error: None,
                last_error_detail: None,
                downloaded: None,
                ready_notified: None,
                available_notified: None,
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

    /// 契约：移动端「发现新版本」广播判定——无新版本不广播；有新版本
    /// 且本会话未登记过才广播；同版本已登记（进更新页反复检测）短路；
    /// 换新版本重新广播（登记随 run_check 迁移失效）。
    #[test]
    fn should_notify_available_contract() {
        let info = |version: &str| {
            Some(AvailableInfo {
                version: version.into(),
                html_url: "u".into(),
                notes: None,
                asset_name: None,
                asset_size: None,
                downloadable: false,
                asset_url: None,
            })
        };
        // 无新版本（已最新/未检测）不广播
        assert_eq!(
            should_notify_available(&UpdateCtlState {
                info: None,
                ..Default::default()
            }),
            None
        );
        // 有新版本且未登记 → 广播该版本
        assert_eq!(
            should_notify_available(&UpdateCtlState {
                info: info("0.9.0"),
                ..Default::default()
            }),
            Some("0.9.0".into())
        );
        // 同版本已登记 → 短路（不重复打扰）
        assert_eq!(
            should_notify_available(&UpdateCtlState {
                info: info("0.9.0"),
                available_notified: Some("0.9.0".into()),
                ..Default::default()
            }),
            None
        );
        // 登记的是旧版本、当前 available 是更新的版本 → 重新广播
        assert_eq!(
            should_notify_available(&UpdateCtlState {
                info: info("0.10.0"),
                available_notified: Some("0.9.0".into()),
                ..Default::default()
            }),
            Some("0.10.0".into())
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
        assert!(
            validate_asset_name("QuotaTray_0.8.1_android-arm64.apk"),
            "apk = Android 更新资产放行"
        );
        assert!(!validate_asset_name("a/b.apk"), "apk 路径分隔符同样拒绝");
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

    /// 契约：探测恢复——下载目录存在与 available 同名的资产文件时，
    /// run_check 恢复 downloaded 记录（磁盘文件即状态，免重新下载），
    /// ready_notified 保持未广播（重启后由 post_check 广播一次）。
    /// 已最新（无 available）时即使磁盘还有文件也不恢复。
    #[tokio::test]
    async fn run_check_recovers_downloaded_from_disk() {
        let dir = std::env::temp_dir().join(format!("qt-recover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = sandbox_state(&dir);
        std::fs::create_dir_all(installer_dir()).unwrap();
        let name = update::expected_asset_name("9.9.9", "x64", update::Flavor::SetupExe);
        let file = installer_dir().join(&name);
        std::fs::write(&file, b"pkg").unwrap();
        let cleanup = || {
            std::fs::remove_file(&file).ok();
            std::fs::remove_dir_all(&dir).ok();
        };

        let body = format!(
            r#"{{"tag_name":"v9.9.9","html_url":"u","assets":[
                {{"name":"{name}","browser_download_url":"https://x/s","size":3}}
            ]}}"#
        );
        let http = RouteHttp {
            routes: vec![("releases/latest", 200, body)],
        };
        let inner = run_check(&state, &http).await;
        let d = inner.downloaded.expect("磁盘同名资产文件应恢复已下载记录");
        assert_eq!(d.asset_name, name);
        assert_eq!(PathBuf::from(&d.path), file, "恢复路径即下载目录内同名文件");
        assert_eq!(inner.ready_notified, None, "恢复不自动置广播位");
        // 状态表同步（run_check 结尾整体写回）
        assert!(
            state
                .update_ctl
                .read()
                .unwrap()
                .downloaded
                .as_ref()
                .is_some_and(|d| d.asset_name == name),
            "恢复结果写入状态表"
        );

        // 已最新（404）：即使磁盘还有文件也不恢复（无 available 即无安装入口）
        let http = RouteHttp {
            routes: vec![("releases/latest", 404, "".into())],
        };
        let inner = run_check(&state, &http).await;
        assert_eq!(inner.downloaded, None, "无新版本不恢复");
        cleanup();
    }

    /// 契约：惰性清理——下载目录里「版本不严格新于当前」的契约名文件
    /// 被删（安装成功后的回收路径），新版本（稍后安装/失败保留）与
    /// 契约外文件不动。
    #[test]
    fn cleanup_stale_installers_removes_only_stale_contract_files() {
        std::fs::create_dir_all(installer_dir()).unwrap();
        let stale = update::expected_asset_name("0.0.1", "x64", update::Flavor::SetupExe);
        let current = update::expected_asset_name(VERSION, "x64", update::Flavor::SetupExe);
        let fresh = update::expected_asset_name("99.0.0", "x64", update::Flavor::SetupExe);
        let alien = format!("qt-alien-{}.txt", std::process::id());
        for name in [&stale, &current, &fresh, &alien] {
            std::fs::write(installer_dir().join(name), b"x").unwrap();
        }
        cleanup_stale_installers();
        assert!(!installer_dir().join(&stale).exists(), "旧版本 → 删");
        assert!(
            !installer_dir().join(&current).exists(),
            "与当前同版本（已装过的残留）→ 删"
        );
        assert!(installer_dir().join(&fresh).exists(), "新版本 → 留");
        assert!(installer_dir().join(&alien).exists(), "契约外文件 → 不动");
        // 测试自清（fresh/alien）
        std::fs::remove_file(installer_dir().join(&fresh)).ok();
        std::fs::remove_file(installer_dir().join(&alien)).ok();
    }

    /// 契约：自动下载判定矩阵——开关×形态×已下载×可下载。
    #[test]
    fn should_auto_download_matrix() {
        let info = |downloadable: bool| AvailableInfo {
            version: "9.9.9".into(),
            html_url: "u".into(),
            notes: None,
            asset_name: Some("QuotaTray_9.9.9_x64-setup.exe".into()),
            asset_size: None,
            downloadable,
            asset_url: Some("dl".into()),
        };
        // 满足条件的正路径
        assert!(should_auto_download(true, false, Some(&info(true)), false));
        // 任一条件不满足即否
        assert!(
            !should_auto_download(false, false, Some(&info(true)), false),
            "开关关"
        );
        assert!(
            !should_auto_download(true, true, Some(&info(true)), false),
            "zip 手动覆盖形态不自动下载"
        );
        assert!(
            !should_auto_download(true, false, Some(&info(true)), true),
            "已下载不重复"
        );
        assert!(
            !should_auto_download(true, false, Some(&info(false)), false),
            "无资产（downloadable=false）不下载"
        );
        assert!(!should_auto_download(true, false, None, false), "无新版本");
        assert!(
            !should_auto_download(true, false, None, true),
            "无新版本且已下载同样不触发"
        );
    }

    /// 契约：就绪广播判定——zip 手动覆盖形态短路（防便携形态出现必然
    /// 失败的「现在安装」入口）、未下载 / 已广播 / 资产不匹配不广播。
    #[test]
    fn should_notify_ready_matrix() {
        let name = update::expected_asset_name("9.9.9", "x64", update::Flavor::SetupExe);
        let mk = |downloaded: bool, notified: bool, asset_match: bool| UpdateCtlState {
            downloaded: downloaded.then(|| DownloadedInstaller {
                path: installer_dir().join(&name).to_string_lossy().into_owned(),
                asset_name: name.clone(),
            }),
            ready_notified: notified.then(|| name.clone()),
            info: Some(AvailableInfo {
                version: "9.9.9".into(),
                html_url: "u".into(),
                notes: None,
                // 资产不匹配场景用别的名字（模拟记录与 available 脱钩）
                asset_name: Some(if asset_match {
                    name.clone()
                } else {
                    "other.exe".into()
                }),
                asset_size: None,
                downloadable: true,
                asset_url: Some("dl".into()),
            }),
            ..Default::default()
        };
        // 正路径：安装形态 + 已下载 + 未广播 + 资产一致 → 返回版本号
        assert_eq!(
            should_notify_ready(false, &mk(true, false, true)),
            Some("9.9.9".into())
        );
        // zip 手动覆盖形态（便携 / ARM64 Preview）一律不广播
        assert_eq!(
            should_notify_ready(true, &mk(true, false, true)),
            None,
            "便携形态死按钮"
        );
        // 其余门禁
        assert_eq!(
            should_notify_ready(false, &mk(false, false, true)),
            None,
            "未下载"
        );
        assert_eq!(
            should_notify_ready(false, &mk(true, true, true)),
            None,
            "本会话已广播"
        );
        assert_eq!(
            should_notify_ready(false, &mk(true, false, false)),
            None,
            "available 与已下载记录资产不一致（脱钩态不广播）"
        );
        // 无 available（检测失败沿用旧记录的中间态）
        let mut inner = mk(true, false, true);
        inner.info = None;
        assert_eq!(
            should_notify_ready(false, &inner),
            None,
            "无 available 不广播"
        );
    }

    /// 契约：ready_notified 跟随已下载记录——同资产保留（不重复广播）、
    /// 记录失效清空、检测失败沿用旧值。
    #[tokio::test]
    async fn ready_notified_carries_with_downloaded() {
        let dir = std::env::temp_dir().join(format!("qt-readynf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = sandbox_state(&dir);
        let name = update::expected_asset_name("9.9.9", "x64", update::Flavor::SetupExe);
        let body = format!(
            r#"{{"tag_name":"v9.9.9","html_url":"u","assets":[
                {{"name":"{name}","browser_download_url":"https://x/s","size":3}}
            ]}}"#
        );

        // 已下载且已广播 → 成功检测同资产：两者都保留
        state.update_ctl.write().unwrap().downloaded = Some(DownloadedInstaller {
            path: installer_dir().join(&name).to_string_lossy().into_owned(),
            asset_name: name.clone(),
        });
        state.update_ctl.write().unwrap().ready_notified = Some(name.clone());
        let http = RouteHttp {
            routes: vec![("releases/latest", 200, body)],
        };
        let inner = run_check(&state, &http).await;
        assert_eq!(
            inner.ready_notified.as_deref(),
            Some(name.as_str()),
            "同资产保留广播位"
        );

        // 已最新（404）：记录与广播位一并清空
        let http = RouteHttp {
            routes: vec![("releases/latest", 404, "".into())],
        };
        let inner = run_check(&state, &http).await;
        assert_eq!(inner.ready_notified, None, "记录失效清广播位");

        // 检测失败：沿用旧广播状态
        state.update_ctl.write().unwrap().ready_notified = Some("setup.exe".into());
        let http = RouteHttp { routes: vec![] };
        let inner = run_check(&state, &http).await;
        assert_eq!(
            inner.ready_notified.as_deref(),
            Some("setup.exe"),
            "检测失败沿用旧广播位"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：available_notified 跟随 available——同版本保留（进更新页
    /// 反复检测不重复打扰）、换版本清空（新版本重新广播）、检测失败
    /// 沿用旧值（与 ready_notified 迁移同款三段式）。
    #[tokio::test]
    async fn available_notified_carries_with_info() {
        let dir = std::env::temp_dir().join(format!("qt-availnf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let state = sandbox_state(&dir);
        let name = update::expected_asset_name("9.9.9", "x64", update::Flavor::SetupExe);
        let body = format!(
            r#"{{"tag_name":"v9.9.9","html_url":"u","assets":[
                {{"name":"{name}","browser_download_url":"https://x/s","size":3}}
            ]}}"#
        );

        // 换版本：登记的是旧版本，检测出新版本 → 清空（重新可广播）
        state.update_ctl.write().unwrap().available_notified = Some("0.1.0".into());
        let http = RouteHttp {
            routes: vec![("releases/latest", 200, body.clone())],
        };
        let inner = run_check(&state, &http).await;
        assert_eq!(inner.available_notified, None, "换版本清空旧登记");

        // 同版本：登记与 available 一致 → 保留（不重复打扰）
        state.update_ctl.write().unwrap().available_notified = Some("9.9.9".into());
        let http = RouteHttp {
            routes: vec![("releases/latest", 200, body)],
        };
        let inner = run_check(&state, &http).await;
        assert_eq!(
            inner.available_notified.as_deref(),
            Some("9.9.9"),
            "同版本保留登记"
        );

        // 检测失败：沿用旧登记
        let http = RouteHttp { routes: vec![] };
        let inner = run_check(&state, &http).await;
        assert_eq!(
            inner.available_notified.as_deref(),
            Some("9.9.9"),
            "检测失败沿用旧登记"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
