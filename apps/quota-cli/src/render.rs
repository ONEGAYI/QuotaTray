//! 输出渲染：comfy-table UTF-8 边框表格与 `--json` 输出结构。
//!
//! 渲染函数均为纯函数（`&[T] → String`），输出字符串由单元测试锁定。

use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, presets::UTF8_FULL};
use quota_core::model::{QueryError, UsageData};
use quota_core::provider::NativeMeta;
use quota_core::{ProviderEntry, ProviderKind};
use serde::Serialize;

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
pub fn query_table(outcomes: &[QueryOutcome]) -> String {
    let mut t = new_table(&["名称", "套餐", "已用", "剩余", "单位", "状态"]);
    for o in outcomes {
        match &o.result {
            Ok(rows) if rows.is_empty() => {
                t.add_row(row(&o.name, &UsageData::default(), "OK（无数据）"));
            }
            Ok(rows) => {
                for d in rows {
                    let status = match d.is_valid {
                        Some(false) => {
                            format!("失效：{}", d.invalid_message.clone().unwrap_or_default())
                        }
                        _ => "OK".to_string(),
                    };
                    t.add_row(row(&o.name, d, &status));
                }
            }
            Err(e) => {
                let kind = if e.is_transient() {
                    "瞬时"
                } else {
                    "确定性"
                };
                t.add_row(row(
                    &o.name,
                    &UsageData::default(),
                    &format!("[{kind}] {}", e.message()),
                ));
            }
        }
    }
    t.to_string()
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
pub fn list_table(entries: &[ProviderEntry]) -> String {
    let mut t = new_table(&["id", "名称", "类型", "启用", "凭据已配"]);
    for e in entries {
        t.add_row(vec![
            Cell::new(&e.id),
            Cell::new(&e.name),
            Cell::new(kind_label(&e.kind)),
            Cell::new(if e.enabled { "是" } else { "否" }),
            Cell::new(if e.api_key_enc.is_some() {
                "✓"
            } else {
                "✗"
            }),
        ]);
    }
    t.to_string()
}

/// `quota natives` 表格：id / 名称。
pub fn natives_table(metas: &[NativeMeta]) -> String {
    let mut t = new_table(&["id", "名称"]);
    for m in metas {
        t.add_row(vec![Cell::new(m.id), Cell::new(m.name)]);
    }
    t.to_string()
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

    /// 契约：成功行含全部列值，None 数值显示 "-"。
    #[test]
    fn query_table_renders_rows() {
        let table = query_table(&[outcome_ok(vec![usage(58.0)])]);
        assert!(table.contains("five_hour"), "{table}");
        assert!(table.contains("58"), "{table}");
        assert!(table.contains("OK"), "{table}");
        // None 字段显示 -
        let table = query_table(&[outcome_ok(vec![UsageData::default()])]);
        assert!(table.contains('-'), "{table}");
    }

    /// 契约：多窗口条目多行、失败条目带分类前缀、失效条目透出 invalid_message。
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
        let table = query_table(&outcomes);
        assert_eq!(table.matches("测试").count(), 2, "多窗口应两行：{table}");
        assert!(table.contains("[瞬时] 查询超时"), "{table}");
        assert!(table.contains("失效：key 已过期"), "{table}");
    }

    /// 契约：list 表格列与类型标签。
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
        let table = list_table(&entries);
        assert!(table.contains("native:deepseek"), "{table}");
        assert!(table.contains("✓"), "{table}");
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
