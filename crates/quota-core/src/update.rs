//! GitHub release 更新检测与安装包下载（M4-b）。
//!
//! 职责边界：本模块只承载纯业务逻辑——版本比较、release 响应解析、
//! 安装包资产挑选、检测节流判定、字节原子落盘；检测时机的调度
//! （CLI 启动钩子 / GUI 常驻定时）与 UI 提示留在端侧。
//!
//! 通道拆分：release 元数据是 JSON 文本，复用 [`HttpClient`]（自定义
//! header 与 302 跟随均支持）；安装包是二进制字节流，走独立的
//! [`AssetDownloader`]——HttpClient 的 body 是 String 且生产实现带 15s
//! 总超时，载不动安装包，也不为此扩展 M2 冻结的 trait API 面。
//!
//! 时区说明：每日定时判定需要"本地日期/时刻"，std 无本地时区 API，
//! 故引入 chrono（仅 clock 能力）；时间戳入参一律 epoch 毫秒，纯函数
//! 可离线测试。

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{Datelike, TimeZone, Timelike};
use serde::Deserialize;

use crate::http::{HttpClient, HttpError, HttpRequest};

/// 当前程序版本（workspace 单源继承，与 CLI `--version` / GUI app 版本一致）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 当前构建的目标架构标签，对齐发布资产命名（x64 / ARM64）。
/// GUI 更新页与 CLI `--version` 共用，保证两端展示一致。
pub fn arch_label() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "ARM64"
    } else {
        "unknown"
    }
}

/// release 所在仓库（owner/repo）。
pub const GITHUB_REPO: &str = "ONEGAYI/QuotaTray";

/// 周期检测的最小间隔（自动检测每 24h 至多一次，与每日到点判定互补）。
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// 下载大小上限（256MB）：NSIS 安装包为 MB 级，超限视为远端异常，防御性拒绝。
const MAX_DOWNLOAD_BYTES: usize = 256 * 1024 * 1024;

/// 建立连接的超时（与 10 分钟总超时独立）：直连不可达时快速失败，
/// 避免长时间零进度挂起后才报错。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

// ---- 类型 -----------------------------------------------------------------

/// 一次检测的结果。
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    /// 仓库还没有任何 release（GitHub API 404）。
    NoRelease,
    /// 已是最新版本（含 tag 不规范无法比较——宁可不提示，不误报更新）。
    UpToDate,
    /// 有新版本。`asset` 为 None 表示该 release 没有匹配的安装包资产，
    /// 端侧应引导去发布页（html_url）手动下载。
    Available {
        /// 去掉 v 前缀的版本号（如 "0.2.1"）。
        version: String,
        /// 发布页地址。
        html_url: String,
        /// release 说明（CHANGELOG 正文，可能为空）。
        notes: Option<String>,
        /// 选中的安装包资产。
        asset: Option<ReleaseAsset>,
    },
}

/// release 附带的可下载资产。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

/// GitHub `releases/latest` 响应中感兴趣的子集（多余字段忽略）。
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

/// 检测/下载错误。Http 网络类可按瞬时处理（自动检测静默、手动检测提示）；
/// Parse 为确定性（远端响应结构异常，重试无意义）。
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("{0}")]
    Http(#[from] HttpError),
    /// HTTP 状态异常（非 200/404，典型如 GitHub 对代理共享出口 IP 的
    /// 限流 403）：`status_text` 为主文案（"HTTP 403"），`detail` 为响应
    /// 体 message（限流原因等，None = 无可解析详情）。Display 只输出主
    /// 文案保持简短，完整信息走 [`UpdateError::full_message`]——端侧按
    /// 交互形态取舍（GUI 悬停展示详情，CLI 直接输出）。
    #[error("网络错误：{status_text}")]
    HttpStatus {
        status_text: String,
        detail: Option<String>,
    },
    #[error("release 信息解析失败：{0}")]
    Parse(String),
}

impl UpdateError {
    /// 是否瞬时（网络类，可重试/可静默）。限流 403 等状态异常与网络
    /// 失败同归瞬时：自动检测静默、手动检测提示后由用户择机重试
    /// （共享出口 IP 的配额会随窗口滚动恢复）。
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            UpdateError::Http(HttpError::Network(_) | HttpError::Timeout)
                | UpdateError::HttpStatus { .. }
        )
    }

    /// 完整错误文案：HttpStatus 有详情时以括号追加响应体 message，
    /// 其余变体同 Display。无悬停交互的端（CLI）应展示此文案。
    pub fn full_message(&self) -> String {
        match self {
            UpdateError::HttpStatus {
                status_text,
                detail: Some(detail),
            } => format!("网络错误：{status_text}（{detail}）"),
            _ => self.to_string(),
        }
    }
}

