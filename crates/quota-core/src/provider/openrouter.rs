//! OpenRouter 官方余额查询。
//!
//! `GET https://openrouter.ai/api/v1/credits`（Bearer api key）
//! 响应：`{"data": {"total_credits": 10.0, "total_usage": 3.5}}`
//! remaining = total_credits − total_usage（USD）；remaining ≤ 0 视为无效。

use async_trait::async_trait;

use super::{NativeMeta, NativeProvider, fetch_json, parse_error, parse_num};
use crate::config::Credentials;
use crate::http::HttpClient;
use crate::model::{QueryError, UsageData};

pub struct OpenRouter;

#[async_trait]
impl NativeProvider for OpenRouter {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "openrouter",
            name: "OpenRouter",
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = crate::http::HttpRequest::get("https://openrouter.ai/api/v1/credits")
            .bearer(&creds.api_key)
            .header("Accept", "application/json");
        let body = fetch_json(http, req).await?;

        let data = body
            .get("data")
            .ok_or_else(|| parse_error("OpenRouter", "data 对象"))?;
        let total = parse_num(data.get("total_credits"))
            .ok_or_else(|| parse_error("OpenRouter", "total_credits 数值"))?;
        let used = parse_num(data.get("total_usage"))
            .ok_or_else(|| parse_error("OpenRouter", "total_usage 数值"))?;
        // 输入侧已拒绝非有限值，但两个有限值相减仍可溢出为 ±inf
        //（如 1e308 − (−1e308)），inf 会绕过耗尽判断且序列化为 null
        let remaining = total - used;
        if !remaining.is_finite() {
            return Err(parse_error("OpenRouter", "remaining 计算溢出"));
        }

        let exhausted = remaining <= 0.0;
        Ok(vec![UsageData {
            plan_name: Some("OpenRouter".into()),
            total: Some(total),
            used: Some(used),
            remaining: Some(remaining),
            unit: Some("USD".into()),
            is_valid: Some(!exhausted),
            invalid_message: exhausted.then(|| "额度已耗尽".into()),
            extra: None,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UsageData;
    use crate::provider::testing::MockHttp;

    fn creds() -> Credentials {
        Credentials::new("sk-or-test")
    }

    async fn query_with(mock: MockHttp) -> Result<Vec<UsageData>, QueryError> {
        OpenRouter.query(&creds(), &mock).await
    }

    /// 正常响应：remaining = total_credits − total_usage。
    #[tokio::test]
    async fn computes_remaining() {
        let body = r#"{"data":{"total_credits":10.0,"total_usage":3.5}}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].total, Some(10.0));
        assert_eq!(data[0].used, Some(3.5));
        assert_eq!(data[0].remaining, Some(6.5));
        assert_eq!(data[0].unit.as_deref(), Some("USD"));
        assert_eq!(data[0].is_valid, Some(true));
    }

    /// remaining ≤ 0 → is_valid=false（继承 cc-switch 语义）。
    #[tokio::test]
    async fn exhausted_credits_marks_invalid() {
        let body = r#"{"data":{"total_credits":10.0,"total_usage":10.0}}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].is_valid, Some(false));
        assert!(data[0].invalid_message.is_some());
    }

    /// 字段缺失 → 确定性失败。
    #[tokio::test]
    async fn missing_fields_are_deterministic() {
        let err = query_with(MockHttp::ok(r#"{"data":{"total_credits":10.0}}"#))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }

    /// 负 remaining（已用超出总额，如 5 − 10）→ 无效。
    #[tokio::test]
    async fn negative_remaining_marks_invalid() {
        let body = r#"{"data":{"total_credits":5.0,"total_usage":10.0}}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].remaining, Some(-5.0));
        assert_eq!(data[0].is_valid, Some(false));
    }

    /// 数值为非有限字符串（"NaN"）→ 确定性解析失败，不得绕过 exhausted 判断。
    #[tokio::test]
    async fn non_finite_values_rejected() {
        let body = r#"{"data":{"total_credits":"NaN","total_usage":1.0}}"#;
        let err = query_with(MockHttp::ok(body)).await.unwrap_err();
        assert!(!err.is_transient());
    }

    /// 相减溢出为 inf（1e308 − (−1e308)）→ 确定性解析失败。
    #[tokio::test]
    async fn overflowed_subtraction_rejected() {
        let body = r#"{"data":{"total_credits":1e308,"total_usage":-1e308}}"#;
        let err = query_with(MockHttp::ok(body)).await.unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("溢出"), "实际：{err}");
    }
}
