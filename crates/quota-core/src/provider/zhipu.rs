//! 智谱 BigModel / Z.ai 的 GLM Coding Plan 套餐用量查询（国内/国际双站）。
//!
//! `GET {base}/api/monitor/usage/quota/limit`（**裸 api key，无 Bearer 前缀**）
//! 响应：`{"data": {"limits": [{"type": "TOKENS_LIMIT", "unit": 3,
//! "percentage": 42.5, ...}]}}`，`percentage` 已是已用百分比（0-100）。
//! 每个 TOKENS_LIMIT 窗口一条 `UsageData`，按 `unit` 归类（5 小时/每周）。
//!
//! 窗口归类依据 cc-switch 实测（本项目调研报告 §4.2 + coding_plan.rs）：
//! - `unit: 3` → 5 小时滚动窗口；`unit: 6` → 每周窗口（number 取值有两种，
//!   只锚定 unit）；老套餐只回 1 条（自然只有 5h 行），新套餐回 2 条；
//! - 不能按 `nextResetTime` 排序代替分类——周期末尾每周窗口会比 5 小时
//!   窗口更早重置，时间排序必然把两桶标反（cc-switch issue #3036）。
//!
//! 端点为社区逆向所得、无官方文档（cc-switch 同款，本项目调研报告 §4.2
//! 收录），官方可能变动。仅支持个人版 Coding Plan key；团队版需额外
//! `bigmodel-organization`/`bigmodel-project` 头（已知限制，暂不支持）。

use async_trait::async_trait;
use serde_json::Value;

use super::{NativeMeta, NativeProvider, fetch_json, parse_error, parse_num};
use crate::config::Credentials;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 双站共享的实现，`id`/`base_url` 随站点实例化。
pub struct ZhipuApi {
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
}

/// 智谱国内站（open.bigmodel.cn）。
pub const ZHIPU: ZhipuApi = ZhipuApi {
    id: "zhipu",
    name: "智谱 GLM Coding Plan",
    base_url: "https://open.bigmodel.cn",
};

/// Z.ai 国际站。
pub const ZAI: ZhipuApi = ZhipuApi {
    id: "zai",
    name: "Z.ai GLM Coding Plan",
    base_url: "https://api.z.ai",
};

/// TOKENS_LIMIT 条目的窗口归类（决定 plan_name 的窗口标签）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZhipuWindow {
    FiveHour,
    Weekly,
}

/// TOKENS_LIMIT 窗口标签（语言中性缩写，双语用户均可读）。
fn window_label(window: ZhipuWindow) -> &'static str {
    match window {
        ZhipuWindow::FiveHour => "5h",
        ZhipuWindow::Weekly => "week",
    }
}

/// 按 `unit` 字段判定 TOKENS_LIMIT 条目所属窗口。
///
/// 实测形态（bigmodel.cn 与 z.ai 共用同一后端，字段一致）：
/// - `unit: 3` → 5 小时滚动窗口（老/新套餐均有）；
/// - `unit: 6` → 每周窗口（`number: 7` 与 `number: 1` 两种取值都被实测
///   过，故只锚定 `unit`、不绑 `number`）；
/// - `unit` 缺失或值不认识 → None，由调用方走重置时间启发式兜底。
fn classify_window(item: &Value) -> Option<ZhipuWindow> {
    match item.get("unit").and_then(Value::as_i64) {
        Some(3) => Some(ZhipuWindow::FiveHour),
        Some(6) => Some(ZhipuWindow::Weekly),
        _ => None,
    }
}

