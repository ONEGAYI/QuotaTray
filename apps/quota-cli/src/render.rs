//! 输出渲染：comfy-table UTF-8 边框表格与 `--json` 输出结构。
//!
//! 渲染函数均为纯函数（`&[T] → String`），语言经参数传入，
//! 输出字符串由单元测试按双语锁定。

use chrono::TimeZone;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, presets::UTF8_FULL};
use quota_core::model::{QueryError, UsageData};
use quota_core::pricing::{PeakWindow, PriceTier, Weekday};
use quota_core::provider::NativeMeta;
use quota_core::{ProviderEntry, ProviderKind};
use serde::Serialize;

use crate::lang::Lang;
use crate::texts::{T, t};

/// 单个条目的查询结果（query 命令的聚合单元）。
#[derive(Clone)]
pub struct QueryOutcome {
    pub id: String,
    pub name: String,
    pub result: Result<Vec<UsageData>, QueryError>,
}

/// `quota query --json` 的单条输出（spec §3 结构：data/error 为可空）。
#[derive(Serialize)]
pub struct QueryJson {
    pub id: String,
    pub name: String,
    pub ok: bool,
    pub data: Option<Vec<UsageData>>,
    pub error: Option<ErrorJson>,
}

#[derive(Serialize)]
pub struct ErrorJson {
    /// "transient" | "deterministic"
    pub kind: &'static str,
    pub message: String,
}

impl QueryOutcome {
    pub fn to_json(&self) -> QueryJson {
        QueryJson {
            id: self.id.clone(),
            name: self.name.clone(),
            ok: self.result.is_ok(),
            data: self.result.as_ref().ok().cloned(),
            error: self.result.as_ref().err().map(|e| ErrorJson {
                kind: if e.is_transient() {
                    "transient"
                } else {
                    "deterministic"
                },
                message: e.message().to_string(),
            }),
        }
    }
}

// ---- 基础表格 ------------------------------------------------------------

pub(crate) fn new_table(header: &[&str]) -> Table {
    let mut t = Table::new();
    t.load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    t.set_header(header.iter().map(Cell::new));
    t
}

/// 数值格式化：None → "-"；88.0 → "88"（Display 自然去尾零）。
pub fn fmt_num(v: Option<f64>) -> String {
    v.map(|n| format!("{n}")).unwrap_or_else(|| "-".into())
}

/// 条目类型标签：`native:deepseek` / `template`。
pub fn kind_label(kind: &ProviderKind) -> String {
    match kind {
        ProviderKind::Native { provider } => format!("native:{provider}"),
        ProviderKind::Template(_) => "template".into(),
    }
}

// ---- 各命令表格 ----------------------------------------------------------

/// `quota query` 表格：名称 / 套餐 / 已用 / 剩余 / 单位 / 重置 / 状态。
/// 多窗口条目每窗口一行；条目失败占一行，状态列带错误分类前缀。
/// `now_ms` 注入当前时刻（倒计时纯函数可测）。
pub fn query_table(outcomes: &[QueryOutcome], lang: Lang, now_ms: i64) -> String {
    let mut table = new_table(&[
        t(lang, T::ColName),
        t(lang, T::ColPlan),
        t(lang, T::ColUsed),
        t(lang, T::ColRemaining),
        t(lang, T::ColUnit),
        t(lang, T::ColReset),
        t(lang, T::ColStatus),
    ]);
    for o in outcomes {
        match &o.result {
            Ok(rows) if rows.is_empty() => {
                table.add_row(row(&o.name, &UsageData::default(), t(lang, T::OkNoData), now_ms));
            }
            Ok(rows) => {
                for d in rows {
                    let status = match d.is_valid {
                        Some(false) => format!(
                            "{}{}",
                            t(lang, T::InvalidPrefix),
                            d.invalid_message.clone().unwrap_or_default()
                        ),
                        _ => "OK".to_string(),
                    };
                    table.add_row(row(&o.name, d, &status, now_ms));
                }
            }
            Err(e) => {
                let kind = if e.is_transient() {
                    t(lang, T::Transient)
                } else {
                    t(lang, T::Deterministic)
                };
                table.add_row(row(
                    &o.name,
                    &UsageData::default(),
                    &format!("[{kind}] {}", e.message()),
                    now_ms,
                ));
            }
        }
    }
    table.to_string()
}

