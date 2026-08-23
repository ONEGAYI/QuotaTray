//! Kimi（月之暗面 Moonshot）开放平台余额查询，国内/国际双站。
//!
//! `GET {base}/v1/users/me/balance`（Bearer api key）
//! 响应：`{"code": 0, "data": {"available_balance": 49.5, ...}}`
//! 文档：platform.kimi.com / platform.kimi.ai（原 platform.moonshot.*，
//! 双站账户与 key 完全独立，API 域名分别为 api.moonshot.cn / .ai）。

use async_trait::async_trait;

use super::{NativeMeta, NativeProvider, fetch_json, parse_error, parse_num};
use crate::config::Credentials;
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
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = HttpRequest::get(format!("{}/v1/users/me/balance", self.base_url))
            .bearer(&creds.api_key);
        let body = fetch_json(http, req).await?;

        // code != 0 为业务错误（401 等已由 HTTP 状态码分类处理）
        if let Some(code) = body.get("code").and_then(parse_int) {
            if code != 0 {
                let message = body
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("未知业务错误");
                return Err(QueryError::deterministic(format!("Kimi {code}：{message}")));
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
            is_valid: None,
            invalid_message: None,
            extra: Some(serde_json::json!({
                "voucher_balance": parse_num(data.get("voucher_balance")),
                "cash_balance": parse_num(data.get("cash_balance")),
            })),
        }])
    }
}

/// 整数字段解析：兼容 JSON number 与字符串数字。
fn parse_int(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
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

    /// 正常响应：available_balance → remaining，unit 按站点币种。
    #[tokio::test]
    async fn parses_balance_cn_and_global() {
        let body = r#"{"code":0,"data":{"available_balance":49.58894,"voucher_balance":46.58893,"cash_balance":3.00001}}"#;
        for (provider, unit) in [(&KIMI_CN, "CNY"), (&KIMI_GLOBAL, "USD")] {
            let data = provider.query(&creds(), &MockHttp::ok(body)).await.unwrap();
            assert_eq!(data[0].remaining, Some(49.58894), "{unit}");
            assert_eq!(data[0].unit.as_deref(), Some(unit));
            assert_eq!(
                data[0].extra.as_ref().unwrap()["voucher_balance"],
                serde_json::json!(46.58893)
            );
        }
    }

    /// 请求打往站点各自的域名，且带 Bearer 头。
    #[tokio::test]
    async fn hits_site_specific_domain_with_bearer() {
        let mock = MockHttp::ok(r#"{"code":0,"data":{"available_balance":1.0}}"#);
        KIMI_CN.query(&creds(), &mock).await.unwrap();
        let req = &mock.captured_requests()[0];
        assert_eq!(
            req.url, "https://api.moonshot.cn/v1/users/me/balance",
            "国内站域名"
        );
        assert_eq!(auth_of(req), "Bearer sk-test");

        let mock = MockHttp::ok(r#"{"code":0,"data":{"available_balance":1.0}}"#);
        KIMI_GLOBAL.query(&creds(), &mock).await.unwrap();
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
            .query(&creds(), &MockHttp::ok(body))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("invalid api key"));
    }

    /// available_balance 缺失 → 确定性失败。
    #[tokio::test]
    async fn missing_balance_is_deterministic() {
        let err = KIMI_CN
            .query(&creds(), &MockHttp::ok(r#"{"code":0,"data":{}}"#))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }
}
