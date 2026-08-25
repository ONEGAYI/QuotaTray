//! `quota query`：查询全部或指定条目，表格/JSON 输出，退出码三分。

use std::time::Duration;

use futures::future::join_all;
use quota_core::model::QueryError;
use quota_core::{AppConfig, HistoryStore, ProviderEntry, QueryEngine, Vault};

use crate::ctx::Ctx;
use crate::exit::exit_code;
use crate::render::{self, QueryOutcome};
use crate::texts::{T, t};

/// 默认轮询间隔（分钟）。spec §3：M2b 固定 5 分钟（条目级配置后续里程碑引入）。
pub const DEFAULT_INTERVAL_MIN: u64 = 5;

pub async fn run(
    ctx: &Ctx,
    ids: Vec<String>,
    json: bool,
    watch: bool,
    interval_min: Option<u64>,
) -> i32 {
    let lang = ctx.lang;
    let cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };

    let entries = match select_entries(&cfg.providers, &ids) {
        Ok(entries) => entries,
        Err(missing) => {
            for id in missing {
                eprintln!(
                    "{}{}",
                    t(lang, T::Err),
                    crate::texts::entry_not_found(lang, &id)
                );
            }
            return 1;
        }
    };
    if entries.is_empty() {
        println!("{}", t(lang, T::QueryNoEntries));
        return 0;
    }

    let vault = match ctx.open_vault() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    let engine = match ctx.new_engine() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    // 历史库为非关键附属数据：打开失败仅告警，查询照常（不写历史）。
    let history = open_history(ctx);

    let print_once = |outcomes: &[QueryOutcome]| {
        if json {
            let payload: Vec<_> = outcomes.iter().map(|o| o.to_json()).collect();
            println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        } else {
            let now_ms = chrono::Local::now().timestamp_millis();
            println!("{}", render::query_table(outcomes, lang, now_ms));
        }
    };

    if watch {
        let term = console::Term::stdout();
        let period = Duration::from_secs(interval_min.unwrap_or(DEFAULT_INTERVAL_MIN) * 60);
        // ctrl_c 常驻监听（tokio 信号无监听者时被丢弃，须覆盖查询与休眠两阶段）
        let mut ctrl_c = std::pin::pin!(tokio::signal::ctrl_c());
        loop {
            let outcomes = tokio::select! {
                r = run_queries(&engine, &vault, &entries) => r,
                _ = &mut ctrl_c => break,
            };
            record_history(history.as_ref(), &outcomes, lang);
            let _ = term.clear_screen();
            print_once(&outcomes);
            println!("{}", crate::texts::watch_hint(lang, period.as_secs() / 60));
            tokio::select! {
                _ = tokio::time::sleep(period) => {}
                _ = &mut ctrl_c => break,
            }
        }
        0
    } else {
        let outcomes = run_queries(&engine, &vault, &entries).await;
        record_history(history.as_ref(), &outcomes, lang);
        print_once(&outcomes);
        exit_code(&flatten(&outcomes))
    }
}

/// 打开历史库；失败告警一次后本进程跳过历史写入。
fn open_history(ctx: &Ctx) -> Option<HistoryStore> {
    match HistoryStore::open(&ctx.history_path()) {
        Ok(store) => Some(store),
        Err(e) => {
            eprintln!("{}{e}", t(ctx.lang, T::HistoryOpenFail));
            None
        }
    }
}

/// 成功查询写入历史库（仅 Ok 结果；写失败告警，不影响查询输出与退出码）。
fn record_history(
    history: Option<&HistoryStore>,
    outcomes: &[QueryOutcome],
    lang: crate::lang::Lang,
) {
    let Some(store) = history else { return };
    let now = chrono::Local::now().timestamp_millis().max(0) as u64;
    for outcome in outcomes {
        if let Ok(data) = &outcome.result {
            if let Err(e) = store.record(&outcome.id, data, now) {
                eprintln!("{}{e}", t(lang, T::HistoryWriteFail));
                return;
            }
        }
    }
}

