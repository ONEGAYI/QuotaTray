//! OpenCode Go 订阅用量查询。
//!
//! `GET https://opencode.ai/inference/go/v1/usage`（Bearer api key）
//! 响应：
//! ```text
//! {"usage":{
//!   "rolling":{"status":"ok","percent":18.5,"resetsAt":"2026-10-21T18:00:00.000Z"},
//!   "weekly":{...},
//!   "monthly":{...}
//! }}
//! ```
//! `percent` 已是**已用百分比**（0–100），与 [`crate::model::UsageData`]
//! 的统一口径一致，无需换算。三个窗口分别对应 Go 文档规定的
//! 5 小时（月额度 20%）、每周（50%）、每月（100%）档位。
//!
//! 端点为 Console 网关路由、无官方公开文档（官方 Go 文档只说明可在
//! Console 查看用量）；路径取自 Console 前端 bundle 与多个独立客户端
//! 实现的一致性交叉验证，官方可能变动。

use async_trait::async_trait;
use serde_json::Value;

use super::{
    NativeMeta, NativeProvider, parse_error, parse_num, parse_success_json, rfc3339_to_epoch_ms,
    status_error_with_body,
};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

pub struct OpenCodeGo;

/// 三个额度窗口：响应字段名、展示标签与文档口径的窗口长度。
/// 顺序即输出顺序（由短到长，与 Kimi Code / MiniMax 的行序一致）。
const WINDOWS: [(&str, &str); 3] = [("rolling", "5h"), ("weekly", "week"), ("monthly", "month")];

/// 单窗口 → 用量行。`percent` 缺失且窗口未标记受限时返回 None
/// （该窗口不下发），由调用方判断是否整份响应异常。
fn window_row(usage: &Value, field: &str, label: &str) -> Option<UsageData> {
    let window = usage.get(field)?;
    let status = window
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    let percent = window
        .get("percent")
        .and_then(|v| parse_num(Some(v)).map(|raw| raw.clamp(0.0, 100.0)));

    // status != "ok" 表示该窗口已被限流：即使远端没给百分比，也按用尽展示，
    // 避免「额度耗尽却显示 0%」的假空窗。
    let limited = status.as_deref().is_some_and(|s| s != "ok");
    let used = match (percent, limited) {
        (Some(p), _) => p,
        (None, true) => 100.0,
        (None, false) => return None,
    };

    Some(UsageData {
        plan_name: Some(format!("OpenCode Go（{label}）")),
        total: Some(100.0),
        used: Some(used),
        remaining: Some((100.0 - used).max(0.0)),
        unit: Some("%".into()),
        reset_at: window
            .get("resetsAt")
            .or_else(|| window.get("resets_at"))
            .and_then(Value::as_str)
            .and_then(rfc3339_to_epoch_ms),
        is_valid: None,
        invalid_message: None,
        extra: None,
    })
}

#[async_trait]
impl NativeProvider for OpenCodeGo {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "opencode_go",
            name: "OpenCode Go",
            console_url: Some("https://opencode.ai/console/go"),
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let request = HttpRequest::get("https://opencode.ai/inference/go/v1/usage")
            .bearer(&creds.api_key)
            .header("Accept", "application/json");

        // 401/403 是网关对凭据下的两类确定裁决，语义不同必须分开：
        // 401 → key 无效/过期；403 → key 有效但账号没有 Go 订阅
        //（选错 key 或未订阅）。合成一条会让用户无从纠正。
        let response = http
            .execute(request.clone())
            .await
            .map_err(|error| match &error {
                crate::http::HttpError::Timeout | crate::http::HttpError::Network(_) => {
                    QueryError::transient(error.to_string())
                }
                crate::http::HttpError::InvalidRequest(_) => {
                    QueryError::deterministic(error.to_string())
                }
            })?;
        if response.status == 401 {
            return Err(QueryError::deterministic(
                "OpenCode Go API Key 无效或已过期，请在 Console 重新创建后更新",
            ));
        }
        if response.status == 403 {
            return Err(QueryError::deterministic(
                "该 API Key 有效，但所属账号未订阅 OpenCode Go（Go / Go Plus），请确认使用的是订阅账号的 Key",
            ));
        }
        if !response.is_success() {
            return Err(status_error_with_body(
                response.status,
                &response.body,
                &request,
            ));
        }

