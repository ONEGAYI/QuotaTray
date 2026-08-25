//! 阿里云百炼（DashScope）充值余额查询。
//!
//! `GET https://dashscope.aliyuncs.com/api/v1/recharge/recharge-balance/query`
//! （Bearer api key），响应顶层 `available_balance` 即可用余额，币种 CNY
//! 为平台约定（响应不携带）。端点无官方文档，契约来自社区实现交叉验证
//! （RAGFlow issue #14671 与 API-Key-Manager dashscope.py）；字段名仅单一
//! 来源明示，真实响应如有出入改此处即可，属已知边界。
//!
//! 免费额度查询走百炼控制台网关（官方 CLI `bl usage free`），需控制台
//! OAuth 登录 token 而非 api key，凭据模型不匹配，不接入。

use async_trait::async_trait;

use super::{NativeMeta, NativeProvider, fetch_json, parse_error, parse_num};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 主 API 域名（dashscope.aliyuncs.com）上的充值余额端点。
/// 注意与已下线的控制台域名 dashscope.console.aliyun.com 无关，
/// 不受 2026-08 控制台迁移影响。
pub struct Bailian;

#[async_trait]
impl NativeProvider for Bailian {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "bailian",
            name: "阿里云百炼",
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = HttpRequest::get(
            "https://dashscope.aliyuncs.com/api/v1/recharge/recharge-balance/query",
        )
        .bearer(&creds.api_key)
        .header("Accept", "application/json");
        let body = fetch_json(http, req).await?;

        // 字段缺失/非数值为确定性失败——端点无官方文档，响应结构漂移
        // 必须显式报错而非静默显示「余额清零」
        let remaining = parse_num(body.get("available_balance"))
            .ok_or_else(|| parse_error("阿里云百炼", "available_balance 数值"))?;

        Ok(vec![UsageData {
            plan_name: Some("阿里云百炼".into()),
            remaining: Some(remaining),
            unit: Some("CNY".into()),
            ..Default::default()
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    fn creds() -> Credentials {
        Credentials::new("sk-bailian-test")
    }

    async fn query_with(mock: MockHttp) -> Result<Vec<UsageData>, QueryError> {
        Bailian.query(&creds(), &mock, PlanVariant::Auto).await
    }

    /// 正常响应：顶层 available_balance → remaining，CNY 为平台约定。
    #[tokio::test]
    async fn parses_balance() {
        let body = r#"{"available_balance": 12.34}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].plan_name.as_deref(), Some("阿里云百炼"));
        assert_eq!(data[0].remaining, Some(12.34));
        assert_eq!(data[0].unit.as_deref(), Some("CNY"));
        assert_eq!(data[0].total, None);
        assert_eq!(data[0].used, None);
    }

    /// available_balance 为字符串数字时同样接受（各平台 API 风格不一）。
    #[tokio::test]
    async fn accepts_string_number() {
        let body = r#"{"available_balance":"88.00"}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].remaining, Some(88.0));
    }

    /// 字段缺失或非数值 → 确定性失败（宁缺毋错，不兜底 0）。
    #[tokio::test]
    async fn missing_balance_is_deterministic() {
        for body in [
            r#"{"balance": 12.34}"#,
            r#"{"available_balance":"abc"}"#,
            "{}",
        ] {
            let err = query_with(MockHttp::ok(body)).await.unwrap_err();
            assert!(!err.is_transient(), "body {body} 应为确定性失败");
        }
    }

    /// 401（invalid_api_key）→ 确定性；403 同为凭据/权限类。
    #[tokio::test]
    async fn auth_errors_are_deterministic() {
        let body =
            r#"{"code":"InvalidApiKey","message":"Invalid API-key provided.","request_id":"..."}"#;
        for status in [401u16, 403] {
            let mut mock = MockHttp::ok("");
            mock.status = status;
            mock.body = body.into();
            let err = query_with(mock).await.unwrap_err();
            assert!(!err.is_transient(), "HTTP {status} 应为确定性失败");
        }
    }

    /// 429（Throttling.*）→ 瞬时；5xx 同。
    #[tokio::test]
    async fn throttling_is_transient() {
        let mut mock = MockHttp::ok("");
        mock.status = 429;
        mock.body =
            r#"{"code":"Throttling.RateQuota","message":"Requests throttling triggered."}"#.into();
        let err = query_with(mock).await.unwrap_err();
        assert!(err.is_transient(), "429 应为瞬时失败");
    }

    /// 请求契约：打到主 API 域名的充值余额端点，Bearer 鉴权。
    #[tokio::test]
    async fn hits_recharge_balance_endpoint_with_bearer() {
        let mock = MockHttp::ok(r#"{"available_balance": 1.0}"#);
        Bailian
            .query(&creds(), &mock, PlanVariant::Auto)
            .await
            .unwrap();
        let reqs = mock.captured_requests();
        assert_eq!(reqs.len(), 1);
        let req = &reqs[0];
        assert_eq!(
            req.url,
            "https://dashscope.aliyuncs.com/api/v1/recharge/recharge-balance/query"
        );
        let auth = req
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.as_str());
        assert_eq!(auth, Some("Bearer sk-bailian-test"));
    }
}