// ---- 版本比较（手写三段比较，不引 semver 依赖） ---------------------------

/// 解析 "vX.Y.Z" / "X.Y.Z" 为三段数字；忽略 `-rc.1` / `+build` 等前后缀；
/// 任何一段非数字或段数不是 3 → None。
pub fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let s = tag.trim().trim_start_matches(['v', 'V']);
    let s = s.split(['-', '+']).next()?.trim();
    let mut parts = s.split('.');
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// remote 是否严格新于 current；任一解析失败返回 None（不提示更新）。
pub fn is_newer(remote: &str, current: &str) -> Option<bool> {
    Some(parse_version(remote)? > parse_version(current)?)
}

// ---- 检测 -----------------------------------------------------------------

/// 查询 GitHub 最新 release 并与 `current` 比较。
///
/// 请求 GitHub API 需带 User-Agent（API 硬性要求）与 vnd Accept；
/// 404 = 无 release（`releases/latest` 不含 draft/prerelease）。
pub async fn check_update(
    http: &dyn HttpClient,
    current: &str,
) -> Result<UpdateStatus, UpdateError> {
    let req = HttpRequest::get(format!(
        "https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
    ))
    .header("User-Agent", &format!("QuotaTray/{VERSION}"))
    .header("Accept", "application/vnd.github+json");
    let resp = http.execute(req).await?;
    match resp.status {
        200 => {}
        404 => return Ok(UpdateStatus::NoRelease),
        // 限流/5xx 等归为网络类（端侧按瞬时处理）；主文案只透状态码，
        // 响应体 message（如限流 403 的 "API rate limit exceeded for
        // IP..."）作为 detail 结构化携带，端侧按交互形态决定是否展示。
        status => {
            return Err(UpdateError::HttpStatus {
                status_text: format!("HTTP {status}"),
                detail: extract_error_message(&resp.body),
            });
        }
    }
    let release: GithubRelease =
        serde_json::from_str(&resp.body).map_err(|e| UpdateError::Parse(e.to_string()))?;
    match is_newer(&release.tag_name, current) {
        Some(true) => Ok(UpdateStatus::Available {
            version: release
                .tag_name
                .trim()
                .trim_start_matches(['v', 'V'])
                .to_string(),
            html_url: release.html_url,
            notes: release.body.filter(|s| !s.trim().is_empty()),
            asset: pick_asset(&release.assets),
        }),
        // 解析失败（tag 不规范）也归入 UpToDate：不误报
        _ => Ok(UpdateStatus::UpToDate),
    }
}

/// 提取 GitHub 错误响应体的 `message` 字段（如限流 403 的
/// "API rate limit exceeded for IP..."）；非 JSON / 无 message / 空白 → None。
/// 按字符截断到 200 并加省略号，防异常响应塞超长文案刷屏。
fn extract_error_message(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        #[serde(default)]
        message: String,
    }
    let message = serde_json::from_str::<ErrorBody>(body).ok()?.message;
    let message = message.trim();
    if message.is_empty() {
        return None;
    }
    let truncated: String = message.chars().take(200).collect();
    Some(if truncated.len() < message.len() {
        format!("{truncated}…")
    } else {
        truncated
    })
}

/// 挑选安装包资产：名字含 "setup" 的 .exe 优先（NSIS 产物约定），
/// 否则首个 .exe；无 .exe → None。
fn pick_asset(assets: &[ReleaseAsset]) -> Option<ReleaseAsset> {
    let is_exe = |a: &ReleaseAsset| a.name.to_ascii_lowercase().ends_with(".exe");
    assets
        .iter()
        .filter(|a| is_exe(a))
        .find(|a| a.name.to_ascii_lowercase().contains("setup"))
        .or_else(|| assets.iter().find(|a| is_exe(a)))
        .cloned()
}

// ---- 下载 -----------------------------------------------------------------

/// 安装包下载进度。`total_bytes=None` 表示服务器未返回 Content-Length，
/// 此时调用方应展示不定总量进度；速率为从本次下载开始计算的平均字节/秒。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub bytes_per_second: u64,
}

