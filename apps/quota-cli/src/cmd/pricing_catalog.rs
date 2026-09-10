//! `quota pricing catalog`：模型与价格目录的状态查看与手动更新（T-04）。
//!
//! status 只读本地（零网络、零客户端构造）；update 显式联网，直连优先 +
//! settings 代理兜底（与安装包更新检测共用同一代理端口）。本票不含自动
//! 补检（T-06）。`status_json` / `render_status` / `outcome_*` 为纯函数。

use std::path::Path;
use std::time::Duration;

use quota_core::pricing_catalog::sync::{
    CATALOG_CACHE_FILE, CatalogOrigin, CatalogStatusView, CatalogSync, CatalogUpdateOutcome,
};
use quota_core::{CatalogCacheEnvelope, CatalogSyncError, FallbackReason};
use serde::Serialize;
use serde_json::json;

use crate::ctx::Ctx;
use crate::lang::Lang;
use crate::texts::{T, t};

/// 目录状态 + 磁盘信封元数据的组合视图（status 输出形状）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CatalogStatusJson {
    pub revision: u64,
    /// "bundled" | "cached"
    pub origin: &'static str,
    /// "no_cache" | "corrupted_cache" | "incompatible_cache" | "stale_cache" | null
    pub fallback_reason: Option<&'static str>,
    pub last_attempt_ms: Option<u64>,
    pub last_success_ms: Option<u64>,
    pub last_error: Option<String>,
}

fn origin_str(origin: CatalogOrigin) -> &'static str {
    match origin {
        CatalogOrigin::Bundled => "bundled",
        CatalogOrigin::Cached => "cached",
    }
}

fn fallback_str(reason: Option<&FallbackReason>) -> Option<&'static str> {
    reason.map(|r| match r {
        FallbackReason::NoCache => "no_cache",
        FallbackReason::CorruptedCache => "corrupted_cache",
        FallbackReason::IncompatibleCache => "incompatible_cache",
        FallbackReason::StaleCache => "stale_cache",
    })
}

/// 组装本地状态视图（纯函数；effective 来自装载，meta 来自磁盘信封）。
pub fn status_view(
    status: &CatalogStatusView,
    meta: (Option<u64>, Option<u64>, Option<String>),
) -> CatalogStatusJson {
    CatalogStatusJson {
        revision: status.revision,
        origin: origin_str(status.origin),
        fallback_reason: fallback_str(status.fallback_reason.as_ref()),
        last_attempt_ms: meta.0.or(status.last_attempt_ms),
        last_success_ms: meta.1.or(status.last_success_ms),
        last_error: meta.2.clone().or_else(|| status.last_error.clone()),
    }
}

/// 读磁盘信封的同步元数据（损坏/缺失 → 全空；不联网）。
pub fn envelope_meta(data_root: &Path) -> (Option<u64>, Option<u64>, Option<String>) {
    match std::fs::read_to_string(data_root.join(CATALOG_CACHE_FILE)) {
        Ok(text) => serde_json::from_str::<CatalogCacheEnvelope>(&text)
            .map(|e| (e.last_attempt_ms, e.last_success_ms, e.last_error))
            .unwrap_or((None, None, None)),
        Err(_) => (None, None, None),
    }
}

/// update 结果的 JSON 形状（`transient` 供脚本区分可重试失败）。
pub fn outcome_json(
    outcome: &CatalogUpdateOutcome,
    status: &CatalogStatusJson,
) -> serde_json::Value {
    match outcome {
        CatalogUpdateOutcome::Updated { catalog } => json!({
            "result": "updated",
            "revision": catalog.revision,
        }),
        CatalogUpdateOutcome::Unchanged { revision } => json!({
            "result": "unchanged",
            "revision": revision,
        }),
        CatalogUpdateOutcome::Busy => json!({
            "result": "busy",
            "revision": status.revision,
        }),
        CatalogUpdateOutcome::Failed(e) => json!({
            "result": "failed",
            "error": e.to_string(),
            "transient": matches!(e, CatalogSyncError::Network(_) | CatalogSyncError::BadResponse(_)),
        }),
    }
}

