//! Kimi Code 订阅额度查询，国内/国际双站。

use async_trait::async_trait;

use serde_json::Value;

use super::{
    NativeMeta, NativeProvider, parse_error, parse_num, parse_success_json, status_error_with_body,
};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpError, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 双站共享实现。
pub struct KimiCode {
    id: &'static str,
    name: &'static str,
    endpoint: &'static str,
}

pub const KIMI_CODE_CN: KimiCode = KimiCode {
    id: "kimi_code_cn",
    name: "Kimi Code（kimi.com/code）",
    endpoint: "https://api.kimi.com/coding/v1/usages",
};

pub const KIMI_CODE_GLOBAL: KimiCode = KimiCode {
    id: "kimi_code_global",
    name: "Kimi Code（kimi.ai/code）",
    endpoint: "https://api.kimi.ai/coding/v1/usages",
};

fn reset_at(value: Option<&Value>) -> Option<i64> {
    let raw = value?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|time| time.timestamp_millis())
}

fn usage_row(detail: &Value, label: &str) -> Option<UsageData> {
    let used = parse_num(detail.get("used"))?;
    let total = parse_num(detail.get("limit"))?;
    Some(UsageData {
        plan_name: Some(format!("Kimi Code（{label}）")),
        total: Some(total),
        used: Some(used),
        // 官方当前契约没有 remaining；即使远端或旧代理额外返回该字段，
        // 也统一由 limit-used 推导，超额使用时将剩余值钳到零。
        remaining: Some((total - used).max(0.0)),
        unit: None,
        reset_at: reset_at(detail.get("resetTime")),
        is_valid: None,
        invalid_message: None,
        // 响应还含账户钱包信息，不整包透传，避免调试烟测误输出。
        extra: None,
    })
}

fn parse_rows(body: &Value) -> Vec<UsageData> {
    let five_hour = body
        .get("limits")
        .and_then(Value::as_array)
        .and_then(|limits| {
            limits.iter().find_map(|item| {
                let window = item.get("window")?;
                let duration = parse_num(window.get("duration"))?;
                let time_unit = window.get("timeUnit")?.as_str()?;
                if duration == 300.0 && time_unit == "TIME_UNIT_MINUTE" {
                    usage_row(item.get("detail")?, "5h")
                } else {
                    None
                }
            })
        });
    let weekly = body.get("usage").and_then(|usage| usage_row(usage, "week"));

    [five_hour, weekly].into_iter().flatten().collect()
}

#[async_trait]
impl NativeProvider for KimiCode {
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
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let request = HttpRequest::get(self.endpoint)
            .bearer(&creds.api_key)
            .header("Accept", "application/json");
        let response = http
            .execute(request.clone())
            .await
            .map_err(|error| match &error {
                HttpError::Timeout | HttpError::Network(_) => {
                    QueryError::transient(error.to_string())
                }
                HttpError::InvalidRequest(_) => QueryError::deterministic(error.to_string()),
            })?;

        if !response.is_success() {
            let common = status_error_with_body(response.status, &response.body, &request);
            // Kimi Code 的 402 表示当前订阅额度暂不可用，按可恢复错误处理；
            // 此特例只属于该端点，不改变其他 Provider 的全局 HTTP 语义。
            return Err(if response.status == 402 {
                let rerouted = QueryError::transient(common.message());
                match common.detail() {
                    Some(d) => rerouted.with_detail(d),
                    None => rerouted,
                }
            } else {
                common
            });
        }

