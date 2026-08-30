//! 阿里云账户余额查询（BssOpenApi QueryAccountBalance，RPC V1 签名）。
//!
//! 端点 `GET https://business.aliyuncs.com/?Action=QueryAccountBalance&Version=2017-12-14`。
//! 凭据为阿里云 RAM AccessKey：`api_key`=AccessKey ID、`api_key2`=AccessKey
//! Secret——native 平台中首个双凭据用户（引擎已整包传入，缺失时此处
//! 引导补配）。口径为**阿里云账户级余额**：百炼按量计费共享该额度，
//! 账户下其他云产品的未结清欠费同样计入（字段 AvailableAmount/Currency）。
//! 预研与实测记录：`docs/预研文档/2026-08-30 百炼余额查询预研.md`。

use async_trait::async_trait;
use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha1::Sha1;

use super::{NativeMeta, NativeProvider, parse_error, parse_num, redact_error_message};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

const ENDPOINT: &str = "https://business.aliyuncs.com/";
const API_VERSION: &str = "2017-12-14";

pub struct AliyunBss;

#[async_trait]
impl NativeProvider for AliyunBss {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "aliyun_bss",
            name: "阿里云余额",
            console_url: Some("https://expense.console.aliyun.com/"),
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let secret = creds.api_key2.as_deref().ok_or_else(|| {
            QueryError::deterministic(
                "缺少 AccessKey Secret：请在第二凭据槽填写（第一凭据槽为 AccessKey ID）",
            )
        })?;
        let url = signed_query_url(&common_params(&creds.api_key), secret);
        let mut req = HttpRequest::get(&url).header("Accept", "application/json");
        // Secret 不进请求任何位置，但登记进脱敏清单兜底：错误体若被网关
        // 回显（含派生信息）一并打码（fetch_json_relogin 同款语义）。
        req.declared_secrets.push(secret.to_string());

        let resp = http.execute(req.clone()).await.map_err(|e| match &e {
            crate::http::HttpError::Timeout | crate::http::HttpError::Network(_) => {
                QueryError::transient(e.to_string())
            }
            crate::http::HttpError::InvalidRequest(_) => QueryError::deterministic(e.to_string()),
        })?;
        let body = if resp.body.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&resp.body).map_err(|e| {
                redact_error_message(
                    QueryError::deterministic("响应不是合法 JSON").with_detail(format!(
                        "JSON 解析错误：{e}\n响应体（已脱敏）：\n{}",
                        crate::http::redact::redact_and_truncate(&resp.body, &req)
                    )),
                    &req,
                )
            })?
        };

        // 阿里云把业务成败放在 body 的 Code 字段（成功恒 "200"），HTTP 状态
        // 与之不一一对应（NotAuthorized 实测为 400），两类失败统一走分类。
        let code = body.get("Code").and_then(Value::as_str).unwrap_or("");
        if !resp.is_success() || code != "200" {
            let message = body.get("Message").and_then(Value::as_str).unwrap_or("");
            let err = classify_error_code(code, message, resp.status);
            let err = match body.get("RequestId").and_then(Value::as_str) {
                Some(id) => err.with_detail(format!("RequestId：{id}")),
                None => err,
            };
            return Err(redact_error_message(err, &req));
        }

        let data = body
            .get("Data")
            .ok_or_else(|| parse_error("阿里云余额", "Data 对象"))?;
        let remaining = parse_num(data.get("AvailableAmount"))
            .ok_or_else(|| parse_error("阿里云余额", "Data.AvailableAmount 数值"))?;
        // 账户级余额如实展示：欠费为负值也保留（is_valid 恒 true，
        // 「没钱」不是凭据失效）。
        let unit = data
            .get("Currency")
            .and_then(Value::as_str)
            .unwrap_or("CNY")
            .to_uppercase();
        Ok(vec![UsageData {
            plan_name: Some("阿里云余额".into()),
            total: None,
            used: None,
            remaining: Some(remaining),
            unit: Some(unit),
            reset_at: None,
            is_valid: Some(true),
            invalid_message: None,
            extra: None,
        }])
    }
}

