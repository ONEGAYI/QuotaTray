//! `quota history`：查询历史查看（三档范围 + 窗口语义过滤 + 分页）与清理。
//!
//! 展示路径：原始点 →（窗口过滤：类别 / 精确键 / all，缺省按范围选粒度）
//! → 按范围档聚合成时间桶 → 按类别排序（全部窗口视图分段表头的前提）
//! → 分页。终端默认交互翻页（空格/b/q）；`--page N` 或管道输出走非交互打印。

use console::Key;
use dialoguer::Confirm;
use dialoguer::theme::ColorfulTheme;
use quota_core::{AppConfig, HistoryPoint, HistoryStore, WindowKind, window_kind};

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

    /// 缺省 `--window` 时按范围档选择的窗口类别：
    /// 一天内看 5 小时窗（粒度匹配），更长回看看周窗。
    fn default_window_kind(self) -> WindowKind {
        match self {
            Self::Hours24 => WindowKind::FiveHour,
            Self::Days7 | Self::Days30 => WindowKind::Weekly,
        }
    }
}

/// `--window` 的类别别名（大小写不敏感）→ 语义类别。
const KIND_ALIASES: &[(&str, WindowKind)] = &[
    ("5h", WindowKind::FiveHour),
    ("five_hour", WindowKind::FiveHour),
    ("five-hour", WindowKind::FiveHour),
    ("weekly", WindowKind::Weekly),
    ("week", WindowKind::Weekly),
];

/// 语义类别的规范 token（JSON `window` 字段与提示共用）。
fn kind_token(kind: WindowKind) -> &'static str {
    match kind {
        WindowKind::FiveHour => "5h",
        WindowKind::Weekly => "weekly",
        WindowKind::Other => "other",
    }
}

fn kind_alias(input: &str) -> Option<WindowKind> {
    let lower = input.to_lowercase();
    KIND_ALIASES
        .iter()
        .find(|(alias, _)| *alias == lower)
        .map(|(_, kind)| *kind)
}

/// 窗口过滤结果：生效 token 供 `--json` 透出，两个提示信号供表格路径打印。
#[derive(Debug, Default)]
struct FilterOutcome {
    points: Vec<HistoryPoint>,
    /// 实际生效的过滤口径（"5h" / "weekly" / "all" / 精确键）；未过滤为 None。
    effective: Option<String>,
    /// 缺省类别缺失回退全部，且另一规范类别有点（应打一行回退提示）。
    fallback_note: bool,
    /// 显式过滤无匹配且范围内有点时的可用窗口键清单（应提示）。
    no_match_hint: Option<Vec<String>>,
}

fn filter_by_exact(points: &[HistoryPoint], key: &str) -> Vec<HistoryPoint> {
    points
        .iter()
        .filter(|p| p.window_key == key)
        .cloned()
        .collect()
}

fn filter_by_kind(points: &[HistoryPoint], kind: WindowKind) -> Vec<HistoryPoint> {
    points
        .iter()
        .filter(|p| window_kind(&p.window_key) == kind)
        .cloned()
        .collect()
}