/// update 失败的退出码：网络/响应类瞬时 → 2（可重试）；拒绝/IO 确定性 → 1；
/// updated / unchanged / busy → 0（busy 非失败，状态已明确输出）。
pub fn outcome_exit_code(outcome: &CatalogUpdateOutcome) -> i32 {
    match outcome {
        CatalogUpdateOutcome::Failed(e) => match e {
            CatalogSyncError::Network(_) | CatalogSyncError::BadResponse(_) => 2,
            CatalogSyncError::Rejected(_) | CatalogSyncError::Io(_) => 1,
        },
        _ => 0,
    }
}

fn fmt_time(lang: Lang, ms: Option<u64>) -> String {
    match ms {
        Some(ms) => crate::render::fmt_datetime_in_tz(ms, None),
        None => t(lang, T::CatalogNever).to_string(),
    }
}

/// 状态文本输出（纯函数）。
pub fn render_status(s: &CatalogStatusJson, lang: Lang) -> String {
    let origin_label = match s.origin {
        "bundled" => t(lang, T::CatalogOriginBundled),
        _ => t(lang, T::CatalogOriginCached),
    };
    let mut lines = vec![format!(
        "{}：revision {} · {}",
        t(lang, T::CatalogStatusTitle),
        s.revision,
        origin_label
    )];
    if let Some(reason) = s.fallback_reason {
        let reason_label = match reason {
            "no_cache" => t(lang, T::CatalogFallbackNoCache),
            "corrupted_cache" => t(lang, T::CatalogFallbackCorrupted),
            "incompatible_cache" => t(lang, T::CatalogFallbackIncompatible),
            _ => t(lang, T::CatalogFallbackStale),
        };
        lines.push(format!(
            "  {}：{}",
            t(lang, T::CatalogFallbackLabel),
            reason_label
        ));
    }
    lines.push(format!(
        "  {}：{}",
        t(lang, T::CatalogLastAttempt),
        fmt_time(lang, s.last_attempt_ms)
    ));
    lines.push(format!(
        "  {}：{}",
        t(lang, T::CatalogLastSuccess),
        fmt_time(lang, s.last_success_ms)
    ));
    if let Some(err) = &s.last_error {
        lines.push(format!("  {}：{}", t(lang, T::CatalogLastError), err));
    }
    lines.join("\n")
}

/// update 结果文本输出（纯函数）。
pub fn render_outcome(outcome: &CatalogUpdateOutcome, lang: Lang) -> String {
    match outcome {
        CatalogUpdateOutcome::Updated { catalog } => {
            format!(
                "{} revision {}",
                t(lang, T::CatalogUpdateUpdated),
                catalog.revision
            )
        }
        CatalogUpdateOutcome::Unchanged { revision } => {
            format!(
                "{}（revision {revision}）",
                t(lang, T::CatalogUpdateUnchanged)
            )
        }
        CatalogUpdateOutcome::Busy => t(lang, T::CatalogUpdateBusy).to_string(),
        CatalogUpdateOutcome::Failed(e) => format!("{}{e}", t(lang, T::CatalogUpdateFailed)),
    }
}

/// 普通定价命令的到期补检预算（spec §7：含两通道；到时取消用本地目录）。
pub const AUTO_CHECK_BUDGET: Duration = Duration::from_secs(5);

/// 普通定价展示命令（非 JSON 模式）的到期自动补检：判定经磁盘信封
/// 元数据（与 GUI 共享节流状态）；开关关闭或未到期零网络；联网部分
/// 受 5 秒总预算约束，超时/失败静默——本地业务结果不受影响（成功则
/// 本次命令继续用装载前快照，下次命令读新目录）。
pub async fn maybe_auto_check(ctx: &Ctx) {
    let dir = ctx.catalog_dir();
    let (attempt, success, _) = envelope_meta(&dir);
    let prefs = crate::settings_io::load_prefs(&ctx.config_path);
    if !quota_core::catalog_should_auto_check(
        prefs.auto_update_pricing_catalog,
        attempt,
        success,
        crate::settings_io::now_ms(),
    ) {
        return;
    }
    let proxy = quota_core::update::proxy_url_of(prefs.update_proxy_port);
    let Ok(clients) =
        quota_core::update::build_dual_http_clients(Duration::from_secs(10), proxy.as_deref())
    else {
        return;
    };
    let sync = CatalogSync::new(
        dir,
        Box::new(clients.direct.clone()),
        clients
            .proxied
            .clone()
            .map(|c| Box::new(c) as Box<dyn quota_core::http::HttpClient>),
        Box::new(crate::settings_io::now_ms),
    );
    // 短命进程不做退出后仍需完成的任务：预算内未完成即放弃
    let _ = tokio::time::timeout(AUTO_CHECK_BUDGET, sync.update()).await;
}

