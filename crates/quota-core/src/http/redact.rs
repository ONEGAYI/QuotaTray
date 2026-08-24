//! 错误详情脱敏：响应体片段进入错误 detail 前的凭据清洗。
//!
//! 清洗方案参考 opencode（`packages/llm/src/route/executor.ts`）的两遍设计：
//! 1. 结构化正则：按字段名形态打码（`"api_key": "v"` 与 `token=v` 两种形态，
//!    字段名保留、仅替换值）；
//! 2. 字面量替换：用本次请求实际携带的密钥值（敏感头含 Bearer 剥壳、
//!    URL query 敏感参数，及各自的 URL-encoded 形式）做精确替换，
//!    对抗服务端在错误响应中回显请求凭据。
//!
//! 截断永远发生在清洗之后——保证保留窗口内不残留半截密钥。
//! 与 opencode 的差异：Rust regex 不支持 lookbehind，`key=` 防误伤
//! （如 `monkey=`）改用 `\b` 词边界实现，语义等价。

use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;
use url::Url;

use super::HttpRequest;

/// 敏感名清单（大小写不敏感；`[-_]?` 兼容 api-key/api_key/apikey 写法）。
/// 头名判断、URL query 参数名、body 字段名共用这一份 source；
/// `key` 作为过短泛化名只参与 body 字段/query 形态匹配（有定界防误伤），
/// 不参与头名子串判断。
const SENSITIVE_NAME_SOURCE: &str = "authorization|proxy-authorization|cookie|api[-_]?key|access[-_]?token|refresh[-_]?token|id[-_]?token|token|secret|credential|signature|x-amz-signature";

/// 错误详情中响应体的最大保留长度（按字符计，中文安全）。
pub(crate) const DETAIL_BODY_LIMIT: usize = 2048;

const REDACTED: &str = "<redacted>";

fn sensitive_name_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!("(?i){SENSITIVE_NAME_SOURCE}")).expect("敏感名正则合法")
    });
    &RE
}

/// body 字段形态（含 query/kv 嵌入形态）的匹配 source：敏感名 + 裸 `key`。
fn sensitive_body_field_source() -> String {
    format!("(?:{SENSITIVE_NAME_SOURCE}|key)")
}

fn redact_json_fields_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        // 组内含字段名两侧引号（opencode 同构），保证替换后引号完整保留
        Regex::new(&format!(
            r#"("{}"\s*:\s*)"[^"]*""#,
            sensitive_body_field_source()
        ))
        .expect("JSON 字段打码正则合法")
    });
    &RE
}

fn redact_query_fields_re() -> &'static Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r#"(?i)\b((?:{})=)[^&\s"']+"#,
            SENSITIVE_NAME_SOURCE
        ))
        .expect("query 字段打码正则合法")
    });
    &RE
}

/// header 名是否敏感：Debug 打码与密钥收集共用的子串判断。
pub(crate) fn is_sensitive_header(name: &str) -> bool {
    sensitive_name_re().is_match(name)
}

/// URL query 参数名是否敏感：头名判断 + 锚定的短泛化名（`key`/`sig`）。
fn is_sensitive_query_name(name: &str) -> bool {
    is_sensitive_header(name) || matches!(name, "key" | "sig")
}