        let body: Value = parse_success_json(&request, &response)?;
        let usage = body
            .get("usage")
            .ok_or_else(|| parse_error("OpenCode Go", "usage 对象"))?;
        let rows: Vec<UsageData> = WINDOWS
            .iter()
            .filter_map(|(field, label)| window_row(usage, field, label))
            .collect();
        if rows.is_empty() {
            return Err(parse_error(
                "OpenCode Go",
                "usage.rolling/weekly/monthly 的 status 或 percent",
            ));
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;
    use crate::provider::testing::MockHttp;

    fn creds() -> Credentials {
        Credentials::new("oc_sk_test_key")
    }

    fn header<'a>(req: &'a HttpRequest, name: &str) -> Option<&'a str> {
        req.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    const OK_BODY: &str = r#"{
        "usage": {
            "rolling": {"status": "ok", "percent": 18.5, "resetsAt": "2026-10-21T18:00:00.000Z"},
            "weekly":  {"status": "ok", "percent": 62,   "resetsAt": "2026-10-26T00:00:00.000Z"},
            "monthly": {"status": "ok", "percent": 40.25,"resetsAt": "2026-11-01T00:00:00.000Z"}
        }
    }"#;

    /// 2026-10-22 真实 Go 订阅账户的实测响应（仅脱敏 key，其余原样）。
    /// `percent` 实测为整数而非小数，与 `parse_num` 的 f64 口径需保持一致。
    const REAL_BODY: &str = r#"{"usage":{
        "rolling":{"status":"ok","percent":2,"resetsAt":"2026-10-21T22:03:17.000Z"},
        "weekly":{"status":"ok","percent":1,"resetsAt":"2026-10-26T00:00:00.000Z"},
        "monthly":{"status":"ok","percent":0,"resetsAt":"2026-11-21T16:55:41.000Z"}
    }}"#;

    /// 线协议：GET + Bearer + JSON Accept，指向网关 Go 用量路由。
    #[tokio::test]
    async fn sends_bearer_get_to_usage_route() {
        let mock = MockHttp::ok(OK_BODY);
        OpenCodeGo
            .query(&creds(), &mock, PlanVariant::Auto)
            .await
            .unwrap();

        let requests = mock.captured_requests();
        let req = &requests[0];
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.url, "https://opencode.ai/inference/go/v1/usage");
        assert_eq!(header(req, "Authorization"), Some("Bearer oc_sk_test_key"));
        assert_eq!(header(req, "Accept"), Some("application/json"));
    }

    /// 三窗口按 5h → week → month 顺序输出，百分比原样透传（已是已用口径）。
    #[tokio::test]
    async fn parses_three_windows_in_fixed_order() {
        let rows = OpenCodeGo
            .query(&creds(), &MockHttp::ok(OK_BODY), PlanVariant::Auto)
            .await
            .unwrap();

        let labels: Vec<&str> = rows
            .iter()
            .map(|row| row.plan_name.as_deref().unwrap())
            .collect();
        assert_eq!(
            labels,
            vec![
                "OpenCode Go（5h）",
                "OpenCode Go（week）",
                "OpenCode Go（month）"
            ]
        );
        assert_eq!(rows[0].used, Some(18.5));
        assert_eq!(rows[0].remaining, Some(81.5));
        assert_eq!(rows[1].used, Some(62.0));
        assert_eq!(rows[2].used, Some(40.25));
        assert!(rows.iter().all(|row| row.total == Some(100.0)));
        assert!(rows.iter().all(|row| row.unit.as_deref() == Some("%")));
        assert!(rows.iter().all(|row| row.is_valid.is_none()));
    }

    /// 实测响应锚定：整数 percent 正确映射为已用口径，三窗口齐全。
    #[tokio::test]
    async fn parses_real_captured_response() {
        let rows = OpenCodeGo
            .query(&creds(), &MockHttp::ok(REAL_BODY), PlanVariant::Auto)
            .await
            .unwrap();

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].used, Some(2.0));
        assert_eq!(rows[0].remaining, Some(98.0));
        assert_eq!(rows[1].used, Some(1.0));
        assert_eq!(rows[1].remaining, Some(99.0));
        assert_eq!(rows[2].used, Some(0.0));
        assert_eq!(rows[2].remaining, Some(100.0));
        assert!(rows.iter().all(|row| row.reset_at.is_some()));
    }

    /// `resetsAt` RFC3339 → epoch 毫秒；非法时间戳不伪造（保持 None）。
    #[tokio::test]
    async fn parses_reset_at_and_ignores_malformed() {
        let body = r#"{"usage":{
            "rolling":{"status":"ok","percent":1,"resetsAt":"2026-10-21T18:00:00.000Z"},
            "weekly":{"status":"ok","percent":2,"resetsAt":"not-a-time"}
        }}"#;
        let rows = OpenCodeGo
            .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
            .await
            .unwrap();

        assert_eq!(rows[0].reset_at, Some(1_792_605_600_000));
        assert_eq!(rows[1].reset_at, None);
    }

    /// 窗口被限流（status != ok）时即使无 percent 也按 100% 用尽展示，
    /// 不能呈现「已耗尽却 0% 已用」。
    #[tokio::test]
    async fn rate_limited_window_without_percent_reads_exhausted() {
        let body = r#"{"usage":{
            "rolling":{"status":"rate-limited","resetsAt":"2026-10-21T18:00:00.000Z"},
            "weekly":{"status":"ok","percent":5}
        }}"#;
        let rows = OpenCodeGo
            .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
            .await
            .unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].used, Some(100.0));
        assert_eq!(rows[0].remaining, Some(0.0));
        assert_eq!(rows[1].used, Some(5.0));
    }

    /// 远端异常百分比钳到 [0,100]，防止 remaining 为负或超出 total。
    #[tokio::test]
    async fn clamps_out_of_range_percent() {
        let body = r#"{"usage":{
            "rolling":{"status":"ok","percent":150},
            "weekly":{"status":"ok","percent":-20}
        }}"#;
        let rows = OpenCodeGo
            .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
            .await
            .unwrap();

        assert_eq!(rows[0].used, Some(100.0));
        assert_eq!(rows[0].remaining, Some(0.0));
        assert_eq!(rows[1].used, Some(0.0));
        assert_eq!(rows[1].remaining, Some(100.0));
    }

    /// 401（key 无效）与 403（未订阅）文案必须区分，且均为确定性失败。
    #[tokio::test]
    async fn distinguishes_unauthorized_from_missing_subscription() {
        let unauthorized = OpenCodeGo
            .query(
                &creds(),
                &MockHttp::status_body(
                    401,
                    r#"{"type":"error","error":{"type":"AuthError","message":"Unauthorized"}}"#,
                ),
                PlanVariant::Auto,
            )
            .await
            .unwrap_err();
        assert!(!unauthorized.is_transient());
        assert!(unauthorized.message().contains("无效或已过期"));

        let forbidden = OpenCodeGo
            .query(
                &creds(),
                &MockHttp::status_body(
                    403,
                    r#"{"type":"error","error":{"type":"EntitlementError","message":"Forbidden"}}"#,
                ),
                PlanVariant::Auto,
            )
            .await
            .unwrap_err();
        assert!(!forbidden.is_transient());
        assert!(forbidden.message().contains("未订阅"));
    }

    /// 限流与 5xx 仍是可重试的瞬时失败（沿用统一状态码语义）。
    #[tokio::test]
    async fn rate_limit_and_server_errors_are_transient() {
        for status in [429u16, 503] {
            let err = OpenCodeGo
                .query(&creds(), &MockHttp::status(status), PlanVariant::Auto)
                .await
                .unwrap_err();
            assert!(err.is_transient(), "HTTP {status} 应为瞬时失败");
        }
    }

    /// 结构缺失 → 确定性失败，不静默返回空列表。
    #[tokio::test]
    async fn missing_usage_object_is_deterministic() {
        let err = OpenCodeGo
            .query(&creds(), &MockHttp::ok(r#"{"data":{}}"#), PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("usage"));
    }

    /// 三窗口全无可用百分比 → 确定性失败（整份响应异常）。
    #[tokio::test]
    async fn empty_windows_are_deterministic() {
        let err = OpenCodeGo
            .query(
                &creds(),
                &MockHttp::ok(r#"{"usage":{"rolling":{},"weekly":{},"monthly":{}}}"#),
                PlanVariant::Auto,
            )
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }
}