/// outcomes → 退出码计算的借用适配（Result 映射丢弃成功载荷）。
fn flatten(outcomes: &[QueryOutcome]) -> Vec<Result<(), QueryError>> {
    outcomes
        .iter()
        .map(|o| o.result.as_ref().map(|_| ()).map_err(|e| e.clone()))
        .collect()
}

/// 按配置顺序选出目标条目；返回 Err(缺失 id 列表)。
///
/// 无 id 参数 → 全部 enabled；有 id → 精确匹配（允许选中禁用条目，
/// 便于临时查询已禁用项）。
pub fn select_entries(
    providers: &[ProviderEntry],
    ids: &[String],
) -> Result<Vec<ProviderEntry>, Vec<String>> {
    if ids.is_empty() {
        return Ok(providers.iter().filter(|e| e.enabled).cloned().collect());
    }
    // 重复 id 去重保序（quota query a a 只查一次）
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<&String> = ids.iter().filter(|id| seen.insert(id.as_str())).collect();
    let mut picked = Vec::with_capacity(ids.len());
    let mut missing = Vec::new();
    for id in ids {
        match providers.iter().find(|e| e.id == *id) {
            Some(e) => picked.push(e.clone()),
            None => missing.push((*id).clone()),
        }
    }
    if missing.is_empty() {
        Ok(picked)
    } else {
        Err(missing)
    }
}