/// 额度重置倒计时（语言中性缩写）："21m" / "3h21m" / "3h" / "4d17h" / "4d"。
/// None 或已到期（<= 0）显示 "-"——窗口翻转在即，倒计时无意义。
/// 跨入天级后丢弃分钟粒度（天级窗口为周/月窗口，小时精度已足够）。
pub fn fmt_reset_countdown(reset_at: Option<i64>, now_ms: i64) -> String {
    let Some(reset_at) = reset_at else {
        return "-".into();
    };
    let total_min = (reset_at - now_ms) / 60_000;
    if total_min <= 0 {
        return "-".into();
    }
    if total_min < 60 {
        return format!("{total_min}m");
    }
    let h = total_min / 60;
    if h < 24 {
        return if total_min % 60 == 0 {
            format!("{h}h")
        } else {
            format!("{h}h{}m", total_min % 60)
        };
    }
    let d = h / 24;
    if h % 24 == 0 {
        format!("{d}d")
    } else {
        format!("{d}d{}h", h % 24)
    }
}

/// 一行数据：数值列右对齐；重置列为倒计时（无/过期显示 "-"）。
fn row(name: &str, d: &UsageData, status: &str, now_ms: i64) -> Vec<Cell> {
    vec![
        Cell::new(name),
        Cell::new(d.plan_name.clone().unwrap_or_else(|| "-".into())),
        Cell::new(fmt_num(d.used)).set_alignment(CellAlignment::Right),
        Cell::new(fmt_num(d.remaining)).set_alignment(CellAlignment::Right),
        Cell::new(d.unit.clone().unwrap_or_else(|| "-".into())),
        Cell::new(fmt_reset_countdown(d.reset_at, now_ms)).set_alignment(CellAlignment::Right),
        Cell::new(status),
    ]
}

/// `quota list` 表格：id / 名称 / 类型 / 启用 / 凭据已配。
pub fn list_table(entries: &[ProviderEntry], lang: Lang) -> String {
    let mut table = new_table(&[
        "id",
        t(lang, T::ColName),
        t(lang, T::ColType),
        t(lang, T::ColEnabled),
        t(lang, T::ColKeySet),
    ]);
    for e in entries {
        table.add_row(vec![
            Cell::new(&e.id),
            Cell::new(&e.name),
            Cell::new(kind_label(&e.kind)),
            Cell::new(if e.enabled {
                t(lang, T::Yes)
            } else {
                t(lang, T::No)
            }),
            Cell::new(if e.api_key_enc.is_some() {
                "✓"
            } else {
                "✗"
            }),
        ]);
    }
    table.to_string()
}

/// `quota natives` 表格：id / 名称 / 峰谷预置。
pub fn natives_table(metas: &[NativeMeta], lang: Lang) -> String {
    let mut table = new_table(&["id", t(lang, T::ColName), t(lang, T::ColPricing)]);
    for m in metas {
        table.add_row(vec![
            Cell::new(m.id),
            Cell::new(m.name),
            Cell::new(if quota_core::pricing::preset(m.id).is_some() {
                "✓"
            } else {
                "-"
            })
            .set_alignment(CellAlignment::Center),
        ]);
    }
    table.to_string()
}

// ---- pricing 渲染 ----------------------------------------------------------

/// `quota pricing show` 价格对照表：项目 / 高峰 / 空闲。
/// 档缺失或单值缺失显示 "-"；价格经 `format_price` 去尾零。
pub fn pricing_table(peak: Option<&PriceTier>, off_peak: Option<&PriceTier>, lang: Lang) -> String {
    fn cell(tier: Option<&PriceTier>, pick: fn(&PriceTier) -> Option<f64>) -> Cell {
        Cell::new(
            tier.and_then(pick)
                .map_or_else(|| "-".into(), quota_core::pricing::format_price),
        )
        .set_alignment(CellAlignment::Right)
    }
    let pick_hit = |t: &PriceTier| t.cache_hit_input;
    let pick_miss = |t: &PriceTier| t.cache_miss_input;
    let pick_out = |t: &PriceTier| t.output;
    let mut table = new_table(&[
        t(lang, T::ColPriceItem),
        t(lang, T::ColPeak),
        t(lang, T::ColOffPeak),
    ]);
    for (label, pick) in [
        (T::PriceCacheHit, pick_hit as fn(&PriceTier) -> Option<f64>),
        (
            T::PriceCacheMiss,
            pick_miss as fn(&PriceTier) -> Option<f64>,
        ),
        (T::PriceOutput, pick_out as fn(&PriceTier) -> Option<f64>),
    ] {
        table.add_row(vec![
            Cell::new(t(lang, label)),
            cell(peak, pick),
            cell(off_peak, pick),
        ]);
    }
    table.to_string()
}

