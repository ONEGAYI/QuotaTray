//! `quota update`：检测 GitHub release 新版本，可选下载安装包。
//!
//! 退出码：成功 0（含无 release / 已最新 / 仅检测）；检测或下载的网络类
//! 失败 2（瞬时）、解析类失败 1（确定性）——与既有三分约定对齐。

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use dialoguer::{Confirm, theme::ColorfulTheme};
use quota_core::http::{HttpClient, ReqwestHttpClient};
use quota_core::update::{
    self, AssetDownloader, DownloadProgress, DownloadProgressReporter, UpdateStatus, VERSION,
};

use crate::ctx::Ctx;
use crate::texts::{self, T, t};

pub struct UpdateArgs {
    /// 仅检测不下载。
    pub check_only: bool,
    /// 跳过下载确认（CI/脚本用）。
    pub yes: bool,
    /// 安装包保存目录（默认当前目录）。
    pub output: Option<PathBuf>,
}

struct CliProgressReporter {
    enabled: bool,
    lang: crate::lang::Lang,
    previous_width: Mutex<usize>,
}

impl CliProgressReporter {
    fn new(lang: crate::lang::Lang) -> Self {
        Self {
            enabled: std::io::stderr().is_terminal(),
            lang,
            previous_width: Mutex::new(0),
        }
    }

    fn finish_line(&self) {
        if self.enabled {
            eprintln!();
        }
    }
}

impl DownloadProgressReporter for CliProgressReporter {
    fn report(&self, progress: DownloadProgress) {
        if !self.enabled {
            return;
        }
        let line = format_cli_progress(self.lang, progress);
        let width = line.chars().count();
        let mut previous = self.previous_width.lock().unwrap();
        let padding = previous.saturating_sub(width);
        eprint!("\r{line}{:padding$}", "");
        let _ = std::io::stderr().flush();
        *previous = width;
    }
}

fn format_cli_progress(lang: crate::lang::Lang, progress: DownloadProgress) -> String {
    let prefix = t(lang, T::UpdateDownloading);
    let downloaded = format_bytes(progress.downloaded_bytes);
    let speed = format!("{}/s", format_bytes(progress.bytes_per_second));
    match progress.total_bytes.filter(|total| *total > 0) {
        Some(total) => {
            let percent = (progress.downloaded_bytes.saturating_mul(100) / total).min(100);
            format!(
                "{prefix} {downloaded} / {} · {speed} · {percent}%",
                format_bytes(total)
            )
        }
        None => format!("{prefix} {downloaded} · {speed}"),
    }
}

fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if value == 0 {
        return "0 B".into();
    }
    let mut scaled = value as f64;
    let mut unit = 0;
    while scaled >= 1024.0 && unit < UNITS.len() - 1 {
        scaled /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{scaled:.1} {}", UNITS[unit])
    }
}

/// 生产入口：reqwest 检测（10s 超时）+ reqwest 下载（10 分钟超时）。
/// 更新代理端口读自 settings.json（GUI 设置页写入，两端口径一致）。
pub async fn run(ctx: &Ctx, args: UpdateArgs) -> i32 {
    let prefs = crate::settings_io::load_prefs(&ctx.config_path);
    if let Some(port) = prefs.update_proxy_port {
        println!("{}", texts::update_proxy_note(ctx.lang, port));
    }
    let proxy = quota_core::update::proxy_url_of(prefs.update_proxy_port);
    let Ok(http) = ReqwestHttpClient::new_with_proxy(Duration::from_secs(10), proxy.as_deref())
    else {
        eprintln!(
            "{}{}",
            t(ctx.lang, T::Err),
            t(ctx.lang, T::UpdateClientFail)
        );
        return 1;
    };
    let Ok(downloader) = update::ReqwestAssetDownloader::try_with_proxy(proxy.as_deref()) else {
        eprintln!(
            "{}{}",
            t(ctx.lang, T::Err),
            t(ctx.lang, T::UpdateClientFail)
        );
        return 1;
    };
    run_with(&http, &downloader, ctx, args).await
}