/// 下载进度接收端。同步回调应快速返回；GUI 可在此转发事件，CLI 可刷新终端行。
pub trait DownloadProgressReporter: Send + Sync {
    fn report(&self, progress: DownloadProgress);
}

/// 安装包下载通道：独立于 HttpClient（String body 与 15s 超时载不动字节流）。
#[async_trait]
pub trait AssetDownloader: Send + Sync {
    async fn download(&self, url: &str) -> Result<Vec<u8>, HttpError>;

    /// 带进度的兼容入口。旧下载器只实现 [`Self::download`] 仍可用，完成时
    /// 至少收到一次终态；生产下载器覆写此方法以提供实时分块进度。
    async fn download_with_progress(
        &self,
        url: &str,
        reporter: &dyn DownloadProgressReporter,
    ) -> Result<Vec<u8>, HttpError> {
        let bytes = self.download(url).await?;
        reporter.report(DownloadProgress {
            downloaded_bytes: bytes.len() as u64,
            total_bytes: Some(bytes.len() as u64),
            bytes_per_second: 0,
        });
        Ok(bytes)
    }
}

/// 更新通道的代理 URL：设置层只存端口（本机 HTTP 代理，如 Clash），
/// 此处统一拼接为 `http://127.0.0.1:{port}`，CLI 与 GUI 共用同一口径。
pub fn proxy_url_of(port: Option<u16>) -> Option<String> {
    port.map(|p| format!("http://127.0.0.1:{p}"))
}

/// reqwest 实现的安装包下载器。
///
/// 要点：10 分钟总超时 + 15 秒连接超时（不可达快速失败）；默认 302 跟随
/// （browser_download_url 会跳转到 objects.githubusercontent.com）；
/// 256MB 上限（Content-Length 预检 + 实际字节数复检）。不依赖
/// [`crate::http::ReqwestHttpClient`] 的客户端——那是查询用的 15s 短超时配置。
pub struct ReqwestAssetDownloader {
    client: reqwest::Client,
}

impl Default for ReqwestAssetDownloader {
    fn default() -> Self {
        Self::new()
    }
}

impl ReqwestAssetDownloader {
    pub fn new() -> Self {
        // builder 仅设超时与代理，失败回退默认客户端（与历史行为一致）
        Self {
            client: build_client(None).unwrap_or_default(),
        }
    }

    /// 带可选代理构造。显式设置代理后 reqwest 不再叠加环境变量代理——
    /// 填了端口就用手动端口，未填走环境变量/直连默认。非法代理 URL
    /// 返回 Err（不静默回退直连，避免用户以为走了代理实际裸连）。
    pub fn try_with_proxy(proxy: Option<&str>) -> Result<Self, HttpError> {
        build_client(proxy).map(|client| Self { client })
    }

    async fn download_inner(
        &self,
        url: &str,
        reporter: Option<&dyn DownloadProgressReporter>,
    ) -> Result<Vec<u8>, HttpError> {
        let mut resp = self
            .client
            .get(url)
            .header("User-Agent", &format!("QuotaTray/{VERSION}"))
            .send()
            .await
            .map_err(map_reqwest_err)?;
        if !resp.status().is_success() {
            return Err(HttpError::Network(format!(
                "HTTP {}",
                resp.status().as_u16()
            )));
        }
        let total_bytes = resp.content_length();
        if let Some(len) = total_bytes {
            if len > MAX_DOWNLOAD_BYTES as u64 {
                return Err(HttpError::Network(format!(
                    "安装包过大（{len} 字节，上限 {MAX_DOWNLOAD_BYTES}）"
                )));
            }
        }

        let mut bytes = Vec::with_capacity(
            total_bytes
                .unwrap_or_default()
                .min(MAX_DOWNLOAD_BYTES as u64) as usize,
        );
        let started = Instant::now();
        let mut last_report = started;
        if let Some(reporter) = reporter {
            reporter.report(DownloadProgress {
                downloaded_bytes: 0,
                total_bytes,
                bytes_per_second: 0,
            });
        }

        while let Some(chunk) = resp.chunk().await.map_err(map_reqwest_err)? {
            if bytes.len().saturating_add(chunk.len()) > MAX_DOWNLOAD_BYTES {
                return Err(HttpError::Network("安装包超过大小上限".into()));
            }
            bytes.extend_from_slice(&chunk);
            if let Some(reporter) = reporter {
                if last_report.elapsed() >= Duration::from_millis(200) {
                    let elapsed = started.elapsed();
                    reporter.report(DownloadProgress {
                        downloaded_bytes: bytes.len() as u64,
                        total_bytes,
                        bytes_per_second: calculate_bytes_per_second(bytes.len() as u64, elapsed),
                    });
                    last_report = Instant::now();
                }
            }
        }

        if let Some(reporter) = reporter {
            reporter.report(DownloadProgress {
                downloaded_bytes: bytes.len() as u64,
                total_bytes,
                bytes_per_second: calculate_bytes_per_second(bytes.len() as u64, started.elapsed()),
            });
        }
        Ok(bytes)
    }
}

