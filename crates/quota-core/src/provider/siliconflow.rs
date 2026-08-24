//! SiliconFlow 余额查询，国内/国际双站（端点路径相同，仅域名与币种不同）。
//!
//! `GET {base}/v1/user/info`（Bearer api key）
//! 响应：`{"code": 20000, "data": {"totalBalance": "42.50", ...}}`
//! 文档：docs.siliconflow.cn / docs.siliconflow.com（双站账户独立）。

use async_trait::async_trait;

use super::{
    NativeMeta, NativeProvider, fetch_json, parse_error, parse_int, parse_num, redact_error_message,
};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 双站共享的实现，域名/币种随站点实例化。
pub struct SiliconFlow {
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    unit: &'static str,
}

/// 国内站（api.siliconflow.cn，人民币）。
pub const SILICONFLOW_CN: SiliconFlow = SiliconFlow {
    id: "siliconflow",
    name: "SiliconFlow",
    base_url: "https://api.siliconflow.cn",
    unit: "CNY",
};

/// 国际站（api.siliconflow.com，美元——官方文档未标注余额单位，
/// 依国际站整体 USD 计价推断）。
pub const SILICONFLOW_GLOBAL: SiliconFlow = SiliconFlow {
    id: "siliconflow_global",
    name: "SiliconFlow（国际站）",
    base_url: "https://api.siliconflow.com",
    unit: "USD",
};

#[async_trait]
impl NativeProvider for SiliconFlow {
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
        let req = HttpRequest::get(format!("{}/v1/user/info", self.base_url))
            .bearer(&creds.api_key)
            .header("Accept", "application/json");
        let snapshot = req.clone();
        let body = fetch_json(http, req).await?;

        // code != 20000 为平台业务错误（含 message），重试无意义；
        // 兼容数字与字符串两种 code 形态（历史版本 API 曾返回字符串）
        if let Some(code) = body.get("code").and_then(parse_int) {
            if code != 20000 {
                let message = body
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("未知业务错误");
                return Err(redact_error_message(
                    QueryError::deterministic(format!("SiliconFlow {code}：{message}")),
                    &snapshot,
                ));
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
            unit: Some(self.unit.into()),
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
        Credentials::new("sk-test")
    }

    async fn query_with(mock: MockHttp) -> Result<Vec<UsageData>, QueryError> {
        SILICONFLOW_CN
            .query(&creds(), &mock, PlanVariant::Auto)
            .await
    }

    /// 正常响应：totalBalance（字符串数字）→ remaining，unit 按站点币种。
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
                reset_at: None,
                is_valid: None,
                invalid_message: None,
                extra: None,
            }]
        );
    }

    /// 国际站：同构响应，域名与 unit 切换为 USD。
    #[tokio::test]
    async fn global_variant_hits_com_domain_in_usd() {
        let mock = MockHttp::ok(r#"{"code":20000,"data":{"totalBalance":"1.25"}}"#);
        let data = SILICONFLOW_GLOBAL
            .query(&creds(), &mock, PlanVariant::Auto)
            .await
            .unwrap();
        assert_eq!(data[0].remaining, Some(1.25));
        assert_eq!(data[0].unit.as_deref(), Some("USD"));
        assert_eq!(
            mock.captured_requests()[0].url,
            "https://api.siliconflow.com/v1/user/info"
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

    /// 安全契约：业务错误 message 中的回显凭据在透出前已脱敏。
    #[tokio::test]
    async fn business_error_message_redacts_echoed_secret() {
        // 回显串与请求 bearer 密钥一致（字面量替换按请求密钥收集）
        let body = r#"{"code":10001,"message":"invalid key sk-test provided"}"#;
        let err = query_with(MockHttp::ok(body)).await.unwrap_err();
        assert!(
            !err.message().contains("sk-test"),
            "业务错误 message 泄漏回显凭据：{}",
            err.message()
        );
        assert!(err.message().contains("<redacted>"), "{}", err.message());
    }

    /// totalBalance 缺失 → 确定性失败。
    #[tokio::test]
    async fn missing_total_balance_is_deterministic() {
        let err = query_with(MockHttp::ok(r#"{"code":20000,"data":{}}"#))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }

    /// code 为字符串数字时仍走业务错误检查（不退化为结构解析错误）。
    #[tokio::test]
    async fn string_code_still_checked() {
        let body = r#"{"code":"10001","message":"invalid token"}"#;
        let err = query_with(MockHttp::ok(body)).await.unwrap_err();
        assert!(!err.is_transient());
        assert!(
            err.message().contains("10001") && err.message().contains("invalid token"),
            "实际：{err}"
        );
    }

    /// code 缺失但结构完整 → 放行（宽松兼容，靠结构校验兜底）。
    #[tokio::test]
    async fn missing_code_with_valid_structure_succeeds() {
        let body = r#"{"data":{"totalBalance":"1.25"}}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].remaining, Some(1.25));
    }
}