/// 可注入入口（测试传 mock http/downloader）。
pub async fn run_with(
    http: &dyn HttpClient,
    downloader: &dyn AssetDownloader,
    ctx: &Ctx,
    args: UpdateArgs,
) -> i32 {
    let lang = ctx.lang;
    let status = match update::check_update(http, VERSION, ctx.update_selector()).await {
        Ok(s) => s,
        Err(e) => {
            // 手动检测也算一次检测：写回节流时间戳（失败也写，语义与启动钩子一致）
            let _ = crate::settings_io::write_last_check(
                &ctx.config_path,
                crate::settings_io::now_ms(),
            );
            // 终端无悬停交互，直接展示完整文案（限流 403 等状态异常
            // 在括号内附 GitHub 响应 message）
            eprintln!("{}{}", t(lang, T::UpdateCheckFail), e.full_message());
            return if e.is_transient() { 2 } else { 1 };
        }
    };
    let _ = crate::settings_io::write_last_check(&ctx.config_path, crate::settings_io::now_ms());
    match status {
        UpdateStatus::NoRelease => {
            println!("{}", t(lang, T::UpdateNoRelease));
            0
        }
        UpdateStatus::UpToDate => {
            println!("{}", texts::update_up_to_date(lang));
            0
        }
        UpdateStatus::Available {
            version,
            html_url,
            notes,
            asset,
        } => {
            println!("{}", texts::update_found(lang, &version));
            if let Some(n) = notes {
                println!("{n}");
            }
            if args.check_only {
                match &asset {
                    Some(a) => println!("{}", texts::update_asset_info(lang, &a.name, a.size)),
                    None => {
                        println!("{}", t(lang, T::UpdateManualUrl));
                        println!("{html_url}");
                    }
                }
                return 0;
            }
            let Some(asset) = asset else {
                println!("{}", t(lang, T::UpdateManualUrl));
                println!("{html_url}");
                return 0;
            };
            let dir = args.output.unwrap_or_else(|| PathBuf::from("."));
            if !args.yes {
                let confirmed = Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt(texts::update_confirm(lang, &dir.join(&asset.name)))
                    .default(false)
                    .interact()
                    .unwrap_or(false);
                if !confirmed {
                    println!("{}", texts::cancelled(lang));
                    return 0;
                }
            }
            let progress = CliProgressReporter::new(lang);
            if progress.enabled {
                progress.report(DownloadProgress {
                    downloaded_bytes: 0,
                    total_bytes: (asset.size > 0).then_some(asset.size),
                    bytes_per_second: 0,
                });
            } else {
                println!("{}", t(lang, T::UpdateDownloading));
            }
            let result = downloader
                .download_with_progress(&asset.browser_download_url, &progress)
                .await;
            progress.finish_line();
            match result {
                Err(e) => {
                    eprintln!("{}{e}", t(lang, T::UpdateDownloadFail));
                    // HttpError 全三分类中仅 InvalidRequest 是确定性
                    match e {
                        quota_core::http::HttpError::InvalidRequest(_) => 1,
                        _ => 2,
                    }
                }
                Ok(bytes) => {
                    let path = dir.join(&asset.name);
                    if let Err(e) = update::write_atomic_bytes(&path, &bytes) {
                        eprintln!("{}{e}", t(lang, T::UpdateSaveFail));
                        return 1;
                    }
                    println!("{}", texts::update_saved(lang, &path));
                    // 收尾指引按资产形态分流：zip 退出后手动解压覆盖，
                    // NSIS 才引导运行 installer。
                    println!(
                        "{}",
                        t(
                            lang,
                            if ctx.update_selector().requires_manual_update() {
                                T::UpdateRunHintPortable
                            } else {
                                T::UpdateRunHint
                            }
                        )
                    );
                    0
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;
    use async_trait::async_trait;
    use quota_core::http::{HttpError, HttpRequest, HttpResponse};
    use std::sync::Arc;

    /// 按 URL 子串路由的 mock（query.rs RouteHttp 同款），另捕获请求。
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
                        raw: Vec::new(),
                    });
                }
            }
            Err(HttpError::Network("no route".into()))
        }
    }

    struct FakeDownloader {
        url_frag: &'static str,
        bytes: Vec<u8>,
        fail: bool,
    }

    #[async_trait]
    impl AssetDownloader for FakeDownloader {
        async fn download(&self, url: &str) -> Result<Vec<u8>, HttpError> {
            if self.fail || !url.contains(self.url_frag) {
                return Err(HttpError::Network("mock download fail".into()));
            }
            Ok(self.bytes.clone())
        }
    }

    fn ctx_with(tag: &str, lang: Lang) -> Ctx {
        let dir = std::env::temp_dir().join(format!("quota-cli-upd-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::ctx::Ctx::with_store(
            dir.join("config.json"),
            Arc::new(quota_core::InMemoryStore::new()),
        )
        .with_lang(lang)
    }

    /// mock 资产名按本机架构动态拼装（与 `AssetSelector::installed()`
    /// 的期望名一致）：精确匹配语义下硬编码 x64 名会让测试退化为
    /// x64-only（WoA ARM CI 必炸），动态生成保证任意架构可跑。
    fn local_installed_asset() -> String {
        let selector = quota_core::update::AssetSelector::installed();
        quota_core::update::expected_asset_name("9.9.9", selector.arch, selector.flavor)
    }

    fn release_json() -> String {
        format!(
            r#"{{
        "tag_name": "v9.9.9",
        "html_url": "https://github.com/ONEGAYI/QuotaTray/releases/v9.9.9",
        "body": "changelog",
        "assets": [{{"name": "{}",
                    "browser_download_url": "https://x/setup.exe", "size": 4}}]
    }}"#,
            local_installed_asset()
        )
    }

    fn release_http() -> RouteHttp {
        RouteHttp {
            routes: vec![("releases/latest", 200, release_json())],
        }
    }

    /// 便携形态的 mock release：资产只含 portable zip（与
    /// `Ctx::portable` 的选择器期望名一致）。
    fn portable_release_http() -> RouteHttp {
        let asset = quota_core::update::expected_asset_name(
            "9.9.9",
            quota_core::update::arch_label(),
            quota_core::update::Flavor::PortableZip,
        );
        RouteHttp {
            routes: vec![(
                "releases/latest",
                200,
                format!(
                    r#"{{"tag_name": "v9.9.9",
                    "html_url": "https://github.com/ONEGAYI/QuotaTray/releases/v9.9.9",
                    "body": "changelog",
                    "assets": [{{"name": "{asset}",
                                "browser_download_url": "https://x/setup.exe", "size": 4}}]}}"#
                ),
            )],
        }
    }

    /// 契约：便携上下文的检测只命中 portable zip——同一 release 不含
    /// setup.exe 时仍可下载（形态分流不回退到安装包）。
    #[tokio::test]
    async fn portable_ctx_selects_portable_zip_asset() {
        let root = std::env::temp_dir().join(format!("quota-cli-upd-port-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let out =
            std::env::temp_dir().join(format!("quota-cli-upd-portout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let store = quota_core::FileStore::new(quota_core::portable_key_path(&root));
        quota_core::Vault::open(&store).unwrap();
        let ctx = Ctx::portable(root.clone(), Lang::Zh);
        let dl = FakeDownloader {
            url_frag: "setup.exe",
            bytes: vec![0x50, 0x4b, 0x00],
            fail: false,
        };
        let code = run_with(
            &portable_release_http(),
            &dl,
            &ctx,
            UpdateArgs {
                check_only: false,
                yes: true,
                output: Some(out.clone()),
            },
        )
        .await;
        assert_eq!(code, 0);
        let saved = out.join(quota_core::update::expected_asset_name(
            "9.9.9",
            quota_core::update::arch_label(),
            quota_core::update::Flavor::PortableZip,
        ));
        assert_eq!(
            std::fs::read(&saved).unwrap(),
            vec![0x50, 0x4b, 0x00],
            "便携形态下载 portable zip"
        );
        let _ = std::fs::remove_dir_all(&out);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn no_release_and_up_to_date_exit_zero_both_langs() {
        for lang in [Lang::Zh, Lang::En] {
            let ctx = ctx_with("norelease", lang);
            let http = RouteHttp {
                routes: vec![("releases/latest", 404, "".into())],
            };
            let code = run_with(
                &http,
                &never_downloader(),
                &ctx,
                UpdateArgs {
                    check_only: true,
                    yes: true,
                    output: None,
                },
            )
            .await;
            assert_eq!(code, 0, "{lang:?} 无 release → 0");
        }
        // 已最新（tag 相同）
        let ctx = ctx_with("uptodate", Lang::Zh);
        let http = RouteHttp {
            routes: vec![(
                "releases/latest",
                200,
                r#"{"tag_name":"v0.1.0","assets":[]}"#.into(),
            )],
        };
        let code = run_with(
            &http,
            &never_downloader(),
            &ctx,
            UpdateArgs {
                check_only: true,
                yes: true,
                output: None,
            },
        )
        .await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn found_and_download_roundtrip() {
        let dir = std::env::temp_dir().join(format!("quota-cli-upd-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = ctx_with("dl", Lang::Zh);
        let dl = FakeDownloader {
            url_frag: "setup.exe",
            bytes: vec![0x4d, 0x5a, 0x00],
            fail: false,
        };
        let code = run_with(
            &release_http(),
            &dl,
            &ctx,
            UpdateArgs {
                check_only: false,
                yes: true,
                output: Some(dir.clone()),
            },
        )
        .await;
        assert_eq!(code, 0);
        let saved = dir.join(local_installed_asset());
        assert_eq!(
            std::fs::read(&saved).unwrap(),
            vec![0x4d, 0x5a, 0x00],
            "二进制原样落盘"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn download_failure_is_transient_exit_two() {
        let ctx = ctx_with("dlfail", Lang::En);
        let dl = FakeDownloader {
            url_frag: "setup.exe",
            bytes: vec![],
            fail: true,
        };
        let code = run_with(
            &release_http(),
            &dl,
            &ctx,
            UpdateArgs {
                check_only: false,
                yes: true,
                output: None,
            },
        )
        .await;
        assert_eq!(code, 2, "网络类下载失败 → 2");
    }

    #[tokio::test]
    async fn check_error_transient_vs_parse_exit_codes() {
        let ctx = ctx_with("err", Lang::Zh);
        // 网络失败 → 2
        let http = RouteHttp { routes: vec![] }; // 无路由 → Network 错
        let code = run_with(
            &http,
            &never_downloader(),
            &ctx,
            UpdateArgs {
                check_only: true,
                yes: true,
                output: None,
            },
        )
        .await;
        assert_eq!(code, 2);
        // 解析失败 → 1
        let http = RouteHttp {
            routes: vec![("releases/latest", 200, "not json".into())],
        };
        let code = run_with(
            &http,
            &never_downloader(),
            &ctx,
            UpdateArgs {
                check_only: true,
                yes: true,
                output: None,
            },
        )
        .await;
        assert_eq!(code, 1);
    }

    #[test]
    fn cli_progress_formats_known_and_unknown_totals() {
        let known = format_cli_progress(
            Lang::En,
            DownloadProgress {
                downloaded_bytes: 5 * 1024 * 1024,
                total_bytes: Some(20 * 1024 * 1024),
                bytes_per_second: 2 * 1024 * 1024,
            },
        );
        assert_eq!(known, "Downloading… 5.0 MB / 20.0 MB · 2.0 MB/s · 25%");

        let unknown = format_cli_progress(
            Lang::Zh,
            DownloadProgress {
                downloaded_bytes: 1536,
                total_bytes: None,
                bytes_per_second: 0,
            },
        );
        assert_eq!(unknown, "下载中… 1.5 KB · 0 B/s");
    }

    fn never_downloader() -> FakeDownloader {
        FakeDownloader {
            url_frag: "\0",
            bytes: vec![],
            fail: true,
        }
    }
}