/// 星期序号（Mon=0…Sun=6，聚合排序用）。
fn weekday_idx(d: Weekday) -> u8 {
    match d {
        Weekday::Mon => 0,
        Weekday::Tue => 1,
        Weekday::Wed => 2,
        Weekday::Thu => 3,
        Weekday::Fri => 4,
        Weekday::Sat => 5,
        Weekday::Sun => 6,
    }
}

/// 序号反取星期（聚合段端点用）。
fn weekday_from_idx(i: u8) -> Weekday {
    match i {
        0 => Weekday::Mon,
        1 => Weekday::Tue,
        2 => Weekday::Wed,
        3 => Weekday::Thu,
        4 => Weekday::Fri,
        5 => Weekday::Sat,
        _ => Weekday::Sun,
    }
}

/// 星期名（zh 周一… / en Mon…）。
fn weekday_name(lang: Lang, d: Weekday) -> &'static str {
    match lang {
        Lang::En => match d {
            Weekday::Mon => "Mon",
            Weekday::Tue => "Tue",
            Weekday::Wed => "Wed",
            Weekday::Thu => "Thu",
            Weekday::Fri => "Fri",
            Weekday::Sat => "Sat",
            Weekday::Sun => "Sun",
        },
        _ => match d {
            Weekday::Mon => "周一",
            Weekday::Tue => "周二",
            Weekday::Wed => "周三",
            Weekday::Thu => "周四",
            Weekday::Fri => "周五",
            Weekday::Sat => "周六",
            Weekday::Sun => "周日",
        },
    }
}

/// 单窗口的星期聚合描述：排序去重 → 连续段合并
/// （`周一至周五` / `Mon–Fri`；孤立日枚举 `周六、周日` / `Sat, Sun`）。
fn window_days_desc(lang: Lang, days: &[Weekday]) -> String {
    let mut idx: Vec<u8> = days.iter().map(|d| weekday_idx(*d)).collect();
    idx.sort_unstable();
    idx.dedup();
    let sep_day = match lang {
        Lang::En => ", ",
        _ => "、",
    };
    let mut parts = Vec::new();
    let mut i = 0;
    while i < idx.len() {
        let mut j = i;
        while j + 1 < idx.len() && idx[j + 1] == idx[j] + 1 {
            j += 1;
        }
        let start = weekday_from_idx(idx[i]);
        let end = weekday_from_idx(idx[j]);
        if i == j {
            parts.push(weekday_name(lang, start).to_string());
        } else {
            match lang {
                Lang::En => parts.push(format!(
                    "{}–{}",
                    weekday_name(lang, start),
                    weekday_name(lang, end)
                )),
                _ => parts.push(format!(
                    "{}至{}",
                    weekday_name(lang, start),
                    weekday_name(lang, end)
                )),
            }
        }
        i = j + 1;
    }
    parts.join(sep_day)
}

/// 窗口集合的人类可读描述：`周一至周五 09:00–12:00、14:00–18:00`
/// （窗口间 zh 顿号 / en 逗号；起止 en dash）。
pub fn windows_desc(windows: &[PeakWindow], lang: Lang) -> String {
    let sep_win = match lang {
        Lang::En => ", ",
        _ => "、",
    };
    windows
        .iter()
        .map(|w| format!("{} {}–{}", window_days_desc(lang, &w.days), w.start, w.end))
        .collect::<Vec<_>>()
        .join(sep_win)
}

/// UTC 偏移描述：Some(480) → `UTC+08:00`；None → 「本地时区」。
pub fn tz_desc(lang: Lang, timezone_offset_minutes: Option<i32>) -> String {
    match timezone_offset_minutes {
        None => t(lang, T::PricingLocalTz).into(),
        Some(m) => {
            let sign = if m < 0 { '-' } else { '+' };
            let m = m.abs();
            format!("UTC{sign}{:02}:{:02}", m / 60, m % 60)
        }
    }
}