/// 两遍清洗：先结构化正则，再用本次请求的真实密钥值做字面量替换。
pub(crate) fn redact_body(body: &str, req: &HttpRequest) -> String {
    let mut text = redact_json_fields_re()
        .replace_all(body, format!(r#"$1"{REDACTED}""#))
        .into_owned();
    text = redact_query_fields_re()
        .replace_all(&text, format!("$1{REDACTED}"))
        .into_owned();
    for secret in secret_values(req) {
        text = text.replace(&secret, REDACTED);
    }
    text
}

/// 清洗后截断到 [`DETAIL_BODY_LIMIT`] 字符，截断时追加尾标。
pub(crate) fn redact_and_truncate(body: &str, req: &HttpRequest) -> String {
    let redacted = redact_body(body, req);
    let total = redacted.chars().count();
    if total <= DETAIL_BODY_LIMIT {
        return redacted;
    }
    let prefix: String = redacted.chars().take(DETAIL_BODY_LIMIT).collect();
    format!("{prefix}\n…（已截断，响应体共 {total} 字符）")
}

/// 从本次请求收集真实密钥值（含 Bearer 剥壳与 URL-encoded 形式）。
///
/// - 敏感名头的值整体 + `Bearer ` 剥壳后的 token（cookie 除外：
///   整串 cookie 替换易误伤 body 中的普通子串，且余额类 API 罕用 cookie 认证）；
/// - URL query 中敏感名参数的值；
/// - 长度 < 4 字符的值跳过（过短字面量替换会产生大量误伤）。
fn secret_values(req: &HttpRequest) -> Vec<String> {
    let mut set = BTreeSet::new();
    let mut add = |value: &str| {
        if value.chars().count() < 4 {
            return;
        }
        set.insert(value.to_string());
        // 近似 encodeURIComponent：多数密钥字符集（字母数字 -_.~）下等价
        let encoded: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
        if encoded != value {
            set.insert(encoded);
        }
    };

    for (name, value) in &req.headers {
        if !is_sensitive_header(name) || name.eq_ignore_ascii_case("cookie") {
            continue;
        }
        add(value);
        if let Some(token) = bearer_token(value) {
            add(token);
        }
    }

    if let Ok(url) = Url::parse(&req.url) {
        for (name, value) in url.query_pairs() {
            if is_sensitive_query_name(&name) {
                add(&value);
            }
        }
    }

    set.into_iter().collect()
}

/// `Bearer <token>` 剥壳（大小写不敏感）。
fn bearer_token(header_value: &str) -> Option<&str> {
    let rest = header_value
        .strip_prefix("Bearer")
        .or_else(|| header_value.strip_prefix("bearer"))?;
    let token = rest.trim_start();
    if token.is_empty() { None } else { Some(token) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：JSON 字段形态——多写法字段名（api_key/api-key/apikey）打码、
    /// 字段名保留可读、非敏感字段不受影响。
    #[test]
    fn json_field_shapes_are_redacted() {
        let req = HttpRequest::get("https://api.example.com/v1/balance");
        let body = r#"{"api_key":"sk-live-secret","api-key":"aaa","apikey":"bbb","access_token":"ttt","data":{"balance":5}}"#;
        let out = redact_body(body, &req);
        assert!(!out.contains("sk-live-secret"), "api_key 值泄漏：{out}");
        assert!(!out.contains("aaa"), "api-key 值泄漏：{out}");
        assert!(!out.contains("bbb"), "apikey 值泄漏：{out}");
        assert!(!out.contains("ttt"), "access_token 值泄漏：{out}");
        assert!(out.contains("\"api_key\""), "字段名应保留：{out}");
        assert!(out.contains("\"balance\":5"), "非敏感字段不应改动：{out}");
    }

    /// 契约：query/kv 形态打码；词边界防止 `monkey=` 类前缀误伤。
    #[test]
    fn query_field_shape_is_redacted_without_false_positive() {
        let req = HttpRequest::get("https://api.example.com");
        let out = redact_body("token=abc123&monkey=zzz&signature=sig456", &req);
        assert!(!out.contains("abc123"), "token 值泄漏：{out}");
        assert!(!out.contains("sig456"), "signature 值泄漏：{out}");
        assert!(out.contains("monkey=zzz"), "非敏感名不应误伤：{out}");
    }

    /// 契约：服务端回显请求密钥 → 字面量替换打码（整值与 Bearer 剥壳都覆盖）。
    #[test]
    fn echoed_request_secret_is_redacted() {
        let req = HttpRequest::get("https://api.example.com").bearer("sk-plain-secret-123");
        let body = r#"{"error":{"message":"invalid key sk-plain-secret-123 provided","header":"Bearer sk-plain-secret-123"}}"#;
        let out = redact_body(body, &req);
        assert!(!out.contains("sk-plain-secret-123"), "回显密钥泄漏：{out}");
        assert!(
            out.contains("invalid key <redacted>"),
            "应保留上下文：{out}"
        );
    }

    /// 契约：URL query 中敏感参数值参与密钥收集，body 回显同样打码。
    #[test]
    fn url_query_secret_is_collected() {
        let req = HttpRequest::get("https://api.example.com/balance?token=urlsecret999&other=1");
        let out = redact_body("echo urlsecret999 here", &req);
        assert!(!out.contains("urlsecret999"), "URL query 密钥泄漏：{out}");
    }

    /// 契约：< 4 字符的短值跳过字面量替换（避免普通子串大面积误伤）。
    #[test]
    fn short_values_are_skipped() {
        let req = HttpRequest::get("https://api.example.com").bearer("abc");
        let out = redact_body("alphabet abc alphabet", &req);
        assert_eq!(out, "alphabet abc alphabet");
    }

    /// 契约：URL-encoded 形式的密钥同样命中（服务端可能转义后回显）。
    #[test]
    fn encoded_form_is_redacted() {
        let req = HttpRequest::get("https://api.example.com").bearer("sk+se/cret=");
        let out = redact_body("echo sk%2Bse%2Fcret%3D and sk+se/cret=", &req);
        assert!(
            !out.contains("sk%2Bse%2Fcret%3D"),
            "encoded 密钥泄漏：{out}"
        );
        assert!(!out.contains("sk+se/cret="), "原文密钥泄漏：{out}");
    }

    /// 契约：先清洗后截断——密钥横跨截断边界时窗口内只有 `<redacted>`，
    /// 不残留半截密钥；未超长时不加尾标。
    #[test]
    fn truncation_happens_after_redaction() {
        let req = HttpRequest::get("https://api.example.com").bearer("sk-cross-boundary-secret");
        let mut body = "x".repeat(2020);
        body.push_str(" sk-cross-boundary-secret ");
        body.push_str(&"y".repeat(500));
        let out = redact_and_truncate(&body, &req);
        assert!(!out.contains("sk-cross-boundary"), "半截密钥泄漏：{out}");
        assert!(out.contains("<redacted>"), "边界处应为已打码值：{out}");
        assert!(out.contains("已截断"), "应带截断尾标：{out}");

        let short = redact_and_truncate("ok", &req);
        assert_eq!(short, "ok", "未超长不应加尾标");
    }
}