#[async_trait]
impl AssetDownloader for ReqwestAssetDownloader {
    async fn download(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        self.download_inner(url, None).await
    }

    async fn download_with_progress(
        &self,
        url: &str,
        reporter: &dyn DownloadProgressReporter,
    ) -> Result<Vec<u8>, HttpError> {
        self.download_inner(url, Some(reporter)).await
    }
}

/// 下载客户端构造：600s 总超时 + 15s 连接超时 + 可选代理。
fn build_client(proxy: Option<&str>) -> Result<reqwest::Client, HttpError> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(CONNECT_TIMEOUT);
    if let Some(url) = proxy {
        let proxy = reqwest::Proxy::all(url)
            .map_err(|e| HttpError::Network(format!("代理配置无效：{e}")))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|e| HttpError::Network(e.to_string()))
}

fn calculate_bytes_per_second(downloaded_bytes: u64, elapsed: Duration) -> u64 {
    let millis = elapsed.as_millis();
    if millis == 0 {
        return 0;
    }
    ((downloaded_bytes as u128 * 1_000) / millis).min(u64::MAX as u128) as u64
}

/// reqwest 错误映射（与 http::reqwest 同语义：timeout→Timeout，其余剥 URL）。
fn map_reqwest_err(e: reqwest::Error) -> HttpError {
    if e.is_timeout() {
        HttpError::Timeout
    } else {
        // 下载 URL 来自 GitHub release 资产，不含凭据，但仍统一剥 URL 防泄漏习惯
        HttpError::Network(e.without_url().to_string())
    }
}

// ---- 落盘 -----------------------------------------------------------------