/// `--window` 过滤（在 core 查询之后、JSON/聚桶之前）：
/// - `all`（大小写不敏感）→ 全部窗口；
/// - 其余串先按**精确键**匹配（向后兼容字面名为 five_hour 等的自定义
///   窗口），无命中再查类别别名（5h/five_hour/five-hour、weekly/week）；
/// - 缺省按 `default_kind` 选类别，选中类别无点不强求——回退全部；
///   仅当另一规范类别（5h/周）有点时置 `fallback_note`（余额单窗等
///   两类皆无的场景静默回退，避免每次打扰）。
fn apply_window_filter(
    points: Vec<HistoryPoint>,
    window: Option<&str>,
    default_kind: WindowKind,
) -> FilterOutcome {
    let mut available: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for point in &points {
        available.insert(point.window_key.as_str());
    }
    let has_kind = |kind: WindowKind| points.iter().any(|p| window_kind(&p.window_key) == kind);
    let keys = || -> Vec<String> { available.iter().map(|s| s.to_string()).collect() };

    match window {
        Some(input) if input.eq_ignore_ascii_case("all") => FilterOutcome {
            points,
            effective: Some("all".into()),
            ..Default::default()
        },
        Some(input) => {
            if available.contains(input) {
                FilterOutcome {
                    points: filter_by_exact(&points, input),
                    effective: Some(input.to_string()),
                    ..Default::default()
                }
            } else if let Some(kind) = kind_alias(input) {
                let filtered = filter_by_kind(&points, kind);
                let no_match_hint = if filtered.is_empty() && !available.is_empty() {
                    Some(keys())
                } else {
                    None
                };
                FilterOutcome {
                    points: filtered,
                    effective: Some(kind_token(kind).into()),
                    no_match_hint,
                    ..Default::default()
                }
            } else {
                FilterOutcome {
                    points: Vec::new(),
                    effective: Some(input.to_string()),
                    no_match_hint: (!available.is_empty()).then(keys),
                    ..Default::default()
                }
            }
        }
        None => {
            if has_kind(default_kind) {
                FilterOutcome {
                    points: filter_by_kind(&points, default_kind),
                    effective: Some(kind_token(default_kind).into()),
                    ..Default::default()
                }
            } else {
                let counterpart = match default_kind {
                    WindowKind::FiveHour => WindowKind::Weekly,
                    _ => WindowKind::FiveHour,
                };
                let fallback_note = has_kind(counterpart);
                FilterOutcome {
                    points,
                    effective: None,
                    fallback_note,
                    ..Default::default()
                }
            }
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
    let points = match store.range(&id, now.saturating_sub(range.span_ms())) {
        Ok(points) => points,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    let outcome = apply_window_filter(points, window.as_deref(), range.default_window_kind());
    let points = outcome.points;

    if json {
        let payload = HistoryJson {
            id,
            name,
            range: range.as_str().into(),
            window: outcome.effective,
            points,
        };
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return 0;
    }
    if points.is_empty() {
        match outcome.no_match_hint {
            // 范围内有点但都不满足显式过滤：列出可用窗口键（比通用空提示
            // 更可解释）；完全无数据才走通用提示
            Some(available) => println!(
                "{}",
                texts::history_window_no_match(
                    lang,
                    window.as_deref().unwrap_or_default(),
                    &available
                )
            ),
            None => println!("{}", t(lang, T::HistoryEmpty)),
        }
        return 0;
    }
    if outcome.fallback_note {
        println!(
            "{}",
            texts::history_window_fallback(
                lang,
                &texts::window_kind_label(lang, range.default_window_kind())
            )
        );
    }

    let mut rows = render::bucket_points_by_window(&points, range.bucket_ms());
    render::order_points_by_kind(&mut rows);
    // 分段表头由整个过滤后视图是否跨类别决定（单页单类别也保留段头）
    let sectioned = render::group_points_by_kind(&rows).len() > 1;
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
                render::history_grouped_table(render::page_slice(&rows, p, size), lang, sectioned)
            );
            println!("{}", texts::history_page_note(lang, p, total));
            0
        }
        // 管道（非终端）：整表输出，翻页交由调用方（less / head）
        None if !console::Term::stdout().is_term() => {
            println!("{}", render::history_grouped_table(&rows, lang, sectioned));
            0
        }
        // 终端默认：交互翻页
        None => interactive_pager(&rows, size, total, sectioned, lang),
    }
}

