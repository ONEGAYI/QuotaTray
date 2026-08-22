//! SiliconFlow 官方余额查询。
//!
//! `GET https://api.siliconflow.cn/v1/user/info`（Bearer api key）
//! 响应：`{"code": 20000, "data": {"totalBalance": "42.50", ...}}`

use async_trait::async_trait;

use super::{NativeMeta, NativeProvider, fetch_json, parse_error, parse_num};
use crate::config::Credentials;
use crate::http::HttpClient;
use crate::model::{QueryError, UsageData};

pub struct SiliconFlow;

#[async_trait]
impl NativeProvider for SiliconFlow {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "siliconflow",
            name: "SiliconFlow",
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = crate::http::HttpRequest::get("https://api.siliconflow.cn/v1/user/info")
            .bearer(&creds.api_key)
            .header("Accept", "application/json");
        let body = fetch_json(http, req).await?;

        // code != 20000 为平台业务错误（含 message），重试无意义
        if let Some(code) = body.get("code").and_then(|v| v.as_i64()) {
            if code != 20000 {
                let message = body
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("未知业务错误");
                return Err(QueryError::deterministic(format!(
                    "SiliconFlow {code}：{message}"
                )));
            }
        }

        let data = body
            .get("data")
            .ok_or_else(|| parse_error("SiliconFlow", "data 对象"))?;
        let remaining = parse_num(data.get("totalBalance"))
            .ok_or_else(|| parse_error("SiliconFlow", "data.totalBalance 数值"))?;

        Ok(vec![UsageData {
            plan_name: Some("SiliconFlow".into()),
            total: None,
            used: None,
            remaining: Some(remaining),
            // M1 面向 api.siliconflow.cn（中国站），货币为 CNY
            unit: Some("CNY".into()),
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
        Credentials {
            api_key: "sk-test".into(),
        }
    }

    async fn query_with(mock: MockHttp) -> Result<Vec<UsageData>, QueryError> {
        SiliconFlow.query(&creds(), &mock).await
    }

    /// 正常响应：totalBalance（字符串数字）→ remaining，unit 固定 CNY。
    #[tokio::test]
    async fn parses_balance() {
        let body = r#"{"code":20000,"data":{"totalBalance":"42.50"}}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(
            data,
            vec![UsageData {
                plan_name: Some("SiliconFlow".into()),
                total: None,
                used: None,
                remaining: Some(42.5),
                unit: Some("CNY".into()),
                is_valid: None,
                invalid_message: None,
                extra: None,
            }]
        );
    }

    /// 业务错误码（code != 20000）→ 确定性失败并透出平台 message。
    #[tokio::test]
    async fn business_error_code_is_deterministic() {
        let body = r#"{"code":10001,"message":"invalid token"}"#;
        let err = query_with(MockHttp::ok(body)).await.unwrap_err();
        assert!(!err.is_transient());
        assert!(format!("{err}").contains("invalid token"));
    }

    /// totalBalance 缺失 → 确定性失败。
    #[tokio::test]
    async fn missing_total_balance_is_deterministic() {
        let err = query_with(MockHttp::ok(r#"{"code":20000,"data":{}}"#))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }
}