        let body: Value = parse_success_json(&request, &response)?;
        let rows = parse_rows(&body);
        if rows.is_empty() {
            return Err(parse_error(
                self.name,
                "usage.used/limit 或 5h limits[].detail.used/limit 数值",
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
        Credentials::new("sk-kimi-code-test")
    }

    fn header<'a>(req: &'a crate::http::HttpRequest, name: &str) -> Option<&'a str> {
        req.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// 官方线协议：双站端点不同，均为 GET + Bearer + JSON Accept。
    #[tokio::test]
    async fn sends_official_requests_for_both_sites() {
        let body = r#"{"usage":{"used":1,"limit":2,"resetTime":"2026-08-03T05:20:51Z"}}"#;
        for (provider, endpoint) in [
            (&KIMI_CODE_CN, "https://api.kimi.com/coding/v1/usages"),
            (&KIMI_CODE_GLOBAL, "https://api.kimi.ai/coding/v1/usages"),
        ] {
            let mock = MockHttp::ok(body);
            provider
                .query(&creds(), &mock, PlanVariant::Auto)
                .await
                .unwrap();
            let requests = mock.captured_requests();
            let req = &requests[0];
            assert_eq!(req.method, Method::Get);
            assert_eq!(req.url, endpoint);
            assert_eq!(
                header(req, "Authorization"),
                Some("Bearer sk-kimi-code-test")
            );
            assert_eq!(header(req, "Accept"), Some("application/json"));
        }
    }

    /// 5h 必须由 window.duration/timeUnit 精确识别，输出顺序固定为 5h → week；
    /// 数字与数字字符串均接受，remaining 由 limit-used 计算且下限为零。
    #[tokio::test]
    async fn parses_five_hour_then_week_from_official_fields() {
        let body = r#"{
            "usage":{"used":"120","limit":100,"remaining":999,"resetTime":"2026-08-10T05:20:51Z"},
            "limits":[
                {"window":{"duration":60,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"used":99,"limit":100,"resetTime":"2026-08-03T01:00:00Z"}},
                {"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"used":"25.5","limit":"80","remaining":999,"resetTime":"2026-08-03T05:20:51Z"}}
            ],
            "boosterWallet":{"account":"must-not-leak"}
        }"#;
        let rows = KIMI_CODE_CN
            .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
            .await
            .unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].plan_name.as_deref(), Some("Kimi Code（5h）"));
        assert_eq!(rows[0].used, Some(25.5));
        assert_eq!(rows[0].total, Some(80.0));
        assert_eq!(rows[0].remaining, Some(54.5));
        assert_eq!(rows[0].unit, None);
        assert_eq!(rows[0].reset_at, Some(1_785_734_451_000));
        assert_eq!(rows[0].extra, None, "不得透传钱包等完整响应");

        assert_eq!(rows[1].plan_name.as_deref(), Some("Kimi Code（week）"));
        assert_eq!(rows[1].used, Some(120.0));
        assert_eq!(rows[1].total, Some(100.0));
        assert_eq!(rows[1].remaining, Some(0.0), "超额时钳到零");
        assert_eq!(rows[1].reset_at, Some(1_786_339_251_000));
    }

    /// 两种窗口互不依赖：任一字段组有效即可成功；无有效行才确定性失败。
    #[tokio::test]
    async fn supports_partial_success_and_rejects_no_valid_rows() {
        let only_five = r#"{"usage":{"used":"bad","limit":10},"limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"used":2,"limit":8}}]}"#;
        let rows = KIMI_CODE_CN
            .query(&creds(), &MockHttp::ok(only_five), PlanVariant::Weekly)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].plan_name.as_deref(), Some("Kimi Code（5h）"));

        let only_week = r#"{"usage":{"used":3,"limit":"9"},"limits":[]}"#;
        let rows = KIMI_CODE_CN
            .query(&creds(), &MockHttp::ok(only_week), PlanVariant::NoWeekly)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "Kimi Code 不受 GLM 套餐变体过滤");
        assert_eq!(rows[0].plan_name.as_deref(), Some("Kimi Code（week）"));

        let err = KIMI_CODE_CN
            .query(
                &creds(),
                &MockHttp::ok(r#"{"usage":{},"limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_HOUR"},"detail":{"used":1,"limit":2}}]}"#),
                PlanVariant::Auto,
            )
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }

    /// resetTime 只接受 RFC3339 字符串；格式非法或非字符串时不伪造时间戳。
    #[tokio::test]
    async fn reset_time_accepts_only_rfc3339_strings() {
        let body = r#"{
            "usage":{"used":1,"limit":2,"resetTime":1775800851000},
            "limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"used":1,"limit":2,"resetTime":"not-a-date"}}]
        }"#;
        let rows = KIMI_CODE_CN
            .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
            .await
            .unwrap();
        assert_eq!(rows[0].reset_at, None);
        assert_eq!(rows[1].reset_at, None);
    }

    /// Kimi Code 特例：402 表示额度暂不可用，可恢复，因此按瞬时失败；
    /// 401/403/404 为确定性，429/5xx/网络为瞬时。
    #[tokio::test]
    async fn classifies_statuses_with_402_as_transient() {
        for code in [401, 403, 404] {
            let err = KIMI_CODE_CN
                .query(&creds(), &MockHttp::status(code), PlanVariant::Auto)
                .await
                .unwrap_err();
            assert!(!err.is_transient(), "{code} 应为确定性");
        }
        for code in [402, 429, 500, 503] {
            let err = KIMI_CODE_CN
                .query(&creds(), &MockHttp::status(code), PlanVariant::Auto)
                .await
                .unwrap_err();
            assert!(err.is_transient(), "{code} 应为瞬时");
        }
        let err = KIMI_CODE_CN
            .query(&creds(), &MockHttp::fail(), PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(err.is_transient(), "网络错误应为瞬时");
    }
}