/// 交互翻页：清屏重绘当前页 + 按键提示；空格/→ 下一页、b/← 上一页、q/Esc 退出。
fn interactive_pager(
    rows: &[HistoryPoint],
    size: u64,
    total: u64,
    sectioned: bool,
    lang: crate::lang::Lang,
) -> i32 {
    let term = console::Term::stdout();
    let mut page: u64 = 1;
    loop {
        let _ = term.clear_screen();
        println!(
            "{}",
            render::history_grouped_table(render::page_slice(rows, page, size), lang, sectioned)
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
    use quota_core::{AppConfig, InMemoryStore, UsageData, WindowKind};

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

    /// 契约：缺省窗口类别按范围档选择（24h→5h，7d/30d→周）。
    #[test]
    fn default_window_kind_follows_range() {
        assert_eq!(
            HistoryRange::Hours24.default_window_kind(),
            WindowKind::FiveHour
        );
        assert_eq!(
            HistoryRange::Days7.default_window_kind(),
            WindowKind::Weekly
        );
        assert_eq!(
            HistoryRange::Days30.default_window_kind(),
            WindowKind::Weekly
        );
    }

    fn pts(keys: &[&str]) -> Vec<HistoryPoint> {
        keys.iter()
            .enumerate()
            .map(|(i, key)| HistoryPoint {
                window_key: (*key).into(),
                sampled_at: i as u64,
                used: None,
                remaining: Some(i as f64),
                total: None,
                unit: None,
            })
            .collect()
    }

    fn keys_of(points: &[HistoryPoint]) -> Vec<&str> {
        points.iter().map(|p| p.window_key.as_str()).collect()
    }

    /// 契约：--window 三态（all / 类别名 / 精确键优先）与缺省回退、
    /// 两个提示信号的触发条件。
    #[test]
    fn window_filter_semantics() {
        let mixed = pts(&["Claude 订阅（5h）", "Claude 订阅（week·Opus）", "DeepSeek"]);

        // 缺省：选中类别有点 → 只留该类别（weekly 含 week·Opus 变体）
        let out = apply_window_filter(mixed.clone(), None, WindowKind::Weekly);
        assert_eq!(keys_of(&out.points), vec!["Claude 订阅（week·Opus）"]);
        assert_eq!(out.effective.as_deref(), Some("weekly"));
        assert!(!out.fallback_note);
        assert!(out.no_match_hint.is_none());

        let out = apply_window_filter(mixed.clone(), None, WindowKind::FiveHour);
        assert_eq!(keys_of(&out.points), vec!["Claude 订阅（5h）"]);
        assert_eq!(out.effective.as_deref(), Some("5h"));

        // 缺省类别缺失：回退全部；仅另一规范类别有点时提示
        let out = apply_window_filter(pts(&["Kimi Code（5h）"]), None, WindowKind::Weekly);
        assert_eq!(out.points.len(), 1, "不强求：回退全部窗口");
        assert_eq!(out.effective, None);
        assert!(out.fallback_note);

        // 两类皆无（余额单窗）：静默回退，不提示
        let out = apply_window_filter(pts(&["DeepSeek"]), None, WindowKind::Weekly);
        assert_eq!(out.points.len(), 1);
        assert_eq!(out.effective, None);
        assert!(!out.fallback_note);

        // all（大小写不敏感）：全部窗口、不提示
        let out = apply_window_filter(mixed.clone(), Some("ALL"), WindowKind::Weekly);
        assert_eq!(out.points.len(), 3);
        assert_eq!(out.effective.as_deref(), Some("all"));

        // 精确键优先于别名：字面名为 five_hour 的自定义窗口仍可精确选中
        let custom = pts(&["five_hour", "weekly"]);
        let out = apply_window_filter(custom, Some("five_hour"), WindowKind::Weekly);
        assert_eq!(keys_of(&out.points), vec!["five_hour"]);
        assert_eq!(out.effective.as_deref(), Some("five_hour"));

        // 类别别名（大小写不敏感）：按语义归类过滤
        let out = apply_window_filter(mixed.clone(), Some("weekly"), WindowKind::Weekly);
        assert_eq!(keys_of(&out.points), vec!["Claude 订阅（week·Opus）"]);
        let out = apply_window_filter(mixed.clone(), Some("5H"), WindowKind::Weekly);
        assert_eq!(keys_of(&out.points), vec!["Claude 订阅（5h）"]);

        // 显式精确键无匹配：空结果 + 可用键清单（BTreeSet 排序）
        let out = apply_window_filter(mixed.clone(), Some("zzz"), WindowKind::Weekly);
        assert!(out.points.is_empty());
        assert_eq!(
            out.no_match_hint,
            Some(vec![
                "Claude 订阅（5h）".to_string(),
                "Claude 订阅（week·Opus）".to_string(),
                "DeepSeek".to_string()
            ])
        );

        // 类别名无匹配（有点但均非该类别）同样提示
        let out = apply_window_filter(pts(&["w0"]), Some("weekly"), WindowKind::Weekly);
        assert!(out.points.is_empty());
        assert_eq!(out.no_match_hint.as_ref().map(|v| v.len()), Some(1));

        // 范围内完全无数据：不打无匹配提示（走通用空提示）
        let out = apply_window_filter(vec![], Some("weekly"), WindowKind::Weekly);
        assert!(out.points.is_empty());
        assert_eq!(out.no_match_hint, None);
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