/// `pricing catalog status`：只读本地（零网络）。
pub fn run_status(ctx: &Ctx, json: bool) -> i32 {
    let effective = quota_core::load_effective(&ctx.catalog_dir());
    // 磁盘信封元数据与内存装载拼合（信封在则用其 attempt/success/error）
    let sync_meta = envelope_meta(&ctx.catalog_dir());
    let view = CatalogStatusView {
        revision: effective.catalog.revision,
        origin: effective.origin,
        fallback_reason: effective.fallback_reason,
        last_attempt_ms: None,
        last_success_ms: None,
        last_error: None,
    };
    let view = status_view(&view, sync_meta);
    if json {
        match serde_json::to_string_pretty(&view) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{}{e}", t(ctx.lang, T::Err));
                return 1;
            }
        }
    } else {
        println!("{}", render_status(&view, ctx.lang));
    }
    0
}

/// `pricing catalog update`：显式联网；生产入口按 settings 代理端口构造
/// 双通道客户端（与安装包更新检测同一端口口径）。
pub async fn run_update(ctx: &Ctx, json: bool) -> i32 {
    let prefs = crate::settings_io::load_prefs(&ctx.config_path);
    let proxy = quota_core::update::proxy_url_of(prefs.update_proxy_port);
    let Ok(clients) =
        quota_core::update::build_dual_http_clients(Duration::from_secs(10), proxy.as_deref())
    else {
        eprintln!(
            "{}{}",
            t(ctx.lang, T::Err),
            t(ctx.lang, T::UpdateClientFail)
        );
        return 1;
    };
    run_update_with(
        ctx,
        json,
        Box::new(clients.direct.clone()),
        clients
            .proxied
            .clone()
            .map(|c| Box::new(c) as Box<dyn quota_core::http::HttpClient>),
    )
    .await
}

/// 可注入入口（测试传 mock 客户端；数据根取自 ctx）。
pub async fn run_update_with(
    ctx: &Ctx,
    json: bool,
    direct: Box<dyn quota_core::http::HttpClient>,
    proxied: Option<Box<dyn quota_core::http::HttpClient>>,
) -> i32 {
    let sync = CatalogSync::new(
        ctx.catalog_dir(),
        direct,
        proxied,
        Box::new(crate::settings_io::now_ms),
    );
    let outcome = sync.update().await;
    let code = outcome_exit_code(&outcome);
    if json {
        let view = status_view(&sync.status(), envelope_meta(&ctx.catalog_dir()));
        match serde_json::to_string_pretty(&outcome_json(&outcome, &view)) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{}{e}", t(ctx.lang, T::Err));
                return 1;
            }
        }
    } else {
        match &outcome {
            CatalogUpdateOutcome::Failed(_) => {
                eprintln!("{}", render_outcome(&outcome, ctx.lang));
            }
            _ => println!("{}", render_outcome(&outcome, ctx.lang)),
        }
    }
    code
}