/// 把 `data.limits` 解析为窗口行（5h 在前、周在后，与 cc-switch 同序）。
///
/// 归类优先级：`unit` 显式字段（3 → 5h、6 → 周）。兜底（`unit` 缺失或
/// 不识别）：仅在 5h 槽空缺时归入（覆盖老套餐单条、无 unit 的历史
/// 形态）；**week 槽只认 unit=6 的明确标注**——无法识别的条目（如
/// v1 套餐的 MCP 工具调用限额）宁可丢弃也不错标成周限额。
/// 非 TOKENS_LIMIT 条目忽略；`used` 钳到 [0,100] 防远端异常值把
/// remaining 顶出 total。
fn parse_windows(data: &Value) -> Vec<UsageData> {
    let mut five_hour: Option<(f64, &Value)> = None;
    let mut weekly: Option<(f64, &Value)> = None;
    let mut unclassified: Vec<(f64, Option<i64>, &Value)> = Vec::new();

    if let Some(limits) = data.get("limits").and_then(Value::as_array) {
        for item in limits {
            let limit_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            // 大小写不敏感：上游若改变大小写仍能识别
            if !limit_type.eq_ignore_ascii_case("TOKENS_LIMIT") {
                continue;
            }
            let Some(raw) = parse_num(item.get("percentage")) else {
                continue;
            };
            let used = raw.clamp(0.0, 100.0);
            let reset = item.get("nextResetTime").and_then(Value::as_i64);
            match classify_window(item) {
                Some(ZhipuWindow::FiveHour) if five_hour.is_none() => {
                    five_hour = Some((used, item))
                }
                Some(ZhipuWindow::Weekly) if weekly.is_none() => weekly = Some((used, item)),
                _ => unclassified.push((used, reset, item)),
            }
        }
    }

    // 兜底排序：无重置时间在前（5h 桶在 0% 等状态下可能没有重置时间），
    // 仅用于挑出填入 5h 空槽的第一条
    unclassified.sort_by_key(|(_, reset, _)| (reset.is_some(), reset.unwrap_or(i64::MIN)));
    for (used, _, item) in unclassified {
        if five_hour.is_none() {
            five_hour = Some((used, item));
        }
    }

    [(ZhipuWindow::FiveHour, five_hour), (ZhipuWindow::Weekly, weekly)]
        .into_iter()
        .filter_map(|(window, slot)| {
            let (used, item) = slot?;
            Some(UsageData {
                plan_name: Some(format!("GLM Coding Plan（{}）", window_label(window))),
                total: Some(100.0),
                used: Some(used),
                remaining: Some(100.0 - used),
                unit: Some("%".into()),
                is_valid: None,
                invalid_message: None,
                extra: Some(item.clone()),
            })
        })
        .collect()
}

#[async_trait]
impl NativeProvider for ZhipuApi {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: self.id,
            name: self.name,
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = HttpRequest::get(format!("{}/api/monitor/usage/quota/limit", self.base_url))
            // 智谱侧约定：Authorization 直接放裸 key，无 Bearer 前缀
            .header("Authorization", creds.api_key.as_str());
        let body = fetch_json(http, req).await?;

        let data = body
            .get("data")
            .ok_or_else(|| parse_error(self.name, "data.limits 数组"))?;

        // 空数组/无一项可解析都视为结构异常
        let rows = parse_windows(data);
        if rows.is_empty() {
            return Err(parse_error(self.name, "limits[].percentage 数值"));
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    fn creds() -> Credentials {
        Credentials::new("raw-key-no-bearer")
    }

    fn auth_of(req: &crate::http::HttpRequest) -> &str {
        req.headers
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.as_str())
            .unwrap()
    }

