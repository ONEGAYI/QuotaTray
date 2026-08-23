//! 输出渲染：comfy-table UTF-8 边框表格与 `--json` 输出结构。
//!
//! 渲染函数均为纯函数（`&[T] → String`），语言经参数传入，
//! 输出字符串由单元测试按双语锁定。

use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, presets::UTF8_FULL};
use quota_core::model::{QueryError, UsageData};
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

fn new_table(header: &[&str]) -> Table {
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

/// `quota query` 表格：名称 / 套餐 / 已用 / 剩余 / 单位 / 状态。
/// 多窗口条目每窗口一行；条目失败占一行，状态列带错误分类前缀。
pub fn query_table(outcomes: &[QueryOutcome], lang: Lang) -> String {
    let mut table = new_table(&[
        t(lang, T::ColName),
        t(lang, T::ColPlan),
        t(lang, T::ColUsed),
        t(lang, T::ColRemaining),
        t(lang, T::ColUnit),
        t(lang, T::ColStatus),
    ]);
    for o in outcomes {
        match &o.result {
            Ok(rows) if rows.is_empty() => {
                table.add_row(row(&o.name, &UsageData::default(), t(lang, T::OkNoData)));
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
                    table.add_row(row(&o.name, d, &status));
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
                ));
            }
        }
    }
    table.to_string()
}

/// 一行数据：数值列右对齐。
fn row(name: &str, d: &UsageData, status: &str) -> Vec<Cell> {
    vec![
        Cell::new(name),
        Cell::new(d.plan_name.clone().unwrap_or_else(|| "-".into())),
        Cell::new(fmt_num(d.used)).set_alignment(CellAlignment::Right),
        Cell::new(fmt_num(d.remaining)).set_alignment(CellAlignment::Right),
        Cell::new(d.unit.clone().unwrap_or_else(|| "-".into())),
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

/// `quota natives` 表格：id / 名称。
pub fn natives_table(metas: &[NativeMeta], lang: Lang) -> String {
    let mut table = new_table(&["id", t(lang, T::ColName)]);
    for m in metas {
        table.add_row(vec![Cell::new(m.id), Cell::new(m.name)]);
    }
    table.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(remaining: f64) -> UsageData {
        UsageData {
            plan_name: Some("five_hour".into()),
            used: Some(40.0),
            remaining: Some(remaining),
            unit: Some("%".into()),
            ..UsageData::default()
        }
    }

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
            let table = query_table(&[outcome_ok(vec![usage(58.0)])], lang);
            assert!(table.contains("five_hour"), "{lang:?}: {table}");
            assert!(table.contains("58"), "{lang:?}: {table}");
            assert!(table.contains("OK"), "{lang:?}: {table}");
            assert!(table.contains(t(lang, T::ColName)), "{lang:?}: {table}");
            assert!(table.contains(t(lang, T::ColStatus)), "{lang:?}: {table}");
            // None 字段显示 -
            let table = query_table(&[outcome_ok(vec![UsageData::default()])], lang);
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
            let table = query_table(&outcomes, lang);
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
            let table = query_table(&[outcome_ok(vec![])], lang);
            assert!(table.contains(needle), "{lang:?}: {table}");
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
