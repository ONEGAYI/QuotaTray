//! StepFun（阶跃星辰）官方余额查询（国内站）。
//!
//! `GET https://api.stepfun.com/v1/accounts`（Bearer api key）
//! 响应：`{"balance": 42.50, "total_cash_balance": ..., "total_voucher_balance": ...}`
//! 币种 CNY 为平台约定（响应不携带）；字段语义参考 cc-switch balance.rs，
//! 仅读顶层 `balance`，现金/代金券拆分字段未经我方验证、不透传。

use async_trait::async_trait;

use super::{NativeMeta, NativeProvider, fetch_json, parse_error, parse_num};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 国内站（api.stepfun.com，人民币）。国际站 api.stepfun.ai 的
/// 余额端点未经验证，暂不预置。
pub struct StepFun;

#[async_trait]
impl NativeProvider for StepFun {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "stepfun",
            name: "StepFun",
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = HttpRequest::get("https://api.stepfun.com/v1/accounts")
            .bearer(&creds.api_key)
            .header("Accept", "application/json");
        let body = fetch_json(http, req).await?;

        // 字段缺失/非数值为确定性失败——不学 cc-switch 兜底 0.0，
        // 避免把响应结构漂移静默显示为「余额清零」
        let remaining =
            parse_num(body.get("balance")).ok_or_else(|| parse_error("StepFun", "balance 数值"))?;

        Ok(vec![UsageData {
            plan_name: Some("StepFun".into()),
            total: None,
            used: None,
            remaining: Some(remaining),
            unit: Some("CNY".into()),
            reset_at: None,
            is_valid: None,
            invalid_message: None,
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
        Credentials::new("sk-step-test")
    }

    async fn query_with(mock: MockHttp) -> Result<Vec<UsageData>, QueryError> {
        StepFun.query(&creds(), &mock, PlanVariant::Auto).await
    }

    /// 正常响应：顶层 balance → remaining，CNY 为平台约定。
    #[tokio::test]
    async fn parses_balance() {
        let body = r#"{"object":"account","balance":42.5,"total_cash_balance":40.0,"total_voucher_balance":2.5}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(
            data,
            vec![UsageData {
                plan_name: Some("StepFun".into()),
                total: None,
                used: None,
                remaining: Some(42.5),
                unit: Some("CNY".into()),
                reset_at: None,
                is_valid: None,
                invalid_message: None,
                extra: None,
            }]
        );
    }

    /// balance 为字符串数字时同样接受（各平台 API 风格不一）。
    #[tokio::test]
    async fn accepts_string_number() {
        let body = r#"{"balance":"110.00"}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].remaining, Some(110.0));
    }

    /// balance 缺失或非数值 → 确定性失败（宁缺毋错，不兜底 0）。
    #[tokio::test]
    async fn missing_balance_is_deterministic() {
        for body in [
            r#"{"total_cash_balance":40.0}"#,
            r#"{"balance":"abc"}"#,
            "{}",
        ] {
            let err = query_with(MockHttp::ok(body)).await.unwrap_err();
            assert!(!err.is_transient(), "body {body} 应为确定性失败");
        }
    }
}
