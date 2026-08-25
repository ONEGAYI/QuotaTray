//! Novita AI 官方余额查询。
//!
//! `GET https://api.novita.ai/v3/user/balance`（Bearer api key）
//! 响应：`{"availableBalance": 1000000, "cashBalance": ..., ...}`
//! 原始单位为 0.0001 USD，÷10000 换算（字段语义参考 cc-switch balance.rs）。

use async_trait::async_trait;

use super::{NativeMeta, NativeProvider, fetch_json, parse_error, parse_num};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 每美元对应的原始余额单位数（1 USD = 10000 单位，即原始单位 0.0001 USD）。
const RAW_UNITS_PER_USD: f64 = 10_000.0;

pub struct Novita;

#[async_trait]
impl NativeProvider for Novita {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "novita",
            name: "Novita AI",
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = HttpRequest::get("https://api.novita.ai/v3/user/balance")
            .bearer(&creds.api_key)
            .header("Accept", "application/json");
        let body = fetch_json(http, req).await?;

        let raw = parse_num(body.get("availableBalance"))
            .ok_or_else(|| parse_error("Novita AI", "availableBalance 数值"))?;
        let remaining = raw / RAW_UNITS_PER_USD;

        // 余额耗尽标记沿用 OpenRouter 先例：不是凭据失效，是「没钱了」的独立表达
        let exhausted = remaining <= 0.0;
        Ok(vec![UsageData {
            plan_name: Some("Novita AI".into()),
            total: None,
            used: None,
            remaining: Some(remaining),
            unit: Some("USD".into()),
            reset_at: None,
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
        Credentials::new("sk-novita-test")
    }

    async fn query_with(mock: MockHttp) -> Result<Vec<UsageData>, QueryError> {
        Novita.query(&creds(), &mock, PlanVariant::Auto).await
    }

    /// 换算契约：原始单位 0.0001 USD，÷10000 → USD。
    #[tokio::test]
    async fn converts_cent_milli_usd() {
        let body = r#"{"availableBalance":1000000,"cashBalance":800000,"creditLimit":0}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(
            data,
            vec![UsageData {
                plan_name: Some("Novita AI".into()),
                total: None,
                used: None,
                remaining: Some(100.0),
                unit: Some("USD".into()),
                reset_at: None,
                is_valid: Some(true),
                invalid_message: None,
                extra: None,
            }]
        );
    }

    /// 字符串数字同样接受；小数换算精度不受影响。
    #[tokio::test]
    async fn accepts_string_number() {
        let data = query_with(MockHttp::ok(r#"{"availableBalance":"500000"}"#))
            .await
            .unwrap();
        assert_eq!(data[0].remaining, Some(50.0));
    }

    /// 余额 ≤ 0 → is_valid=false + 「额度已耗尽」（OpenRouter 同款语义）；
    /// 负余额保留原值（欠费可见）。
    #[tokio::test]
    async fn zero_or_negative_balance_marks_exhausted() {
        for (raw, expect) in [(0.0, 0.0), (-25000.0, -2.5)] {
            let body = format!(r#"{{"availableBalance":{raw}}}"#);
            let data = query_with(MockHttp::ok(&body)).await.unwrap();
            assert_eq!(data[0].remaining, Some(expect));
            assert_eq!(data[0].is_valid, Some(false));
            assert_eq!(data[0].invalid_message.as_deref(), Some("额度已耗尽"));
        }
    }

    /// availableBalance 缺失/非数值 → 确定性失败（不兜底 0）。
    #[tokio::test]
    async fn missing_balance_is_deterministic() {
        for body in [
            r#"{"cashBalance":100}"#,
            r#"{"availableBalance":"x"}"#,
            "{}",
        ] {
            let err = query_with(MockHttp::ok(body)).await.unwrap_err();
            assert!(!err.is_transient(), "body {body} 应为确定性失败");
        }
    }
}