    /// 正常响应（新套餐两条）：unit 3 → 5h 行在前、unit 6 → week 行在后；
    /// used 钳制、remaining 互补、原始 item 透传 extra。
    #[tokio::test]
    async fn parses_percentage_windows_by_unit() {
        let body = r#"{"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":42.5,"nextResetTime":1755000000000},
            {"type":"TOKENS_LIMIT","unit":6,"number":7,"percentage":7.0,"nextResetTime":1755500000000}
        ]}}"#;
        let data = ZHIPU.query(&creds(), &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].used, Some(42.5));
        assert_eq!(data[0].plan_name.as_deref(), Some("GLM Coding Plan（5h）"));
        assert_eq!(data[1].plan_name.as_deref(), Some("GLM Coding Plan（week）"));
        assert_eq!(data[1].remaining, Some(93.0));
        assert_eq!(
            data[0].extra.as_ref().unwrap()["unit"],
            serde_json::json!(3),
            "原始窗口项透传 extra"
        );
    }

    /// 老套餐只回一条（unit 3）：单行 5h，不造 week 行。
    #[tokio::test]
    async fn legacy_single_limit_yields_only_five_hour_row() {
        let body = r#"{"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3,"percentage":31.0}]}}"#;
        let data = ZHIPU.query(&creds(), &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].plan_name.as_deref(), Some("GLM Coding Plan（5h）"));
    }

    /// 非 TOKENS_LIMIT 条目忽略；TOKENS_LIMIT 大小写不敏感。
    #[tokio::test]
    async fn non_tokens_limit_items_are_skipped() {
        let body = r#"{"data":{"limits":[
            {"type":"CONCURRENCY_LIMIT","percentage":99.0},
            {"type":"tokens_limit","unit":3,"percentage":10.0}
        ]}}"#;
        let data = ZHIPU.query(&creds(), &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data.len(), 1, "只应保留 TOKENS_LIMIT 条目");
        assert_eq!(data[0].used, Some(10.0));
    }

    /// unit 缺失的兜底：仅在 5h 槽空缺时归入（无 reset 优先）；week 槽
    /// 只认 unit=6 的明确标注，无法识别的条目丢弃，不错标成周限额。
    #[tokio::test]
    async fn unclassified_items_fill_five_hour_slot_only() {
        // 无 reset 的未知条目填 5h；带 reset 的未知条目丢弃（不造 week 行）
        let body = r#"{"data":{"limits":[
            {"type":"TOKENS_LIMIT","percentage":9.0,"nextResetTime":1755500000000},
            {"type":"TOKENS_LIMIT","percentage":31.0}
        ]}}"#;
        let data = ZHIPU.query(&creds(), &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].plan_name.as_deref(), Some("GLM Coding Plan（5h）"));
        assert_eq!(data[0].used, Some(31.0), "无 reset 的未知条目填 5h");

        // 两条都带 reset：升序第一条填 5h，另一条丢弃
        let body = r#"{"data":{"limits":[
            {"type":"TOKENS_LIMIT","percentage":9.0,"nextResetTime":1755500000000},
            {"type":"TOKENS_LIMIT","percentage":31.0,"nextResetTime":1755000000000}
        ]}}"#;
        let data = ZHIPU.query(&creds(), &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].used, Some(31.0), "更早重置的填 5h");

        // unit=3 已占 5h 槽后，未知条目（如 v1 的 MCP 限额若同为
        // TOKENS_LIMIT）不再入列——宁缺毋错
        let body = r#"{"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":3,"percentage":9.0},
            {"type":"TOKENS_LIMIT","unit":42,"percentage":31.0}
        ]}}"#;
        let data = ZHIPU.query(&creds(), &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].used, Some(9.0));
        assert_eq!(data[0].plan_name.as_deref(), Some("GLM Coding Plan（5h）"));
    }

    /// 边界契约：used 钳到 [0,100]——超界值不得把 remaining 顶出 total。
    #[tokio::test]
    async fn percentage_is_clamped_to_unit_range() {
        let body = r#"{"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":3,"percentage":120.0},
            {"type":"TOKENS_LIMIT","unit":6,"percentage":-5.0}
        ]}}"#;
        let data = ZHIPU.query(&creds(), &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].used, Some(100.0));
        assert_eq!(data[0].remaining, Some(0.0));
        assert_eq!(data[1].used, Some(0.0));
        assert_eq!(data[1].remaining, Some(100.0));
    }

    /// 错误分类：非 JSON 响应与网络故障均为确定性/瞬时（复用共用
    /// fetch_json 分类，此处锁端到端行为）。
    #[tokio::test]
    async fn error_classification() {
        let err = ZHIPU
            .query(&creds(), &MockHttp::ok("<html>Not Found</html>"))
            .await
            .unwrap_err();
        assert!(!err.is_transient(), "非 JSON 应为确定性");

        let err = ZHIPU.query(&creds(), &MockHttp::fail()).await.unwrap_err();
        assert!(err.is_transient(), "网络故障应为瞬时");
    }

    /// 请求头契约：Authorization 为裸 key（无 Bearer 前缀），域名按站点。
    #[tokio::test]
    async fn raw_key_header_and_site_domains() {
        let mock = MockHttp::ok(r#"{"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3,"percentage":1.0}]}}"#);
        ZHIPU.query(&creds(), &mock).await.unwrap();
        let req = &mock.captured_requests()[0];
        assert_eq!(
            req.url,
            "https://open.bigmodel.cn/api/monitor/usage/quota/limit"
        );
        assert_eq!(auth_of(req), "raw-key-no-bearer", "智谱约定为裸 key");

        let mock = MockHttp::ok(r#"{"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3,"percentage":1.0}]}}"#);
        ZAI.query(&creds(), &mock).await.unwrap();
        assert_eq!(
            mock.captured_requests()[0].url,
            "https://api.z.ai/api/monitor/usage/quota/limit"
        );
    }

    /// limits 数组存在但无一项含数值 percentage → 确定性失败。
    #[tokio::test]
    async fn empty_or_numericless_limits_is_deterministic() {
        for body in [
            r#"{"data":{"limits":[]}}"#,
            r#"{"data":{"limits":[{"name":"5h"}]}}"#,
        ] {
            let err = ZHIPU
                .query(&creds(), &MockHttp::ok(body))
                .await
                .unwrap_err();
            assert!(!err.is_transient(), "body: {body}");
        }
    }

    /// 结构缺失（data/limits 不存在）→ 确定性失败。
    #[tokio::test]
    async fn missing_structure_is_deterministic() {
        let err = ZHIPU
            .query(&creds(), &MockHttp::ok(r#"{"data":{}}"#))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }
}