/// 并行查询全部条目，结果按配置顺序聚合（spec：并行发起、顺序输出）。
pub async fn run_queries(
    engine: &QueryEngine,
    vault: &Vault,
    entries: &[ProviderEntry],
) -> Vec<QueryOutcome> {
    let results = join_all(entries.iter().map(|e| engine.query(vault, e))).await;
    entries
        .iter()
        .zip(results)
        .map(|(e, result)| QueryOutcome {
            id: e.id.clone(),
            name: e.name.clone(),
            result,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quota_core::InMemoryStore;
    use quota_core::PlanVariant;
    use quota_core::config::ProviderKind;
    use quota_core::http::{HttpClient, HttpError, HttpRequest, HttpResponse};
    use std::sync::Arc;

    /// 按 URL 子串路由的 mock：为不同平台端点返回不同响应体。
    #[derive(Clone)]
    struct RouteHttp {
        routes: Vec<(&'static str, u16, &'static str)>,
    }

    impl RouteHttp {
        fn ok(url: &'static str, body: &'static str) -> Self {
            Self {
                routes: vec![(url, 200, body)],
            }
        }
        fn with(mut self, url: &'static str, status: u16, body: &'static str) -> Self {
            self.routes.push((url, status, body));
            self
        }
    }

    #[async_trait::async_trait]
    impl HttpClient for RouteHttp {
        async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
            for (frag, status, body) in &self.routes {
                if req.url.contains(frag) {
                    return Ok(HttpResponse {
                        status: *status,
                        body: body.to_string(),
                        raw: Vec::new(),
                    });
                }
            }
            Err(HttpError::Network("mock 无路由".into()))
        }
    }

    fn entry(id: &str, provider: &str) -> ProviderEntry {
        ProviderEntry {
            id: id.into(),
            name: format!("条目-{id}"),
            kind: ProviderKind::Native {
                provider: provider.into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        }
    }

    fn template_entry(id: &str) -> ProviderEntry {
        let tpl: quota_core::TemplateConfig = serde_json::from_value(serde_json::json!({
            "request": { "url": "{{baseUrl}}/tpl/balance" },
            "extract": { "remaining": "$.balance" }
        }))
        .unwrap();
        ProviderEntry {
            id: id.into(),
            name: format!("条目-{id}"),
            kind: ProviderKind::Template(Box::new(tpl)),
            enabled: true,
            api_key_enc: None,
            base_url: Some("https://tpl.demo.com".into()),
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        }
    }

    fn make_setup(http: RouteHttp) -> (Vault, QueryEngine) {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let engine = QueryEngine::new(Arc::new(http), quota_core::DEFAULT_TIMEOUT);
        (vault, engine)
    }

    fn with_key(vault: &Vault, e: &mut ProviderEntry, key: &str) {
        e.set_api_key(vault, key).unwrap();
    }

    /// spec 验收：3 个 native 平台 + 1 个 template 条目全链输出正确、退出码 0。
    #[tokio::test]
    async fn three_natives_plus_template_all_ok() {
        let http = RouteHttp::ok(
            "deepseek",
            r#"{"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"88.00"}]}"#,
        )
        .with(
            "siliconflow",
            200,
            r#"{"data":{"totalBalance":"42.50","name":"u"}}"#,
        )
        .with(
            "openrouter",
            200,
            r#"{"data":{"total_credits":100,"total_usage":40}}"#,
        )
        .with("tpl.demo.com", 200, r#"{"balance":7.5}"#);
        let (vault, engine) = make_setup(http);
        let mut entries = vec![
            entry("d1", "deepseek"),
            entry("s1", "siliconflow"),
            entry("o1", "openrouter"),
            template_entry("t1"),
        ];
        for e in &mut entries {
            with_key(&vault, e, "sk-test");
        }

        let outcomes = run_queries(&engine, &vault, &entries).await;
        assert_eq!(outcomes.len(), 4);
        for o in &outcomes {
            assert!(o.result.is_ok(), "{} 失败：{:?}", o.id, o.result);
        }
        assert_eq!(exit_code(&flatten(&outcomes)), 0);

        // 双语表格均含全部余额值
        for lang in [crate::lang::Lang::Zh, crate::lang::Lang::En] {
            let table = render::query_table(&outcomes, lang, 1_700_000_000_000);
            assert!(table.contains("88"), "{lang:?} deepseek 余额：{table}");
            assert!(table.contains("42.5"), "{lang:?} siliconflow 余额：{table}");
            assert!(
                table.contains("60"),
                "{lang:?} openrouter remaining：{table}"
            );
            assert!(table.contains("7.5"), "{lang:?} template 余额：{table}");
        }
    }

    /// 契约：确定性（401）混入 → 退出码 1；仅瞬时（503/超时）→ 2。
    #[tokio::test]
    async fn exit_code_reflects_error_mix() {
        let http = RouteHttp::ok(
            "deepseek",
            r#"{"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"1"}]}"#,
        )
        .with("siliconflow", 401, "")
        .with("openrouter", 503, "");
        let (vault, engine) = make_setup(http);
        let mut entries = vec![
            entry("d1", "deepseek"),
            entry("s1", "siliconflow"),
            entry("o1", "openrouter"),
        ];
        for e in &mut entries {
            with_key(&vault, e, "k");
        }
        let outcomes = run_queries(&engine, &vault, &entries).await;
        assert_eq!(exit_code(&flatten(&outcomes)), 1);

        // deepseek OK + openrouter 503 → 仅瞬时 → 2
        let mixed = vec![
            outcomes[0].clone(),
            QueryOutcome {
                id: "x".into(),
                name: "n".into(),
                result: Err(QueryError::transient("503")),
            },
        ];
        assert_eq!(exit_code(&flatten(&mixed)), 2);
    }

    /// 契约：未配置凭据 → 确定性失败（退出码 1），且不发起网络请求。
    #[tokio::test]
    async fn missing_credentials_is_deterministic() {
        let (vault, engine) = make_setup(RouteHttp::ok("x", "{}"));
        let entries = vec![entry("d1", "deepseek")];
        let outcomes = run_queries(&engine, &vault, &entries).await;
        assert_eq!(exit_code(&flatten(&outcomes)), 1);
    }

    /// 契约：query 指定不存在的 id → 退出 1（select_entries 拦截，不触网络/凭据库）。
    #[tokio::test]
    async fn query_missing_id_exits_one() {
        let ctx = Ctx::with_store(
            std::path::PathBuf::from("nonexistent.json"),
            std::sync::Arc::new(quota_core::InMemoryStore::new()),
        );
        assert_eq!(run(&ctx, vec!["zzz".into()], false, false, None).await, 1);
    }

    /// 契约：无任何 enabled 条目 → 提示并退出 0（首次使用场景）。
    #[tokio::test]
    async fn query_empty_config_exits_zero() {
        let ctx = Ctx::with_store(
            std::path::PathBuf::from("nonexistent.json"),
            std::sync::Arc::new(quota_core::InMemoryStore::new()),
        );
        assert_eq!(run(&ctx, vec![], false, false, None).await, 0);
    }

    /// 契约：条目筛选——默认 enabled 全集；指定 id 精确匹配；缺失 id 报告。
    #[test]
    fn select_entries_semantics() {
        let mut a = entry("a", "deepseek");
        a.enabled = true;
        let mut b = entry("b", "deepseek");
        b.enabled = false;
        let providers = vec![a, b];

        let sel = select_entries(&providers, &[]).unwrap();
        assert_eq!(sel.len(), 1, "默认只查 enabled");

        let sel = select_entries(&providers, &["b".to_string()]).unwrap();
        assert_eq!(sel[0].id, "b", "指定 id 可选中禁用条目");

        // 重复 id 去重保序：只查一次
        let sel = select_entries(&providers, &["b".to_string(), "b".to_string()]).unwrap();
        assert_eq!(sel.len(), 1);

        // 部分存在部分缺失 → 整体报错
        let err = select_entries(&providers, &["a".to_string(), "zzz".to_string()]).unwrap_err();
        assert_eq!(err, vec!["zzz".to_string()]);
    }

    /// 历史写入契约的沙箱目录（config.json 与 history.db 同目录）。
    fn history_test_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quota-cli-query-history-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn outcome(id: &str, result: Result<Vec<quota_core::UsageData>, QueryError>) -> QueryOutcome {
        QueryOutcome {
            id: id.into(),
            name: format!("条目-{id}"),
            result,
        }
    }

    /// 契约：record_history 只写成功结果（失败条目无历史点）；
    /// 打开失败的库返回 None 且不 panic。
    /// 注：run() 全链会构造真实 reqwest 引擎（网络不可 mock），
    /// 故接线函数单独锁定，run 内调用由编译保证。
    #[test]
    fn record_history_writes_successful_outcomes_only() {
        let dir = history_test_dir("record");
        let ctx = Ctx::with_store(
            dir.join("config.json"),
            Arc::new(quota_core::InMemoryStore::new()),
        );
        let store = open_history(&ctx).expect("沙箱目录应可打开历史库");

        let ok = outcome(
            "d1",
            Ok(vec![quota_core::UsageData {
                plan_name: Some("five_hour".into()),
                remaining: Some(88.0),
                unit: Some("CNY".into()),
                ..Default::default()
            }]),
        );
        let failed = outcome("e1", Err(QueryError::transient("503")));
        record_history(Some(&store), &[ok, failed], ctx.lang);

        let reopened = quota_core::HistoryStore::open(&ctx.history_path()).unwrap();
        let points = reopened.range("d1", 0).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].window_key, "five_hour");
        assert_eq!(points[0].remaining, Some(88.0));
        assert!(
            reopened.range("e1", 0).unwrap().is_empty(),
            "失败条目不产生历史点"
        );

        // None（打开失败）时静默跳过
        record_history(None, &[], ctx.lang);

        // Windows 上句柄存活时删除会失败，先释放再清理
        drop(store);
        drop(reopened);
        let _ = std::fs::remove_dir_all(dir);
    }
}