/// 字节原子落盘：tmp（pid+进程内序号防并发互踩）+ rename，失败清理 tmp。
/// 模式与 config/settings/snapshot 的 JSON 原子写一致。
pub fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".into());
    let tmp = dir.join(format!(
        "{}.{}.{}.tmp",
        file_name,
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&tmp, bytes)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

// ---- 自动检测节流 / 每日定时（纯函数，epoch ms 入参） ---------------------

/// 周期检测判定（CLI 启动钩子与 GUI 首启共用）：开关开启且距上次检测
/// ≥24h（从未检测视为应检）。
pub fn should_check(enabled: bool, last_check_ms: Option<u64>, now_ms: u64) -> bool {
    enabled && last_check_ms.is_none_or(|t| now_ms.saturating_sub(t) >= DAY_MS)
}

/// "HH:MM" 解析（24 小时制含边界）；非法 → None。
pub fn parse_hhmm(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u8 = h.trim().parse().ok()?;
    let m: u8 = m.trim().parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

/// 每日到点判定（GUI 常驻调度用）：今天还没检测过（last_check 不落在
/// 今天本地日期）且本地时刻已过 `hhmm`。`hhmm` 非法视为永不到点
/// （设置层 sanitize 已兜底，此处防御）。
pub fn due_daily(last_check_ms: Option<u64>, hhmm: &str, now_ms: u64) -> bool {
    let Some((target_h, target_m)) = parse_hhmm(hhmm) else {
        return false;
    };
    let now = local_datetime(now_ms);
    let last_today = last_check_ms
        .is_some_and(|t| local_datetime(t).num_days_from_ce() == now.num_days_from_ce());
    !last_today && (now.hour(), now.minute()) >= (target_h as u32, target_m as u32)
}

/// 端侧调度统一判定：首启/距上次 ≥24h（`should_check`）或每天到点未检
/// （`due_daily`）——后者覆盖"每天一次"语义（即使距上次不足 24h，
/// 跨天到点也应检，如昨晚 23:50 检过、今晨 09:00 到点）。
pub fn due_check(enabled: bool, last_check_ms: Option<u64>, hhmm: &str, now_ms: u64) -> bool {
    should_check(enabled, last_check_ms, now_ms)
        || (enabled && due_daily(last_check_ms, hhmm, now_ms))
}

/// epoch 毫秒 → 本地时间（超出 chrono 有效范围按当前时间兜底，不 panic）。
fn local_datetime(epoch_ms: u64) -> chrono::DateTime<chrono::Local> {
    chrono::Local
        .timestamp_millis_opt(epoch_ms as i64)
        .single()
        .unwrap_or_else(chrono::Local::now)
}

// ---- 测试 -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingProgress(Mutex<Vec<DownloadProgress>>);

    impl DownloadProgressReporter for RecordingProgress {
        fn report(&self, progress: DownloadProgress) {
            self.0.lock().unwrap().push(progress);
        }
    }

    struct LegacyDownloader;

    #[async_trait]
    impl AssetDownloader for LegacyDownloader {
        async fn download(&self, _url: &str) -> Result<Vec<u8>, HttpError> {
            Ok(vec![1, 2, 3, 4])
        }
    }

    // ---- 版本比较 ----

    /// 契约：架构标签落在已发布资产命名集合内；未支持目标显式
    /// unknown 而非 panic（跨平台库约束下不应编译失败）。
    #[test]
    fn arch_label_in_known_set() {
        assert!(["x64", "ARM64", "unknown"].contains(&arch_label()));
    }

    #[test]
    fn parse_version_accepts_common_forms() {
        assert_eq!(parse_version("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("V2.0.0"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3+build.7"), Some((1, 2, 3)));
        assert_eq!(parse_version(" 1.2.3 "), Some((1, 2, 3)));
        // 非法形态
        assert_eq!(parse_version("1.2"), None, "段数不足");
        assert_eq!(parse_version("1.2.3.4"), None, "段数过多");
        assert_eq!(parse_version("a.b.c"), None, "非数字");
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("latest"), None);
    }

    #[test]
    fn is_newer_compares_and_tolerates_bad_tags() {
        assert_eq!(is_newer("v0.2.0", "0.1.9"), Some(true));
        assert_eq!(is_newer("0.1.0", "0.1.0"), Some(false), "相同版本不算新");
        assert_eq!(is_newer("v0.1.9", "0.2.0"), Some(false));
        assert_eq!(
            is_newer("nightly", "0.1.0"),
            None,
            "远端 tag 不规范 → 不提示"
        );
        assert_eq!(is_newer("0.2.0", "dev"), None, "本地版本不规范 → 不提示");
    }

    // ---- check_update（MockHttp 路由 + 请求头契约） ----

    const RELEASE_JSON: &str = r#"{
        "tag_name": "v0.2.0",
        "html_url": "https://github.com/ONEGAYI/QuotaTray/releases/v0.2.0",
        "body": "修复若干问题",
        "assets": [
            {"name": "QuotaTray_0.2.0_x64.zip", "browser_download_url": "https://x/zip", "size": 1},
            {"name": "QuotaTray_0.2.0_x64-setup.exe", "browser_download_url": "https://x/setup", "size": 2},
            {"name": "notes.txt", "browser_download_url": "https://x/txt", "size": 3}
        ]
    }"#;

    #[tokio::test]
    async fn check_update_finds_new_release_and_picks_setup_asset() {
        let http = MockHttp::ok(RELEASE_JSON);
        let status = check_update(&http, "0.1.0").await.unwrap();
        match status {
            UpdateStatus::Available {
                version,
                html_url,
                notes,
                asset,
            } => {
                assert_eq!(version, "0.2.0", "版本号去掉 v 前缀");
                assert_eq!(
                    html_url,
                    "https://github.com/ONEGAYI/QuotaTray/releases/v0.2.0"
                );
                assert_eq!(notes.as_deref(), Some("修复若干问题"));
                let asset = asset.expect("应选中 setup 安装包");
                assert_eq!(asset.name, "QuotaTray_0.2.0_x64-setup.exe");
                assert_eq!(asset.browser_download_url, "https://x/setup");
            }
            other => panic!("应为 Available：{other:?}"),
        }
    }

    #[tokio::test]
    async fn check_update_sends_required_headers() {
        let http = MockHttp::ok(RELEASE_JSON);
        check_update(&http, "0.1.0").await.unwrap();
        let reqs = http.captured_requests();
        assert_eq!(reqs.len(), 1);
        let req = &reqs[0];
        assert!(
            req.url
                .contains(&format!("repos/{GITHUB_REPO}/releases/latest")),
            "URL 应指向本仓库 latest：{}",
            req.url
        );
        let header = |name: &str| {
            req.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(
            header("User-Agent"),
            Some(format!("QuotaTray/{VERSION}").as_str()),
            "GitHub API 必需 UA"
        );
        assert_eq!(header("Accept"), Some("application/vnd.github+json"));
        assert!(
            !format!("{req:?}").contains("github_pat_"),
            "本请求不含凭据，Debug 应原样（回归占位）"
        );
    }

    #[tokio::test]
    async fn check_update_404_means_no_release() {
        let status = check_update(&MockHttp::status(404), "0.1.0").await.unwrap();
        assert_eq!(
            status,
            UpdateStatus::NoRelease,
            "当前仓库无 release 的实际状态"
        );
    }

    #[tokio::test]
    async fn check_update_same_or_bad_tag_is_up_to_date() {
        let http = MockHttp::ok(r#"{"tag_name":"v0.1.0","assets":[]}"#);
        assert_eq!(
            check_update(&http, "0.1.0").await.unwrap(),
            UpdateStatus::UpToDate
        );
        // tag 不规范：宁可不提示
        let http = MockHttp::ok(r#"{"tag_name":"latest-hotfix","assets":[]}"#);
        assert_eq!(
            check_update(&http, "0.1.0").await.unwrap(),
            UpdateStatus::UpToDate
        );
    }

    #[tokio::test]
    async fn check_update_network_error_propagates_as_transient() {
        let err = check_update(&MockHttp::fail(), "0.1.0").await.unwrap_err();
        assert!(err.is_transient(), "网络错误应归瞬时：{err}");
    }

    /// 契约：限流 403 等状态异常——主文案保持 "网络错误：HTTP {status}"
    /// （与历史行为一致），响应体 message 结构化为 detail；完整文案走
    /// full_message。场景来源：代理共享出口 IP 被 GitHub 未认证限流。
    #[tokio::test]
    async fn check_update_status_error_keeps_short_text_and_carries_detail() {
        let http = MockHttp::status_body(
            403,
            r#"{"message":"API rate limit exceeded for 103.190.179.2. (But here's the good news: Authenticated requests get a higher rate limit.)","documentation_url":"https://docs.github.com"}"#,
        );
        let err = check_update(&http, "0.1.0").await.unwrap_err();
        let UpdateError::HttpStatus {
            status_text,
            detail,
        } = &err
        else {
            panic!("非 200/404 应为 HttpStatus 变体：{err:?}");
        };
        assert_eq!(status_text, "HTTP 403");
        assert!(
            detail
                .as_deref()
                .unwrap_or_default()
                .contains("API rate limit exceeded"),
            "detail 应透出限流原因：{detail:?}"
        );
        // 主文案与历史 Display 完全一致——卡片主文字不因详情加入而变长
        assert_eq!(err.to_string(), "网络错误：HTTP 403");
        assert!(
            err.full_message()
                .starts_with("网络错误：HTTP 403（API rate limit exceeded"),
            "完整文案括号追加详情：{}",
            err.full_message()
        );
        assert!(err.is_transient(), "限流类状态异常归瞬时（可静默重试）");
    }

    /// 契约：非 JSON / 空 message 的错误响应体 → detail None，
    /// 主文案与完整文案一致（无可追加的详情）。
    #[tokio::test]
    async fn check_update_status_error_without_message_falls_back() {
        for body in ["", "<html>blocked</html>", r#"{"message":"  "}"#] {
            let err = check_update(&MockHttp::status_body(403, body), "0.1.0")
                .await
                .unwrap_err();
            let UpdateError::HttpStatus { detail, .. } = &err else {
                panic!("应仍为 HttpStatus 变体：{err:?}");
            };
            assert_eq!(detail, &None, "body={body:?} 不应解析出 detail");
            assert_eq!(err.full_message(), err.to_string());
        }
    }

    /// 契约：detail 超长截断（200 字符 + 省略号），防异常响应刷屏。
    #[test]
    fn extract_error_message_truncates_long_text() {
        let long = "x".repeat(500);
        let detail = extract_error_message(&format!(r#"{{"message":"{long}"}}"#))
            .expect("长 message 应被提取（截断后）");
        assert!(detail.chars().count() <= 201, "截断到 200 字符加省略号");
        assert!(detail.ends_with('…'));
        // 恰好不超长时不加省略号
        let exact = "y".repeat(200);
        let detail = extract_error_message(&format!(r#"{{"message":"{exact}"}}"#)).unwrap();
        assert_eq!(detail, exact);
        assert!(!detail.ends_with('…'));
    }

    /// 契约：多字节字符（中文）截断不 panic 且不出半截字符。
    #[test]
    fn extract_error_message_truncates_on_char_boundary() {
        let long = "错".repeat(300);
        let detail = extract_error_message(&format!(r#"{{"message":"{long}"}}"#)).unwrap();
        let chars: Vec<char> = detail.chars().collect();
        assert!(chars.len() <= 201);
        assert!(chars.iter().all(|c| *c != '\u{FFFD}'), "不得出现替换字符");
    }

    #[tokio::test]
    async fn check_update_bad_json_is_deterministic_parse_error() {
        let err = check_update(&MockHttp::ok("not json"), "0.1.0")
            .await
            .unwrap_err();
        assert!(!err.is_transient(), "解析失败是确定性错误：{err}");
        assert!(matches!(err, UpdateError::Parse(_)));
    }

    #[test]
    fn pick_asset_prefers_setup_exe_then_any_exe() {
        let mk = |name: &str| ReleaseAsset {
            name: name.into(),
            browser_download_url: format!("https://x/{name}"),
            size: 1,
        };
        let assets = vec![mk("a.zip"), mk("b.exe"), mk("c-setup.exe")];
        assert_eq!(pick_asset(&assets).unwrap().name, "c-setup.exe");
        let assets = vec![mk("a.zip"), mk("b.exe")];
        assert_eq!(pick_asset(&assets).unwrap().name, "b.exe");
        assert_eq!(pick_asset(&[mk("a.zip")]), None, "无 exe 资产 → None");
    }

    #[tokio::test]
    async fn progress_api_keeps_legacy_downloaders_compatible() {
        let reporter = RecordingProgress::default();
        let bytes = LegacyDownloader
            .download_with_progress("https://x/setup.exe", &reporter)
            .await
            .unwrap();
        assert_eq!(bytes, vec![1, 2, 3, 4]);
        assert_eq!(
            reporter.0.lock().unwrap().as_slice(),
            &[DownloadProgress {
                downloaded_bytes: 4,
                total_bytes: Some(4),
                bytes_per_second: 0,
            }],
            "旧实现无需改签名，也应至少收到完成态"
        );
    }

    #[tokio::test]
    async fn reqwest_downloader_reports_initial_and_completed_progress() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nDATA")
                .unwrap();
        });

        let reporter = RecordingProgress::default();
        let bytes = ReqwestAssetDownloader::new()
            .download_with_progress(&format!("http://{addr}/setup.exe"), &reporter)
            .await
            .unwrap();
        server.join().unwrap();

        assert_eq!(bytes, b"DATA");
        let reports = reporter.0.lock().unwrap();
        assert_eq!(
            reports.first(),
            Some(&DownloadProgress {
                downloaded_bytes: 0,
                total_bytes: Some(4),
                bytes_per_second: 0,
            })
        );
        assert_eq!(reports.last().unwrap().downloaded_bytes, 4);
        assert_eq!(reports.last().unwrap().total_bytes, Some(4));
    }

    #[test]
    fn speed_uses_elapsed_time_and_handles_zero_duration() {
        assert_eq!(
            calculate_bytes_per_second(1_500, Duration::from_millis(500)),
            3_000
        );
        assert_eq!(calculate_bytes_per_second(1_500, Duration::ZERO), 0);
    }

    // ---- 代理构造 ----

    #[test]
    fn proxy_url_of_maps_port_to_local_http_proxy() {
        assert_eq!(proxy_url_of(None), None, "未配置端口 → 不走代理");
        assert_eq!(
            proxy_url_of(Some(7897)),
            Some("http://127.0.0.1:7897".into())
        );
        assert_eq!(proxy_url_of(Some(1)), Some("http://127.0.0.1:1".into()));
        assert_eq!(
            proxy_url_of(Some(u16::MAX)),
            Some(format!("http://127.0.0.1:{}", u16::MAX))
        );
    }

    #[test]
    fn try_with_proxy_builds_or_rejects_url() {
        assert!(
            ReqwestAssetDownloader::try_with_proxy(None).is_ok(),
            "None 等价于 new()"
        );
        assert!(
            ReqwestAssetDownloader::try_with_proxy(Some("http://127.0.0.1:7897")).is_ok(),
            "合法代理 URL 应构造成功"
        );
        assert!(
            ReqwestAssetDownloader::try_with_proxy(Some("not a url")).is_err(),
            "非法 URL（无 scheme）应返回 Err 而非静默回退"
        );
    }

    // ---- 节流 / 每日定时 ----

    #[test]
    fn should_check_gating() {
        let now = 1_000_000_000_000u64;
        assert!(should_check(true, None, now), "从未检测 → 应检");
        assert!(!should_check(false, None, now), "开关关闭 → 不检");
        assert!(
            !should_check(true, Some(now - DAY_MS + 1), now),
            "不足 24h → 不检"
        );
        assert!(should_check(true, Some(now - DAY_MS), now), "恰 24h → 应检");
    }

    #[test]
    fn parse_hhmm_bounds() {
        assert_eq!(parse_hhmm("09:00"), Some((9, 0)));
        assert_eq!(parse_hhmm("23:59"), Some((23, 59)));
        assert_eq!(parse_hhmm("0:00"), Some((0, 0)));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("09:60"), None);
        assert_eq!(parse_hhmm("9"), None);
        assert_eq!(parse_hhmm("ab:cd"), None);
        assert_eq!(parse_hhmm(" 09:00 "), Some((9, 0)), "容忍空白");
    }

    /// due_daily 依赖本地时区，用"相对构造"测语义：同一 now 下，
    /// last_check 分别取"同一天较早时刻"与"昨天同一时刻"。
    #[test]
    fn due_daily_semantics() {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        let now_ms = now.timestamp_millis() as u64;
        let hhmm = format!("{:02}:{:02}", now.hour(), now.minute());
        // 恒过点语义下：同日已检 → false；未检（昨天检的）→ true
        let earlier_today = now - chrono::TimeDelta::seconds(60);
        assert!(
            !due_daily(Some(earlier_today.timestamp_millis() as u64), &hhmm, now_ms),
            "今天已检过 → 不到点"
        );
        let yesterday = now - chrono::TimeDelta::seconds(20 * 3600);
        assert!(
            due_daily(Some(yesterday.timestamp_millis() as u64), &hhmm, now_ms)
                || yesterday.num_days_from_ce() == now.num_days_from_ce(),
            "跨天未检且已过点 → 应检（构造仍落在同一天时跳过）"
        );
        assert!(due_daily(None, &hhmm, now_ms), "从未检且已过点 → 应检");
        // 设定时刻尚未到达：目标 = 下一小时同分（不跨天时构造有效）
        let later = format!("{:02}:{:02}", (now.hour() + 1) % 24, now.minute());
        if now.hour() < 23 {
            assert!(!due_daily(None, &later, now_ms), "未到设定时刻 → 不检");
        }
        // 非法 hhmm 永不到点
        assert!(!due_daily(None, "9时", now_ms));
        assert!(!due_daily(None, "99:00", now_ms));
    }

    #[test]
    fn due_check_combines_throttle_and_daily() {
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        let now_ms = now.timestamp_millis() as u64;
        let hhmm = format!("{:02}:{:02}", now.hour(), now.minute());
        // 距上次不足 24h 但跨天到点 → due_daily 兜住
        let hours_ago_20 = now - chrono::TimeDelta::seconds(20 * 3600);
        if hours_ago_20.num_days_from_ce() < now.num_days_from_ce() {
            assert!(
                due_check(
                    true,
                    Some(hours_ago_20.timestamp_millis() as u64),
                    &hhmm,
                    now_ms
                ),
                "跨天到点即使不足 24h 也应检"
            );
        }
        // 开关关闭全兜底
        assert!(!due_check(false, None, &hhmm, now_ms));
    }

    // ---- 落盘 ----

    #[test]
    fn write_atomic_bytes_roundtrip_and_no_tmp_left() {
        let dir = std::env::temp_dir().join(format!("qt-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("setup.exe");
        let payload = vec![0x4d_u8, 0x5a, 0x00, 0xff, 0x01];
        write_atomic_bytes(&path, &payload).unwrap();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            payload,
            "二进制原样（含 0x00）"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不留 tmp 残留");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Downloader trait 的对象安全性与 Arc 共享（编译期契约）。
    #[test]
    fn downloader_is_object_safe() {
        let _d: Arc<dyn AssetDownloader> = Arc::new(ReqwestAssetDownloader::new());
    }
}