// ---- RPC V1 签名（纯函数，官方向量锁定） --------------------------------

/// 阿里云 percentEncode：RFC3986 子集——字母数字与 `-_.~` 保留，空格
/// 编码为 `%20`（非 `+`）、`*` 编码为 `%2A`，其余字节（含 UTF-8 多字节）
/// 逐字节 `%XY` 大写十六进制。
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// RPC V1 签名：参数按 key 字典序排序（本项目参数 key 唯一，元组排序即
/// 满足规范）→ 规范化查询串 → `GET&%2F&percentEncode(串)` →
/// `Base64(HMAC-SHA1(Secret+"&", ...))`。
fn sign_query(params: &[(String, String)], secret: &str) -> String {
    let mut sorted: Vec<_> = params.to_vec();
    sorted.sort();
    let canonical = sorted
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let string_to_sign = format!("GET&{}&{}", percent_encode("/"), percent_encode(&canonical));
    let mut mac = Hmac::<Sha1>::new_from_slice(format!("{secret}&").as_bytes())
        .expect("HMAC-SHA1 接受任意长度密钥");
    mac.update(string_to_sign.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
}

/// 签名后拼出完整请求 URL（含 Signature 参数，整体按 key 排序保证输出
/// 稳定，便于测试断言）。
fn signed_query_url(params: &[(String, String)], secret: &str) -> String {
    let signature = sign_query(params, secret);
    let mut all: Vec<_> = params.to_vec();
    all.push(("Signature".into(), signature));
    all.sort();
    let qs = all
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{ENDPOINT}?{qs}")
}

/// RPC 公共参数 + 本接口业务参数（无业务入参）。
fn common_params(access_key_id: &str) -> Vec<(String, String)> {
    vec![
        ("Action".into(), "QueryAccountBalance".into()),
        ("Version".into(), API_VERSION.into()),
        ("Format".into(), "JSON".into()),
        ("AccessKeyId".into(), access_key_id.into()),
        ("SignatureMethod".into(), "HMAC-SHA1".into()),
        ("SignatureVersion".into(), "1.0".into()),
        ("SignatureNonce".into(), random_hex(16)),
        (
            "Timestamp".into(),
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        ),
    ]
}

/// 随机 hex（SignatureNonce 防重放）：系统 CSPRNG 取字节，失败即确定性
/// 报错——不做弱随机兜底（重放防护降级无提示更糟）。
fn random_hex(n_bytes: usize) -> String {
    let mut buf = vec![0u8; n_bytes];
    getrandom::fill(&mut buf).expect("系统随机源不可用");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// 阿里云错误码 → 错误双轨分类：
/// 授权/身份/签名类 = 确定性（重试无意义，附中文引导）；
/// `Throttling*` = 瞬时（可重试）；未知码透传原始 code/message（调用方
/// 统一过脱敏）。
fn classify_error_code(code: &str, message: &str, status: u16) -> QueryError {
    match code {
        "NotAuthorized" => QueryError::deterministic(
            "RAM 未授权：请为该 AccessKey 所属 RAM 用户授予 AliyunBSSReadOnlyAccess 权限",
        ),
        "InvalidAccessKeyId.NotFound" => {
            QueryError::deterministic("AccessKey ID 无效：确认复制完整且未被禁用或删除")
        }
        "SignatureDoesNotMatch" => {
            QueryError::deterministic("签名不匹配：AccessKey Secret 有误或复制不完整")
        }
        c if c.starts_with("Throttling") => {
            let msg = if message.trim().is_empty() {
                format!("HTTP {status}")
            } else {
                message.trim().chars().take(120).collect()
            };
            QueryError::transient(format!("触发阿里云限流：{msg}"))
        }
        _ => {
            QueryError::deterministic(format!("阿里云返回错误 {code}（HTTP {status}）：{message}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    fn creds() -> Credentials {
        Credentials::new("LTAI-test-id").with_api_key2("test-secret")
    }

    async fn query_with(mock: MockHttp) -> Result<Vec<UsageData>, QueryError> {
        AliyunBss.query(&creds(), &mock, PlanVariant::Auto).await
    }

    /// 契约：RPC V1 签名与官方文档向量一致
    /// （https://help.aliyun.com/zh/sdk/product-overview/rpc-mechanism 示例）。
    #[test]
    fn sign_query_matches_official_vector() {
        let params: Vec<(String, String)> = [
            ("AccessKeyId", "testid"),
            ("Action", "DescribeDedicatedHosts"),
            ("Format", "JSON"),
            ("RegionId", "cn-beijing"),
            ("SignatureMethod", "HMAC-SHA1"),
            ("SignatureNonce", "edb2b34af0af9a6d14deaf7c1a5315eb"),
            ("SignatureVersion", "1.0"),
            ("Timestamp", "2023-03-13T08:34:30Z"),
            ("Version", "2014-05-26"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        assert_eq!(
            sign_query(&params, "testsecret"),
            "9NaGiOspFP5UPcwX8Iwt2YJXXuk="
        );
    }

    /// 契约：percentEncode 遵循 RFC3986 子集（空格 %20、* %2A、~ 保留、
    /// 冒号编码、中文多字节）。
    #[test]
    fn percent_encode_rules() {
        assert_eq!(percent_encode("a b"), "a%20b");
        assert_eq!(percent_encode("1*2"), "1%2A2");
        assert_eq!(percent_encode("~._-zA9"), "~._-zA9");
        assert_eq!(percent_encode("08:34:30Z"), "08%3A34%3A30Z");
        assert_eq!(percent_encode("杭州"), "%E6%9D%AD%E5%B7%9E");
    }

    /// 契约：请求 URL 指向正确端点、携带 Action/Version 与签名参数；
    /// AccessKeyId 明文入 query（阿里云协议如此，ID 本身非机密）。
    #[tokio::test]
    async fn request_url_carries_action_and_signature() {
        let mock = MockHttp::ok(success_body());
        let _ = AliyunBss.query(&creds(), &mock, PlanVariant::Auto).await;
        let reqs = mock.captured_requests();
        assert_eq!(reqs.len(), 1);
        let url = &reqs[0].url;
        assert!(
            url.starts_with("https://business.aliyuncs.com/?"),
            "端点错误：{url}"
        );
        for fragment in [
            "Action=QueryAccountBalance",
            "Version=2017-12-14",
            "SignatureMethod=HMAC-SHA1",
            "Signature=",
            "AccessKeyId=LTAI-test-id",
        ] {
            assert!(url.contains(fragment), "缺少 {fragment}：{url}");
        }
    }

    /// 契约：实测成功响应结构（2026-08-30 脱敏样例）→ 单条余额数据，
    /// 可用额度与币种取自 Data，is_valid 恒 true。
    #[tokio::test]
    async fn parses_success_response() {
        let data = query_with(MockHttp::ok(success_body())).await.unwrap();
        assert_eq!(
            data,
            vec![UsageData {
                plan_name: Some("阿里云余额".into()),
                total: None,
                used: None,
                remaining: Some(28.35),
                unit: Some("CNY".into()),
                reset_at: None,
                is_valid: Some(true),
                invalid_message: None,
                extra: None,
            }]
        );
    }

    /// 欠费为负值时如实保留（「没钱」不是凭据失效）。
    #[tokio::test]
    async fn negative_balance_kept_as_is() {
        let body =
            r#"{"Code":"200","Success":true,"Data":{"AvailableAmount":"-3.50","Currency":"CNY"}}"#;
        let data = query_with(MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].remaining, Some(-3.5));
        assert_eq!(data[0].is_valid, Some(true));
    }

    /// Currency 缺失回退 CNY；小写归一为大写。
    #[tokio::test]
    async fn currency_defaults_and_normalizes() {
        let no_ccy = r#"{"Code":"200","Data":{"AvailableAmount":"1.00"}}"#;
        assert_eq!(
            query_with(MockHttp::ok(no_ccy)).await.unwrap()[0].unit,
            Some("CNY".into())
        );
        let lower = r#"{"Code":"200","Data":{"AvailableAmount":"2.00","Currency":"cny"}}"#;
        assert_eq!(
            query_with(MockHttp::ok(lower)).await.unwrap()[0].unit,
            Some("CNY".into())
        );
    }

    /// 契约：NotAuthorized（实测 HTTP 400 + body Code）→ 确定性失败，
    /// 文案引导授权；detail 附 RequestId 便于排查。
    #[tokio::test]
    async fn not_authorized_is_deterministic_with_hint() {
        let body = r#"{"Code":"NotAuthorized","Message":"This API is not authorized for caller.","RequestId":"req-1"}"#;
        let err = query_with(MockHttp::status_body(400, body))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(
            err.message().contains("AliyunBSSReadOnlyAccess"),
            "文案：{}",
            err.message()
        );
        assert!(err.detail().unwrap_or_default().contains("req-1"));
    }

    /// AccessKey ID / Secret 错误 → 确定性 + 中文引导。
    #[tokio::test]
    async fn identity_errors_are_deterministic() {
        let cases = [
            ("InvalidAccessKeyId.NotFound", "invalid id"),
            ("SignatureDoesNotMatch", "signature mismatch"),
        ];
        for (code, msg) in cases {
            let body = format!(r#"{{"Code":"{code}","Message":"{msg}"}}"#);
            let err = query_with(MockHttp::status_body(400, &body))
                .await
                .unwrap_err();
            assert!(!err.is_transient(), "{code} 应为确定性失败");
            assert!(!err.message().is_empty());
        }
    }

    /// Throttling 前缀（Throttling / Throttling.User 等）→ 瞬时失败。
    #[tokio::test]
    async fn throttling_is_transient() {
        for code in ["Throttling", "Throttling.User"] {
            let body = format!(r#"{{"Code":"{code}","Message":"Requests too frequent"}}"#);
            let err = query_with(MockHttp::status_body(400, &body))
                .await
                .unwrap_err();
            assert!(err.is_transient(), "{code} 应为瞬时失败");
        }
    }

    /// 未知错误码：确定性透传原始 code（HTTP 状态无关）。
    #[tokio::test]
    async fn unknown_code_passes_through() {
        let body = r#"{"Code":"InternalError.PartialError","Message":"boom"}"#;
        let err = query_with(MockHttp::status_body(200, body))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("InternalError.PartialError"));
    }

    /// 缺少第二凭据（AccessKey Secret）→ 确定性失败并引导补配，
    /// 不发任何请求。
    #[tokio::test]
    async fn missing_api_key2_is_deterministic() {
        let mock = MockHttp::ok(success_body());
        let err = AliyunBss
            .query(&Credentials::new("LTAI-test-id"), &mock, PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("AccessKey Secret"));
        assert!(mock.captured_requests().is_empty(), "不应发出请求");
    }

    /// Data/AvailableAmount 缺失或非数值 → 确定性失败（不兜底 0）。
    #[tokio::test]
    async fn missing_fields_are_deterministic() {
        for body in [
            r#"{"Code":"200"}"#,
            r#"{"Code":"200","Data":{}}"#,
            r#"{"Code":"200","Data":{"AvailableAmount":"x"}}"#,
        ] {
            let err = query_with(MockHttp::ok(body)).await.unwrap_err();
            assert!(!err.is_transient(), "body {body} 应为确定性失败");
        }
    }

    /// 网络故障/超时 → 瞬时失败；响应非 JSON → 确定性失败。
    #[tokio::test]
    async fn transport_errors_classified() {
        assert!(
            query_with(MockHttp::fail())
                .await
                .unwrap_err()
                .is_transient()
        );
        let err = query_with(MockHttp::ok("<html>gateway</html>"))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }

    fn success_body() -> &'static str {
        r#"{
            "Code": "200",
            "Success": true,
            "Data": {
                "AvailableAmount": "28.35",
                "AvailableCashAmount": "28.35",
                "CreditAmount": "0.00",
                "MybankCreditAmount": "0.00",
                "QuotaLimit": "0.00",
                "Currency": "CNY"
            }
        }"#
    }
}
