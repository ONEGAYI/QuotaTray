//! Kimi（月之暗面 Moonshot）开放平台余额查询，国内/国际双站。
//!
//! `GET {base}/v1/users/me/balance`（Bearer api key）
//! 响应：`{"code": 0, "data": {"available_balance": 49.5, ...}}`
//! 文档：platform.kimi.com / platform.kimi.ai（原 platform.moonshot.*，
//! 双站账户与 key 完全独立，API 域名分别为 api.moonshot.cn / .ai）。

use async_trait::async_trait;

use super::{
    NativeMeta, NativeProvider, fetch_json, parse_error, parse_int, parse_num, redact_error_message,
};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 双站共享的实现，`id`/`base_url`/币种随站点实例化。
pub struct Kimi {
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    unit: &'static str,
}

/// 国内站（api.moonshot.cn，人民币账户）。
pub const KIMI_CN: Kimi = Kimi {
    id: "kimi_cn",
    name: "Kimi（国内站）",
    base_url: "https://api.moonshot.cn",
    unit: "CNY",
};

/// 国际站（api.moonshot.ai，美元账户）。
pub const KIMI_GLOBAL: Kimi = Kimi {
    id: "kimi_global",
    name: "Kimi（国际站）",
    base_url: "https://api.moonshot.ai",
    unit: "USD",
};

#[async_trait]
impl NativeProvider for Kimi {
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
        let req = HttpRequest::get(format!("{}/v1/users/me/balance", self.base_url))
            .bearer(&creds.api_key);
        let snapshot = req.clone();
        let body = fetch_json(http, req).await?;

        // code != 0 为业务错误（401 等已由 HTTP 状态码分类处理）
        if let Some(code) = body.get("code").and_then(parse_int) {
            if code != 0 {
                let message = body
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("未知业务错误");
                return Err(redact_error_message(
                    QueryError::deterministic(format!("Kimi {code}：{message}")),
                    &snapshot,
                ));
            }
        }

        let data = body
            .get("data")
            .ok_or_else(|| parse_error("Kimi", "data 对象"))?;
        let remaining = parse_num(data.get("available_balance"))
            .ok_or_else(|| parse_error("Kimi", "data.available_balance 数值"))?;

        // 代金券/现金拆分进 extra（主表展示可用余额 = 现金 + 代金券）
        Ok(vec![UsageData {
            plan_name: Some("Kimi".into()),
            total: None,
            used: None,
            remaining: Some(remaining),
            unit: Some(self.unit.into()),
            reset_at: None,
            is_valid: None,
            invalid_message: None,
            extra: Some(serde_json::json!({
                "voucher_balance": parse_num(data.get("voucher_balance")),
                "cash_balance": parse_num(data.get("cash_balance")),
            })),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    fn creds() -> Credentials {
        Credentials::new("sk-test")
    }

    fn auth_of(req: &crate::http::HttpRequest) -> &str {
        req.headers
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.as_str())
            .unwrap()
    }

    /// 正常响应：available_balance → remaining，unit 按站点币种；
    /// 代金券/现金拆分完整透传 extra。
    #[tokio::test]
    async fn parses_balance_cn_and_global() {
        let body = r#"{"code":0,"data":{"available_balance":49.58894,"voucher_balance":46.58893,"cash_balance":3.00001}}"#;
        for (provider, unit) in [(&KIMI_CN, "CNY"), (&KIMI_GLOBAL, "USD")] {
            let data = provider
                .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
                .await
                .unwrap();
            assert_eq!(data[0].remaining, Some(49.58894), "{unit}");
            assert_eq!(data[0].unit.as_deref(), Some(unit));
            let extra = data[0].extra.as_ref().unwrap();
            assert_eq!(extra["voucher_balance"], serde_json::json!(46.58893));
            assert_eq!(extra["cash_balance"], serde_json::json!(3.00001));
        }
    }

    /// 请求打往站点各自的域名，且带 Bearer 头。
    #[tokio::test]
    async fn hits_site_specific_domain_with_bearer() {
        let mock = MockHttp::ok(r#"{"code":0,"data":{"available_balance":1.0}}"#);
        KIMI_CN
            .query(&creds(), &mock, PlanVariant::Auto)
            .await
            .unwrap();
        let req = &mock.captured_requests()[0];
        assert_eq!(
            req.url, "https://api.moonshot.cn/v1/users/me/balance",
            "国内站域名"
        );
        assert_eq!(auth_of(req), "Bearer sk-test");

        let mock = MockHttp::ok(r#"{"code":0,"data":{"available_balance":1.0}}"#);
        KIMI_GLOBAL
            .query(&creds(), &mock, PlanVariant::Auto)
            .await
            .unwrap();
        assert_eq!(
            mock.captured_requests()[0].url,
            "https://api.moonshot.ai/v1/users/me/balance",
            "国际站域名"
        );
    }

    /// 业务错误码（code != 0）→ 确定性失败并透出 message。
    #[tokio::test]
    async fn business_error_code_is_deterministic() {
        let body = r#"{"code":401,"message":"invalid api key"}"#;
        let err = KIMI_CN
            .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("invalid api key"));
    }

    /// 安全契约：业务错误 message 中的回显凭据在透出前已脱敏
    /// （2xx 业务错误同样可能回显请求凭据）。
    #[tokio::test]
    async fn business_error_message_redacts_echoed_secret() {
        // 回显串与请求 bearer 密钥一致（字面量替换按请求密钥收集）
        let body = r#"{"code":401,"message":"invalid key sk-test provided"}"#;
        let err = KIMI_CN
            .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(
            !err.message().contains("sk-test"),
            "业务错误 message 泄漏回显凭据：{}",
            err.message()
        );
        assert!(err.message().contains("<redacted>"), "{err}");
    }

    /// code 为字符串数字时仍走业务错误检查（与 SiliconFlow 同语义）。
    #[tokio::test]
    async fn string_code_still_checked() {
        let body = r#"{"code":"401","message":"invalid api key"}"#;
        let err = KIMI_CN
            .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(
            err.message().contains("401") && err.message().contains("invalid api key"),
            "实际：{err}"
        );
    }

    /// 错误分类：非 JSON 响应确定性；网络故障瞬时。
    #[tokio::test]
    async fn error_classification() {
        let err = KIMI_CN
            .query(
                &creds(),
                &MockHttp::ok("<html>Bad Gateway</html>"),
                PlanVariant::Auto,
            )
            .await
            .unwrap_err();
        assert!(!err.is_transient(), "非 JSON 应为确定性");

        let err = KIMI_CN
            .query(&creds(), &MockHttp::fail(), PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(err.is_transient(), "网络故障应为瞬时");
    }

    /// available_balance 缺失 → 确定性失败。
    #[tokio::test]
    async fn missing_balance_is_deterministic() {
        let err = KIMI_CN
            .query(
                &creds(),
                &MockHttp::ok(r#"{"code":0,"data":{}}"#),
                PlanVariant::Auto,
            )
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }
}
