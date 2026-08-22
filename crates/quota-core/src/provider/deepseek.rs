//! DeepSeek 官方余额查询。
//!
//! `GET https://api.deepseek.com/user/balance`（Bearer api key）
//! 响应：`{"is_available": true, "balance_infos": [{"currency": "CNY", "total_balance": "110.00"}]}`

use async_trait::async_trait;

use super::{NativeMeta, NativeProvider, fetch_json, parse_error, parse_num};
use crate::config::Credentials;
use crate::http::HttpClient;
use crate::model::{QueryError, UsageData};

pub struct DeepSeek;

#[async_trait]
impl NativeProvider for DeepSeek {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "deepseek",
            name: "DeepSeek",
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = crate::http::HttpRequest::get("https://api.deepseek.com/user/balance")
            .bearer(&creds.api_key)
            .header("Accept", "application/json");
        let body = fetch_json(http, req).await?;

        let infos = body
            .get("balance_infos")
            .and_then(|v| v.as_array())
            .ok_or_else(|| parse_error("DeepSeek", "balance_infos 数组"))?;
        let first = infos
            .first()
            .ok_or_else(|| parse_error("DeepSeek", "balance_infos 至少一条"))?;

        let remaining = parse_num(first.get("total_balance"))
            .ok_or_else(|| parse_error("DeepSeek", "total_balance 数值"))?;
        let unit = first
            .get("currency")
            .and_then(|v| v.as_str())
            .map(String::from);

        // is_available 缺失时按 true 处理（API 文档为必填，防御旧端点）
        let is_available = body
            .get("is_available")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Ok(vec![UsageData {
            plan_name: Some("DeepSeek".into()),
            total: None,
            used: None,
            remaining: Some(remaining),
            unit,
            is_valid: Some(is_available),
            invalid_message: (!is_available).then(|| "账户不可用（is_available=false）".into()),
            extra: None,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UsageData;
    use crate::provider::testing::MockHttp;

    const OK_BODY: &str = r#"{
        "is_available": true,
        "balance_infos": [{"currency": "CNY", "total_balance": "110.00"}]
    }"#;

    fn creds() -> Credentials {
        Credentials {
            api_key: "sk-test".into(),
        }
    }

    async fn query_with(mock: MockHttp) -> Result<Vec<UsageData>, QueryError> {
        DeepSeek.query(&creds(), &mock).await
    }

    /// 正常响应：remaining 取自 total_balance，unit 取自 currency。
    #[tokio::test]
    async fn parses_balance() {
        let data = query_with(MockHttp::ok(OK_BODY)).await.unwrap();
        assert_eq!(
            data,
            vec![UsageData {
                plan_name: Some("DeepSeek".into()),
                total: None,
                used: None,
                remaining: Some(110.0),
                unit: Some("CNY".into()),
                is_valid: Some(true),
                invalid_message: None,
                extra: None,
            }]
        );
    }

    /// is_available=false：is_valid 置 false 并给出失效说明。
    #[tokio::test]
    async fn unavailable_account_marks_invalid() {
        let body = r#"{
            "is_available": false,
            "balance_infos": [{"currency": "CNY", "total_balance": "0"}]
        }"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].is_valid, Some(false));
        assert!(data[0].invalid_message.is_some());
    }

    /// balance_infos 缺失/为空 → 确定性失败。
    #[tokio::test]
    async fn missing_balance_infos_is_deterministic() {
        for body in ["{}", r#"{"balance_infos": []}"#] {
            let err = query_with(MockHttp::ok(body)).await.unwrap_err();
            assert!(!err.is_transient(), "body={body} 应为确定性失败");
        }
    }

    /// 401 → 确定性；500 → 瞬时；网络故障 → 瞬时；非法 JSON → 确定性。
    #[tokio::test]
    async fn error_classification() {
        assert!(
            !query_with(MockHttp::status(401))
                .await
                .unwrap_err()
                .is_transient()
        );
        assert!(
            query_with(MockHttp::status(500))
                .await
                .unwrap_err()
                .is_transient()
        );
        assert!(
            query_with(MockHttp::fail())
                .await
                .unwrap_err()
                .is_transient()
        );
        assert!(
            !query_with(MockHttp::ok("not-json"))
                .await
                .unwrap_err()
                .is_transient()
        );
    }
}