/// `pricing catalog validate`：离线校验数据文件（使用与运行时相同的
/// core 解析器与校验规则，无第二套 schema 解释）。仅传候选路径时做
/// 单包完整校验；附 `--baseline` 时额外生成审核差异报告并执行
/// revision 递增与物理删除检查（数据 PR / CI 共用入口）。
pub fn run_validate(path: &str, baseline: Option<&str>, json: bool) -> i32 {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("读取 {path} 失败：{e}");
            return 1;
        }
    };
    let candidate = match quota_core::parse_catalog(&text) {
        Ok(c) => c,
        Err(e) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"valid": false, "error": e.to_string()})
                );
            } else {
                eprintln!("校验失败：{e}");
            }
            return 1;
        }
    };
    let Some(baseline_path) = baseline else {
        if json {
            println!(
                "{}",
                serde_json::json!({"valid": true, "revision": candidate.revision})
            );
        } else {
            println!("校验通过：revision {}", candidate.revision);
        }
        return 0;
    };
    let base_text = match std::fs::read_to_string(baseline_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("读取基线 {baseline_path} 失败：{e}");
            return 1;
        }
    };
    let base = match quota_core::parse_catalog(&base_text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("基线校验失败（基线本身必须合法）：{e}");
            return 1;
        }
    };
    // 物理删除 / revision 不递增 / 同版本异内容：确定性拒绝
    let mut failures: Vec<String> = Vec::new();
    if let Err(e) = quota_core::validate_no_removal(&candidate, &base) {
        failures.push(e.to_string());
    }
    if candidate.revision <= base.revision {
        failures.push(format!(
            "revision 须严格递增：基线 {}，候选 {}",
            base.revision, candidate.revision
        ));
    }
    let diffs = quota_core::catalog_diff(&base, &candidate);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "valid": failures.is_empty(),
                "revision": candidate.revision,
                "baseline_revision": base.revision,
                "failures": failures,
                "diffs": diffs.iter().map(|d| d.report_line()).collect::<Vec<_>>(),
            })
        );
    } else {
        println!(
            "审核差异（基线 rev{} → 候选 rev{}）：",
            base.revision, candidate.revision
        );
        for d in &diffs {
            println!("  - {}", d.report_line());
        }
        if failures.is_empty() {
            println!("对比校验通过");
        } else {
            for f in &failures {
                eprintln!("拒绝：{f}");
            }
        }
    }
    if failures.is_empty() { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quota_core::http::{HttpError, HttpRequest, HttpResponse};
    use std::sync::Mutex;

    /// 单响应 mock（网络失败或固定响应；CLI 侧测试用）。
    #[derive(Default)]
    struct MockClient {
        fail: bool,
        body: String,
        status: u16,
        calls: Mutex<usize>,
    }

    impl MockClient {
        fn ok(body: &str) -> Box<Self> {
            Box::new(Self {
                fail: false,
                body: body.into(),
                status: 200,
                calls: Mutex::new(0),
            })
        }
        fn failing() -> Box<Self> {
            Box::new(Self {
                fail: true,
                ..Default::default()
            })
        }
    }

    #[async_trait]
    impl quota_core::http::HttpClient for MockClient {
        async fn execute(&self, _req: HttpRequest) -> Result<HttpResponse, HttpError> {
            *self.calls.lock().unwrap() += 1;
            if self.fail {
                return Err(HttpError::Network("mock".into()));
            }
            Ok(HttpResponse {
                status: self.status,
                raw: self.body.clone().into_bytes(),
                body: self.body.clone(),
            })
        }
    }

    /// 契约：状态视图拼合——bundled + 无缓存 → revision=种子版本、
    /// 四态回退原因字符串映射正确。
    #[test]
    fn status_view_maps_enums() {
        let view = CatalogStatusView {
            revision: 5,
            origin: CatalogOrigin::Cached,
            fallback_reason: None,
            last_attempt_ms: None,
            last_success_ms: None,
            last_error: None,
        };
        let j = status_view(&view, (Some(1), Some(2), Some("x".into())));
        assert_eq!(j.origin, "cached");
        assert_eq!(j.fallback_reason, None);
        assert_eq!(j.last_attempt_ms, Some(1), "信封元数据优先透出");
        assert_eq!(j.last_error.as_deref(), Some("x"));

        let view = CatalogStatusView {
            revision: 1,
            origin: CatalogOrigin::Bundled,
            fallback_reason: Some(FallbackReason::CorruptedCache),
            last_attempt_ms: None,
            last_success_ms: None,
            last_error: None,
        };
        let j = status_view(&view, (None, None, None));
        assert_eq!(j.origin, "bundled");
        assert_eq!(j.fallback_reason, Some("corrupted_cache"));
    }

    /// 契约：退出码映射——updated/unchanged/busy=0；网络/响应类=2（瞬时）；
    /// 拒绝/IO=1（确定性）。
    #[test]
    fn outcome_exit_codes() {
        let updated = CatalogUpdateOutcome::Updated {
            catalog: quota_core::bundled_catalog().clone(),
        };
        assert_eq!(outcome_exit_code(&updated), 0);
        assert_eq!(
            outcome_exit_code(&CatalogUpdateOutcome::Unchanged { revision: 1 }),
            0
        );
        assert_eq!(outcome_exit_code(&CatalogUpdateOutcome::Busy), 0);
        assert_eq!(
            outcome_exit_code(&CatalogUpdateOutcome::Failed(CatalogSyncError::Network(
                "x".into()
            ))),
            2
        );
        assert_eq!(
            outcome_exit_code(&CatalogUpdateOutcome::Failed(
                CatalogSyncError::BadResponse("404".into())
            )),
            2
        );
        assert_eq!(
            outcome_exit_code(&CatalogUpdateOutcome::Failed(CatalogSyncError::Rejected(
                "r".into()
            ))),
            1
        );
        assert_eq!(
            outcome_exit_code(&CatalogUpdateOutcome::Failed(CatalogSyncError::Io(
                "io".into()
            ))),
            1
        );
    }

    /// 契约：update JSON 形状——updated/failed（含 transient 标记）。
    #[test]
    fn outcome_json_shape() {
        let status = CatalogStatusJson {
            revision: 1,
            origin: "bundled",
            fallback_reason: None,
            last_attempt_ms: None,
            last_success_ms: None,
            last_error: None,
        };
        let updated = CatalogUpdateOutcome::Updated {
            catalog: quota_core::bundled_catalog().clone(),
        };
        assert_eq!(outcome_json(&updated, &status)["result"], "updated");
        let failed = CatalogUpdateOutcome::Failed(CatalogSyncError::Network("boom".into()));
        let j = outcome_json(&failed, &status);
        assert_eq!(j["result"], "failed");
        assert_eq!(j["transient"], true);
        let rejected = CatalogUpdateOutcome::Failed(CatalogSyncError::Rejected("bad".into()));
        assert_eq!(outcome_json(&rejected, &status)["transient"], false);
        assert_eq!(
            outcome_json(&CatalogUpdateOutcome::Busy, &status)["result"],
            "busy"
        );
    }

    /// 契约：文本状态输出双语含核心字段。
    #[test]
    fn render_status_bilingual() {
        let view = CatalogStatusJson {
            revision: 2,
            origin: "cached",
            fallback_reason: None,
            last_attempt_ms: Some(1_789_000_000_000),
            last_success_ms: None,
            last_error: Some("HTTP 404".into()),
        };
        let zh = render_status(&view, Lang::Zh);
        assert!(zh.contains("revision 2"), "{zh}");
        assert!(zh.contains("缓存"), "{zh}");
        assert!(zh.contains("HTTP 404"), "{zh}");
        let en = render_status(&view, Lang::En);
        assert!(en.contains("revision 2"), "{en}");
        assert!(en.contains("cached"), "{en}");
    }

    // ---- 端到端串联（V-10：mock 发布 → update → show 读到新价） ---------

    fn temp_ctx(tag: &str) -> (crate::ctx::Ctx, std::path::PathBuf) {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "quota-cli-cat-{}-{}-{}",
            tag,
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        (
            crate::ctx::Ctx::with_store(
                root.join("config.json"),
                std::sync::Arc::new(quota_core::InMemoryStore::new()),
            ),
            root,
        )
    }

    fn deepseek_config(path: &std::path::Path) {
        quota_core::AppConfig::default()
            .save(path)
            .unwrap_or_else(|_| {
                // AppConfig 无 Default 直存时手工构造空 provider 列表
                let json = r#"{"custom_models": {}, "providers": []}"#;
                std::fs::write(path, json).unwrap();
            });
    }

    /// rev2 新价包：flash CNY 峰价改为 0.99（区分种子旧价 0.04）。
    fn rev2_package() -> String {
        let mut cat = quota_core::bundled_catalog().clone();
        cat.revision = 2;
        for m in &mut cat.providers[0].suites[0].models {
            if m.id == "flash" {
                m.peak = Some(quota_core::PriceTier::full(0.99, 0.99, 0.99));
            }
        }
        serde_json::to_string(&cat).unwrap()
    }

    /// 契约（V-10）：update 成功落盘 rev2 → 后续 pricing 解析走有效目录
    /// 读到新价；退出码 0。
    #[tokio::test]
    async fn update_then_resolve_reads_new_price() {
        let (ctx, root) = temp_ctx("e2e");
        deepseek_config(&ctx.config_path);
        // 更新前：内置种子价
        let before = quota_core::load_effective(&ctx.catalog_dir());
        let entry = deepseek_entry_for_resolve();
        let r = quota_core::pricing::resolve_in_catalog(
            &entry,
            &Default::default(),
            None,
            &before.catalog,
        )
        .unwrap();
        assert_eq!(r.peak.as_ref().unwrap().cache_hit_input, Some(0.04));

        // update：mock 200 rev2 新价包
        let code = run_update_with(&ctx, true, MockClient::ok(&rev2_package()), None).await;
        assert_eq!(code, 0);

        // 更新后：show 同一装载路径读到 rev2 新价与版本
        let after = quota_core::load_effective(&ctx.catalog_dir());
        assert_eq!(after.catalog.revision, 2);
        assert_eq!(after.origin, quota_core::CatalogOrigin::Cached);
        let r = quota_core::pricing::resolve_in_catalog(
            &entry,
            &Default::default(),
            None,
            &after.catalog,
        )
        .unwrap();
        assert_eq!(
            r.peak.as_ref().unwrap().cache_hit_input,
            Some(0.99),
            "读到新价"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 契约：status 只读本地（零网络调用）且退出 0。
    #[test]
    fn status_never_touches_network() {
        let (ctx, root) = temp_ctx("st-net");
        deepseek_config(&ctx.config_path);
        assert_eq!(run_status(&ctx, false), 0);
        assert_eq!(run_status(&ctx, true), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 契约：update 失败返回非零（网络类瞬时=2）；--config 数据根不被
    /// 误写（缓存只落在 ctx.catalog_dir）。
    #[tokio::test]
    async fn update_failure_exit_code_and_config_untouched() {
        let (ctx, root) = temp_ctx("fail");
        deepseek_config(&ctx.config_path);
        let before = std::fs::read_to_string(&ctx.config_path).unwrap();
        let code = run_update_with(&ctx, false, MockClient::failing(), None).await;
        assert_eq!(code, 2, "网络失败为瞬时退出码");
        assert_eq!(
            std::fs::read_to_string(&ctx.config_path).unwrap(),
            before,
            "config.json 不被目录更新触碰"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 契约：跨进程锁被占 → busy 文案 + 退出 0；锁文件不被删。
    #[tokio::test]
    async fn update_busy_is_zero_and_keeps_foreign_lock() {
        let (ctx, root) = temp_ctx("busy");
        deepseek_config(&ctx.config_path);
        let lock = ctx.catalog_dir().join("pricing-catalog.lock");
        std::fs::write(&lock, b"pid=99999").unwrap();
        let code = run_update_with(&ctx, false, MockClient::ok(&rev2_package()), None).await;
        assert_eq!(code, 0, "busy 非失败");
        assert!(lock.exists(), "他人锁不被删");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn deepseek_entry_for_resolve() -> quota_core::ProviderEntry {
        use quota_core::config::ProviderKind;
        quota_core::ProviderEntry {
            id: "p1".into(),
            name: "DeepSeek".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            api_key2_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: quota_core::PlanVariant::Auto,
            use_proxy: false,
            console_url: None,
        }
    }
}
