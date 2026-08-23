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

use async_trait::async_trait;
use chrono::{Datelike, TimeZone, Timelike};
use serde::Deserialize;

use crate::http::{HttpClient, HttpError, HttpRequest};

/// 当前程序版本（workspace 单源继承，与 CLI `--version` / GUI app 版本一致）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// release 所在仓库（owner/repo）。
pub const GITHUB_REPO: &str = "ONEGAYI/QuotaTray";

/// 周期检测的最小间隔（自动检测每 24h 至多一次，与每日到点判定互补）。
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// 下载大小上限（256MB）：NSIS 安装包为 MB 级，超限视为远端异常，防御性拒绝。
const MAX_DOWNLOAD_BYTES: usize = 256 * 1024 * 1024;

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
    #[error("release 信息解析失败：{0}")]
    Parse(String),
}

impl UpdateError {
    /// 是否瞬时（网络类，可重试/可静默）。
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            UpdateError::Http(HttpError::Network(_) | HttpError::Timeout)
        )
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
        // 限流/5xx 等归为网络类（端侧按瞬时处理）；HttpError 无状态码变体，
        // 以 Network 携带状态描述，不为此扩枚举。
        status => return Err(HttpError::Network(format!("HTTP {status}")).into()),
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

/// 安装包下载通道：独立于 HttpClient（String body 与 15s 超时载不动字节流）。
#[async_trait]
pub trait AssetDownloader: Send + Sync {
    async fn download(&self, url: &str) -> Result<Vec<u8>, HttpError>;
}

/// reqwest 实现的安装包下载器。
///
/// 要点：10 分钟总超时；默认 302 跟随（browser_download_url 会跳转到
/// objects.githubusercontent.com）；256MB 上限（Content-Length 预检 +
/// 实际字节数复检）。不依赖 [`crate::http::ReqwestHttpClient`] 的客户端
/// ——那是查询用的 15s 短超时配置。
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
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .unwrap_or_default(), // builder 仅设超时，失败回退默认客户端
        }
    }
}

#[async_trait]
impl AssetDownloader for ReqwestAssetDownloader {
    async fn download(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let resp = self
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
        if let Some(len) = resp.content_length() {
            if len as usize > MAX_DOWNLOAD_BYTES {
                return Err(HttpError::Network(format!(
                    "安装包过大（{len} 字节，上限 {MAX_DOWNLOAD_BYTES}）"
                )));
            }
        }
        let bytes = resp.bytes().await.map_err(map_reqwest_err)?;
        if bytes.len() > MAX_DOWNLOAD_BYTES {
            return Err(HttpError::Network("安装包超过大小上限".into()));
        }
        Ok(bytes.to_vec())
    }
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
    use std::sync::Arc;

    // ---- 版本比较 ----

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
