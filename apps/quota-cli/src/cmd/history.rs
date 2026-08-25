//! `quota history`：查询历史查看（三档范围 + 分页）与清理。
//!
//! 展示路径：原始点 →（窗口过滤）→ 按范围档聚合成时间桶 → 分页。
//! 终端默认交互翻页（空格/b/q）；`--page N` 或管道输出走非交互打印。

use console::Key;
use dialoguer::Confirm;
use dialoguer::theme::ColorfulTheme;
use quota_core::{AppConfig, HistoryPoint, HistoryStore};

use crate::ctx::Ctx;
use crate::render::{self, HistoryJson};
use crate::texts::{self, T, t};

/// 回看范围三档（`--range`）；聚合桶粒度随范围走，单窗口任意档 ≤ 168 桶。
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum HistoryRange {
    /// 近 24 小时（15 分钟桶）
    #[value(name = "24h")]
    Hours24,
    /// 近 7 天（1 小时桶，默认档）
    #[value(name = "7d")]
    Days7,
    /// 近 30 天（6 小时桶）
    #[value(name = "30d")]
    Days30,
}

impl HistoryRange {
    fn span_ms(self) -> u64 {
        match self {
            Self::Hours24 => 24 * 60 * 60 * 1000,
            Self::Days7 => 7 * 24 * 60 * 60 * 1000,
            Self::Days30 => 30 * 24 * 60 * 60 * 1000,
        }
    }

    fn bucket_ms(self) -> u64 {
        match self {
            Self::Hours24 => 15 * 60 * 1000,
            Self::Days7 => 60 * 60 * 1000,
            Self::Days30 => 6 * 60 * 60 * 1000,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Hours24 => "24h",
            Self::Days7 => "7d",
            Self::Days30 => "30d",
        }
    }
}

/// 每页默认行数。
const DEFAULT_PAGE_SIZE: u64 = 20;

pub fn run_show(
    ctx: &Ctx,
    id: String,
    window: Option<String>,
    range: HistoryRange,
    page_size: Option<u64>,
    page: Option<u64>,
    json: bool,
) -> i32 {
    let lang = ctx.lang;
    let cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    if let Err(missing) =
        crate::cmd::query::select_entries(&cfg.providers, std::slice::from_ref(&id))
    {
        for miss in missing {
            eprintln!("{}{}", t(lang, T::Err), texts::entry_not_found(lang, &miss));
        }
        return 1;
    }
    let name = cfg
        .providers
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.name.clone())
        .unwrap_or_default();

    let store = match HistoryStore::open(&ctx.history_path()) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::HistoryOpenFail));
            return 1;
        }
    };
    let now = chrono::Local::now().timestamp_millis().max(0) as u64;
    let mut points = match store.range(&id, now.saturating_sub(range.span_ms())) {
        Ok(points) => points,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    if let Some(key) = &window {
        points.retain(|p| &p.window_key == key);
    }

    if json {
        let payload = HistoryJson {
            id,
            name,
            range: range.as_str().into(),
            points,
        };
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return 0;
    }
    if points.is_empty() {
        println!("{}", t(lang, T::HistoryEmpty));
        return 0;
    }

    let rows = render::bucket_points_by_window(&points, range.bucket_ms());
    let size = page_size.unwrap_or(DEFAULT_PAGE_SIZE);
    let total = render::total_pages(rows.len(), size);

    match page {
        // 非交互打印模式：只输出指定页
        Some(p) => {
            if p > total {
                eprintln!(
                    "{}{}",
                    t(lang, T::Err),
                    texts::history_page_out_of_range(lang, p, total)
                );
                return 1;
            }
            println!(
                "{}",
                render::history_table(render::page_slice(&rows, p, size), lang)
            );
            println!("{}", texts::history_page_note(lang, p, total));
            0
        }
        // 管道（非终端）：整表输出，翻页交由调用方（less / head）
        None if !console::Term::stdout().is_term() => {
            println!("{}", render::history_table(&rows, lang));
            0
        }
        // 终端默认：交互翻页
        None => interactive_pager(&rows, size, total, lang),
    }
}

/// 交互翻页：清屏重绘当前页 + 按键提示；空格/→ 下一页、b/← 上一页、q/Esc 退出。
fn interactive_pager(rows: &[HistoryPoint], size: u64, total: u64, lang: crate::lang::Lang) -> i32 {
    let term = console::Term::stdout();
    let mut page: u64 = 1;
    loop {
        let _ = term.clear_screen();
        println!(
            "{}",
            render::history_table(render::page_slice(rows, page, size), lang)
        );
        println!("{}", texts::history_page_footer(lang, page, total));
        let key = match term.read_key() {
            Ok(key) => key,
            Err(_) => break,
        };
        match key {
            Key::Char(' ') | Key::Enter | Key::ArrowRight => page = (page + 1).min(total),
            Key::Char('b') | Key::ArrowLeft => page = page.saturating_sub(1).max(1),
            Key::Char('q') | Key::Escape => break,
            _ => {}
        }
    }
    0
}

pub fn run_clear(ctx: &Ctx, id: Option<String>, yes: bool) -> i32 {
    let lang = ctx.lang;
    if let Some(id) = &id {
        let cfg = match AppConfig::load(&ctx.config_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{}{e}", t(lang, T::Err));
                return 1;
            }
        };
        if let Err(missing) =
            crate::cmd::query::select_entries(&cfg.providers, std::slice::from_ref(id))
        {
            for miss in missing {
                eprintln!("{}{}", t(lang, T::Err), texts::entry_not_found(lang, &miss));
            }
            return 1;
        }
    }
    if !yes && !confirm(texts::history_clear_confirm(lang, id.as_deref())) {
        println!("{}", texts::cancelled(lang));
        return 0;
    }
    let store = match HistoryStore::open(&ctx.history_path()) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::HistoryOpenFail));
            return 1;
        }
    };
    if let Err(e) = store.clear(id.as_deref()) {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    println!("{}", texts::history_cleared(lang, id.as_deref()));
    0
}

