//! OpenCode 用量查询（当前覆盖 Go / Go Plus 订阅套餐）。
//!
//! 平台标识取产品线「OpenCode」而非套餐名「Go」：Zen 按量账户与 Go 订阅
//! 共用同一 Console 与密钥体系，后续接入 Zen 余额时并入本平台，不再新增
//! 注册项。当前只实现 Go 订阅的窗口用量路由。
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

/// OpenCode 品牌下的套餐条目（`id`/`name`/控制台 URL 随套餐实例化）。
///
/// 注册项按**套餐**命名（`opencode_go`），UI 的「OpenCode」品牌聚合由前端
/// 分组完成：分组键 `opencode` 下并列 Go 与（后续）Zen，与 SiliconFlow
/// 双站的分组模式一致。
pub struct OpenCode {
    id: &'static str,
    name: &'static str,
    /// 控制台直达预置 URL（套餐级：Go 有独立产品页）。
    console_url: &'static str,
}

/// OpenCode Go / Go Plus 订阅（$10 / $40 每月）。
pub const OPENCODE_GO: OpenCode = OpenCode {
    id: "opencode_go",
    name: "OpenCode Go",
    console_url: "https://opencode.ai/console/go",
};

/// 三个额度窗口：响应字段名、展示标签与文档口径的窗口长度。
/// 顺序即输出顺序（由短到长，与 Kimi Code / MiniMax 的行序一致）。
const WINDOWS: [(&str, &str); 3] = [("rolling", "5h"), ("weekly", "week"), ("monthly", "month")];

/// 单窗口 → 用量行。`percent` 缺失且窗口未标记受限时返回 None
/// （该窗口不下发），由调用方判断是否整份响应异常。
fn window_row(usage: &Value, name: &str, field: &str, label: &str) -> Option<UsageData> {
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
        plan_name: Some(format!("{name}（{label}）")),
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
impl NativeProvider for OpenCode {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: self.id,
            name: self.name,
            console_url: Some(self.console_url),
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
        // 401 → key 无效/过期；403 → key 有效但无 Go 订阅权限
        //（选错 key 或未订阅）。合成一条会让用户无从纠正。
        //
        // 文案是面向用户的引导，detail 保留网关原文（脱敏后）——端点无
        // 公开文档，403 究竟是订阅缺失还是区域/WAF 拦截无法先验确定，
        // 只留文案会让误诊无从求证。
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
        if response.status == 401 || response.status == 403 {
            let message = if response.status == 401 {
                "OpenCode API Key 无效或已过期，请在 Console 重新创建后更新"
            } else {
                "该 API Key 有效但无 OpenCode Go 订阅权限（Go / Go Plus）：请确认使用订阅账号的 Key；若已订阅，也可能被区域策略或风控拦截"
            };
            return Err(QueryError::deterministic(message).with_detail(format!(
                "HTTP {} 响应体（已脱敏）：\n{}",
                response.status,
                crate::http::redact::redact_and_truncate(&response.body, &request)
            )));
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
            .ok_or_else(|| parse_error(self.name, "usage 对象"))?;
        let rows: Vec<UsageData> = WINDOWS
            .iter()
            .filter_map(|(field, label)| window_row(usage, self.name, field, label))
            .collect();
        if rows.is_empty() {
            return Err(parse_error(
                self.name,
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
        OPENCODE_GO
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
        let rows = OPENCODE_GO
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
    /// 实测报文快照锚定：证明真实响应（整数 percent、毫秒精度 resetsAt）
    /// 能完整解析出三窗口。行为细节由上面的合成用例覆盖，这里只守
    /// 「远端报文形状变化」这一信号，不重复逐字段断言。
    #[tokio::test]
    async fn parses_real_captured_response() {
        let rows = OPENCODE_GO
            .query(&creds(), &MockHttp::ok(REAL_BODY), PlanVariant::Auto)
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
        assert!(rows.iter().all(|row| row.reset_at.is_some()));
        // 实测 account 用量极低，已用值应接近零而非被误读成 100
        assert!(rows.iter().all(|row| row.used.is_some_and(|u| u < 10.0)));
    }

    /// `resetsAt` RFC3339 → epoch 毫秒；非法时间戳不伪造（保持 None）。
    #[tokio::test]
    async fn parses_reset_at_and_ignores_malformed() {
        let body = r#"{"usage":{
            "rolling":{"status":"ok","percent":1,"resetsAt":"2026-10-21T18:00:00.000Z"},
            "weekly":{"status":"ok","percent":2,"resetsAt":"not-a-time"}
        }}"#;
        let rows = OPENCODE_GO
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
        let rows = OPENCODE_GO
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
        let rows = OPENCODE_GO
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
        let unauthorized = OPENCODE_GO
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

        let forbidden = OPENCODE_GO
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
        assert!(forbidden.message().contains("订阅权限"));
    }

    /// 401/403 的固定文案面向用户，但网关的原始说明必须留在 detail 里：
    /// 端点无公开文档，403 究竟因订阅缺失还是区域/WAF 拦截无法先验确定，
    /// 排查时需要有据可查。
    ///
    /// 响应体刻意回显 key（网关在错误说明里回显凭据是本项目已记录的
    /// 真实形态）：detail 未过脱敏时本用例必须失败，否则「不回显凭据」
    /// 只是注释口号而非被锁定的行为。
    #[tokio::test]
    async fn auth_failures_keep_redacted_detail() {
        for (status, body, marker, error_type) in [
            (
                401u16,
                r#"{"type":"error","error":{"type":"AuthError","message":"Unauthorized for oc_sk_test_key"}}"#,
                "无效或已过期",
                "AuthError",
            ),
            (
                403,
                r#"{"type":"error","error":{"type":"EntitlementError","message":"Forbidden for oc_sk_test_key"}}"#,
                "订阅权限",
                "EntitlementError",
            ),
        ] {
            let err = OPENCODE_GO
                .query(
                    &creds(),
                    &MockHttp::status_body(status, body),
                    PlanVariant::Auto,
                )
                .await
                .unwrap_err();
            assert!(err.message().contains(marker), "HTTP {status}: {err}");
            let detail = err.detail().unwrap_or_default();
            assert!(
                detail.contains(&format!("HTTP {status}")),
                "HTTP {status} 缺 detail：{detail}"
            );
            // 按状态精确匹配网关错误类型，避免任一标记都能过关
            assert!(
                detail.contains(error_type),
                "HTTP {status} detail 未保留网关说明（期望 {error_type}）：{detail}"
            );
            assert!(
                !detail.contains("oc_sk_test_key"),
                "detail 泄漏凭据：{detail}"
            );
        }
    }

    /// 限流与 5xx 仍是可重试的瞬时失败（沿用统一状态码语义）。
    #[tokio::test]
    async fn rate_limit_and_server_errors_are_transient() {
        for status in [429u16, 503] {
            let err = OPENCODE_GO
                .query(&creds(), &MockHttp::status(status), PlanVariant::Auto)
                .await
                .unwrap_err();
            assert!(err.is_transient(), "HTTP {status} 应为瞬时失败");
        }
    }

    /// 结构缺失 → 确定性失败，不静默返回空列表。
    #[tokio::test]
    async fn missing_usage_object_is_deterministic() {
        let err = OPENCODE_GO
            .query(&creds(), &MockHttp::ok(r#"{"data":{}}"#), PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("usage"));
    }

    /// 三窗口全无可用百分比 → 确定性失败（整份响应异常）。
    #[tokio::test]
    async fn empty_windows_are_deterministic() {
        let err = OPENCODE_GO
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