/// 按峰谷判定时区格式化时刻（`08-19 12:00`；非法偏移回退本地）。
pub fn fmt_datetime_in_tz(ms: u64, timezone_offset_minutes: Option<i32>) -> String {
    let formatted = timezone_offset_minutes
        .and_then(|m| m.checked_mul(60))
        .and_then(chrono::FixedOffset::east_opt)
        .and_then(|tz| tz.timestamp_millis_opt(ms as i64).single())
        .map(|dt| dt.format("%m-%d %H:%M").to_string())
        .or_else(|| {
            chrono::Local
                .timestamp_millis_opt(ms as i64)
                .single()
                .map(|dt| dt.format("%m-%d %H:%M").to_string())
        });
    formatted.unwrap_or_else(|| chrono::Local::now().format("%m-%d %H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quota_core::PlanVariant;

    fn usage(remaining: f64) -> UsageData {
        UsageData {
            plan_name: Some("five_hour".into()),
            used: Some(40.0),
            remaining: Some(remaining),
            unit: Some("%".into()),
            ..UsageData::default()
        }
    }

    /// 倒计时断言的固定基准时刻（epoch 毫秒）。
    const NOW: i64 = 1_700_000_000_000;

    fn outcome_ok(rows: Vec<UsageData>) -> QueryOutcome {
        QueryOutcome {
            id: "e1".into(),
            name: "测试".into(),
            result: Ok(rows),
        }
    }

    /// 契约：成功行含全部列值，None 数值显示 "-"（两语言表头齐备）。
    #[test]
    fn query_table_renders_rows() {
        for lang in [Lang::Zh, Lang::En] {
            let table = query_table(&[outcome_ok(vec![usage(58.0)])], lang, NOW);
            assert!(table.contains("five_hour"), "{lang:?}: {table}");
            assert!(table.contains("58"), "{lang:?}: {table}");
            assert!(table.contains("OK"), "{lang:?}: {table}");
            assert!(table.contains(t(lang, T::ColName)), "{lang:?}: {table}");
            assert!(table.contains(t(lang, T::ColStatus)), "{lang:?}: {table}");
            // None 字段显示 -
            let table = query_table(&[outcome_ok(vec![UsageData::default()])], lang, NOW);
            assert!(table.contains('-'), "{lang:?}: {table}");
        }
    }

    /// 契约：多窗口条目多行、失败条目带分类前缀、失效条目透出 invalid_message（双语）。
    #[test]
    fn query_table_multi_window_and_errors() {
        let mut invalid = usage(1.0);
        invalid.is_valid = Some(false);
        invalid.invalid_message = Some("key 已过期".into());
        let outcomes = vec![
            outcome_ok(vec![usage(60.0), usage(120.0)]),
            QueryOutcome {
                id: "e2".into(),
                name: "坏条目".into(),
                result: Err(QueryError::transient("查询超时（15 秒）")),
            },
            QueryOutcome {
                id: "e3".into(),
                name: "失效条目".into(),
                result: Ok(vec![invalid]),
            },
        ];
        for (lang, kind_prefix, invalid_prefix) in [
            (Lang::Zh, "[瞬时] ", "失效："),
            (Lang::En, "[transient] ", "invalid: "),
        ] {
            let table = query_table(&outcomes, lang, NOW);
            assert_eq!(
                table.matches("测试").count(),
                2,
                "{lang:?} 多窗口应两行：{table}"
            );
            assert!(
                table.contains(&format!("{kind_prefix}查询超时")),
                "{lang:?}: {table}"
            );
            assert!(
                table.contains(&format!("{invalid_prefix}key 已过期")),
                "{lang:?}: {table}"
            );
        }
    }

    /// 契约：无数据行的「OK（无数据）」双语。
    #[test]
    fn query_table_no_data_row() {
        for (lang, needle) in [(Lang::Zh, "OK（无数据）"), (Lang::En, "OK (no data)")] {
            let table = query_table(&[outcome_ok(vec![])], lang, NOW);
            assert!(table.contains(needle), "{lang:?}: {table}");
        }
    }

    /// 契约：重置倒计时格式分档（分钟/时+分/天+时）与缺省/过期回退。
    #[test]
    fn reset_countdown_format_tiers() {
        let m = |mins: i64| NOW + mins * 60_000;
        // 无数据 → "-"
        assert_eq!(fmt_reset_countdown(None, NOW), "-");
        // 已过期 / 恰在当下 → "-"（翻转在即，无展示意义）
        assert_eq!(fmt_reset_countdown(Some(NOW - 1), NOW), "-");
        assert_eq!(fmt_reset_countdown(Some(NOW), NOW), "-");
        // 分钟档（< 1h）
        assert_eq!(fmt_reset_countdown(Some(m(21)), NOW), "21m");
        assert_eq!(fmt_reset_countdown(Some(m(59)), NOW), "59m");
        // 时+分档；整时省略分钟
        assert_eq!(fmt_reset_countdown(Some(m(201)), NOW), "3h21m");
        assert_eq!(fmt_reset_countdown(Some(m(180)), NOW), "3h");
        // 天+时档（周/月窗口）；整天省略小时；跨天后丢弃分钟粒度
        assert_eq!(fmt_reset_countdown(Some(m(24 * 60 + 5 * 60)), NOW), "1d5h");
        assert_eq!(fmt_reset_countdown(Some(m(4 * 24 * 60 + 17 * 60)), NOW), "4d17h");
        assert_eq!(fmt_reset_countdown(Some(m(4 * 24 * 60)), NOW), "4d");
        assert_eq!(fmt_reset_countdown(Some(m(24 * 60 + 17)), NOW), "1d");
    }

    /// 契约：query 表格含重置列表头，行内显示倒计时而非原始时间戳。
    #[test]
    fn query_table_renders_reset_column() {
        let mut with_reset = usage(58.0);
        with_reset.reset_at = Some(NOW + 201 * 60_000);
        for lang in [Lang::Zh, Lang::En] {
            let table = query_table(&[outcome_ok(vec![with_reset.clone()])], lang, NOW);
            assert!(table.contains(t(lang, T::ColReset)), "{lang:?}: {table}");
            assert!(table.contains("3h21m"), "{lang:?}: {table}");
        }
    }

    /// 契约：list 表格列与类型标签（两语言表头与是/否）。
    #[test]
    fn list_table_labels() {
        let entries = vec![ProviderEntry {
            id: "abc234".into(),
            name: "DeepSeek".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: Some("v1:xxx".into()),
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
        }];
        for lang in [Lang::Zh, Lang::En] {
            let table = list_table(&entries, lang);
            assert!(table.contains("native:deepseek"), "{lang:?}: {table}");
            assert!(table.contains("✓"), "{lang:?}: {table}");
            assert!(table.contains(t(lang, T::ColEnabled)), "{lang:?}: {table}");
            assert!(table.contains(t(lang, T::ColKeySet)), "{lang:?}: {table}");
            assert!(table.contains(t(lang, T::Yes)), "{lang:?}: {table}");
        }
        // 禁用条目显示 否/no
        let mut disabled = entries[0].clone();
        disabled.enabled = false;
        assert!(list_table(&[disabled.clone()], Lang::Zh).contains("否"));
        assert!(list_table(&[disabled], Lang::En).contains("no"));
    }

    /// 契约：natives 表头双语。
    #[test]
    fn natives_table_headers() {
        let metas = quota_core::provider::metas();
        for lang in [Lang::Zh, Lang::En] {
            let table = natives_table(&metas, lang);
            assert!(table.contains(t(lang, T::ColName)), "{lang:?}: {table}");
            assert!(table.contains("id"), "{lang:?}: {table}");
        }
    }

    /// 契约：--json 输出结构——成功与失败两态、kind 双值。
    #[test]
    fn query_json_shape() {
        let ok = outcome_ok(vec![usage(58.0)]).to_json();
        let j = serde_json::to_value(&ok).unwrap();
        assert_eq!(j["ok"], true);
        assert!(j["data"].is_array());
        assert!(j["error"].is_null());

        let err = QueryOutcome {
            id: "e2".into(),
            name: "x".into(),
            result: Err(QueryError::deterministic("HTTP 401")),
        }
        .to_json();
        let j = serde_json::to_value(&err).unwrap();
        assert_eq!(j["ok"], false);
        assert!(j["data"].is_null());
        assert_eq!(j["error"]["kind"], "deterministic");
        assert_eq!(j["error"]["message"], "HTTP 401");

        let transient = QueryOutcome {
            id: "e3".into(),
            name: "y".into(),
            result: Err(QueryError::transient("timeout")),
        }
        .to_json();
        assert_eq!(
            serde_json::to_value(&transient).unwrap()["error"]["kind"],
            "transient"
        );
    }

    /// 安全契约：JSON 输出不含任何 key 字段。
    #[test]
    fn json_output_has_no_key_field() {
        let ok = outcome_ok(vec![usage(1.0)]).to_json();
        let j = serde_json::to_string(&ok).unwrap();
        assert!(!j.to_lowercase().contains("key"), "{j}");
    }
}