fn confirm(prompt: String) -> bool {
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .default(false)
        .interact()
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use quota_core::config::{PlanVariant, ProviderEntry, ProviderKind};
    use quota_core::{AppConfig, InMemoryStore, UsageData};

    use super::*;

    fn test_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quota-cli-history-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ctx_with_entry(dir: &std::path::Path, id: &str) -> Ctx {
        let ctx = Ctx::with_store(dir.join("config.json"), Arc::new(InMemoryStore::new()));
        AppConfig {
            providers: vec![ProviderEntry {
                id: id.into(),
                name: "历史条目".into(),
                kind: ProviderKind::Native {
                    provider: "deepseek".into(),
                },
                enabled: true,
                api_key_enc: None,
                base_url: None,
                pricing: None,
                plan_variant: PlanVariant::Auto,
                use_proxy: false,
            }],
            custom_models: Default::default(),
        }
        .save(&ctx.config_path)
        .unwrap();
        ctx
    }

    fn record_point(ctx: &Ctx, id: &str, remaining: f64) {
        // 用当前时刻落点（回看窗口从 now 起算，固定历史时间戳会被过滤）
        let now = chrono::Local::now().timestamp_millis().max(0) as u64;
        HistoryStore::open(&ctx.history_path())
            .unwrap()
            .record(
                id,
                &[UsageData {
                    plan_name: Some("five_hour".into()),
                    remaining: Some(remaining),
                    unit: Some("%".into()),
                    ..Default::default()
                }],
                now,
            )
            .unwrap();
    }

    /// 契约：范围三档取值解析（clap ValueEnum 名称锁定）。
    #[test]
    fn range_values_parse() {
        use clap::ValueEnum;
        let parse = |s: &str| HistoryRange::from_str(s, false).unwrap();
        assert_eq!(parse("24h"), HistoryRange::Hours24);
        assert_eq!(parse("7d"), HistoryRange::Days7);
        assert_eq!(parse("30d"), HistoryRange::Days30);
        assert!(HistoryRange::from_str("3d", false).is_err());
    }

    /// 契约：桶粒度随范围档收紧（24h 最细、30d 最粗）。
    #[test]
    fn bucket_granularity_follows_range() {
        assert_eq!(HistoryRange::Hours24.bucket_ms(), 15 * 60 * 1000);
        assert_eq!(HistoryRange::Days7.bucket_ms(), 60 * 60 * 1000);
        assert_eq!(HistoryRange::Days30.bucket_ms(), 6 * 60 * 60 * 1000);
    }

    /// 契约：条目不存在 → 退出 1（不触历史库）。
    #[test]
    fn show_missing_id_exits_one() {
        let dir = test_dir("show-missing");
        let ctx = ctx_with_entry(&dir, "real1");
        assert_eq!(
            run_show(
                &ctx,
                "zzz".into(),
                None,
                HistoryRange::Days7,
                None,
                None,
                false
            ),
            1
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：空历史 → 提示并退出 0；有历史时 --json 与 --page 退出 0，
    /// 页码超界 → 退出 1（测试进程非终端，无 --page 走整表打印分支）。
    #[test]
    fn show_paging_and_json_paths() {
        let dir = test_dir("show-paths");
        let ctx = ctx_with_entry(&dir, "e1");
        // 空历史
        assert_eq!(
            run_show(
                &ctx,
                "e1".into(),
                None,
                HistoryRange::Days7,
                None,
                None,
                false
            ),
            0
        );

        record_point(&ctx, "e1", 42.0);
        assert_eq!(
            run_show(
                &ctx,
                "e1".into(),
                None,
                HistoryRange::Days7,
                None,
                None,
                true
            ),
            0,
            "--json 退出 0"
        );
        assert_eq!(
            run_show(
                &ctx,
                "e1".into(),
                None,
                HistoryRange::Days7,
                Some(1),
                Some(1),
                false
            ),
            0,
            "--page 1 退出 0"
        );
        assert_eq!(
            run_show(
                &ctx,
                "e1".into(),
                None,
                HistoryRange::Days7,
                Some(1),
                Some(2),
                false
            ),
            1,
            "只有 1 页时 --page 2 超界退出 1"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：clear --yes 清空指定条目；单条目 id 不存在 → 退出 1。
    #[test]
    fn clear_removes_history_for_entry() {
        let dir = test_dir("clear");
        let ctx = ctx_with_entry(&dir, "e1");
        record_point(&ctx, "e1", 1.0);

        assert_eq!(run_clear(&ctx, Some("zzz".into()), true), 1);
        assert_eq!(run_clear(&ctx, Some("e1".into()), true), 0);
        assert!(
            HistoryStore::open(&ctx.history_path())
                .unwrap()
                .range("e1", 0)
                .unwrap()
                .is_empty()
        );

        record_point(&ctx, "e1", 2.0);
        assert_eq!(run_clear(&ctx, None, true), 0);
        assert!(
            HistoryStore::open(&ctx.history_path())
                .unwrap()
                .range("e1", 0)
                .unwrap()
                .is_empty(),
            "无 id 清空全部"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
