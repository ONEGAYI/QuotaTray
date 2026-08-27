//! SiliconFlow 余额查询，国内/国际双站（端点路径相同，仅域名与币种不同）。
//!
//! `GET {base}/v1/user/info`（Bearer api key）
//! 响应：`{"code": 20000, "data": {"totalBalance": "42.50", ...}}`
//! 文档：docs.siliconflow.cn / docs.siliconflow.com（双站账户独立）。
//!
//! 国内站该接口已被官方停止服务（2026-08-14 起 HTTP 410，替代 API 未发布，
//! 见 AGENTS.md「外部接口停用追踪」），410 转译为止血提示。

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
    /// 410 止血文案；仅国内站设置，国际站 None 维持通用 HTTP 错误路径。
    deprecated_410_notice: Option<&'static str>,
}

/// 国内站（api.siliconflow.cn，人民币）。
pub const SILICONFLOW_CN: SiliconFlow = SiliconFlow {
    id: "siliconflow",
    name: "SiliconFlow",
    base_url: "https://api.siliconflow.cn",
    unit: "CNY",
    deprecated_410_notice: Some(
        "SiliconFlow 国内站余额接口已由官方停止服务，暂无替代 API；这不表示 API Key 无效。",
    ),
};

/// 国际站（api.siliconflow.com，美元——官方文档未标注余额单位，
/// 依国际站整体 USD 计价推断）。
pub const SILICONFLOW_GLOBAL: SiliconFlow = SiliconFlow {
    id: "siliconflow_global",
    name: "SiliconFlow（国际站）",
    base_url: "https://api.siliconflow.com",
    unit: "USD",
    deprecated_410_notice: None,
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
        let body = match fetch_json(http, req).await {
            Ok(v) => v,
            Err(e) => {
                // 止血特判（仅国内站）：410 是官方废弃的终态，转译为明确
                // 提示以免误判 Key 无效；原始 410 detail 保留供排查。
                // 前缀匹配耦合 status_error_with_body 的 "HTTP {status}" 格式
                //（状态码至多三位数，"410" 后只会是结尾或冒号）。
                if let Some(notice) = self.deprecated_410_notice {
                    if e.message().starts_with("HTTP 410") {
                        let mut hint = QueryError::deterministic(notice.to_string());
                        if let Some(d) = e.detail() {
                            hint = hint.with_detail(d);
                        }
                        return Err(hint);
                    }
                }
                return Err(e);
            }
        };

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

    /// 410 响应体（issue #50 实录形态：code 20092，无 error 字段）。
    fn gone_body() -> &'static str {
        r#"{"code":20092,"message":"This endpoint is deprecated and is no longer available.","data":null}"#
    }

    /// 止血契约（issue #50）：国内站 /v1/user/info 官方废弃后 410，
    /// 错误转译为明确提示——说明接口停止服务且不代表 API Key 无效，
    /// 原始 410 细节保留在 detail 供排查。
    #[tokio::test]
    async fn cn_410_translated_to_deprecation_notice() {
        let err = query_with(MockHttp::status_body(410, gone_body()))
            .await
            .unwrap_err();
        assert!(!err.is_transient(), "接口废弃是终态，不应可重试");
        let msg = err.message();
        assert!(msg.contains("已由官方停止服务"), "实际：{msg}");
        assert!(msg.contains("这不表示 API Key 无效"), "实际：{msg}");
        assert!(!msg.contains("HTTP 410"), "主文案应替换而非追加：{msg}");
        let detail = err.detail().unwrap_or_default();
        assert!(
            detail.contains("20092"),
            "原始响应体应保留在 detail：{detail}"
        );
    }

    /// 止血契约：特判仅限国内站——国际站 410 仍走通用 HTTP 错误路径。
    #[tokio::test]
    async fn global_410_keeps_plain_http_error() {
        let err = SILICONFLOW_GLOBAL
            .query(
                &creds(),
                &MockHttp::status_body(410, gone_body()),
                PlanVariant::Auto,
            )
            .await
            .unwrap_err();
        let msg = err.message();
        assert!(msg.contains("HTTP 410"), "实际：{msg}");
        assert!(
            !msg.contains("停止服务"),
            "国际站不应出现国内站止血文案：{msg}"
        );
    }

    /// 止血契约：只特判 410——国内站其他 4xx（如 401）不触发止血文案。
    #[tokio::test]
    async fn cn_other_status_not_translated() {
        let err = query_with(MockHttp::status(401)).await.unwrap_err();
        let msg = err.message();
        assert!(msg.contains("HTTP 401"), "实际：{msg}");
        assert!(!msg.contains("停止服务"), "非 410 不应误伤：{msg}");
    }
}
