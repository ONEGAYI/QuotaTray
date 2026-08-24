//! 错误详情脱敏：响应体片段进入错误 detail/message 前的凭据清洗。
//!
//! 清洗方案参考 opencode（`packages/llm/src/route/executor.ts`）的两遍设计：
//! 1. 结构化正则：按字段名形态打码（`"api_key": "v"` 与 `token=v` 两种形态，
//!    字段名保留、仅替换值）；
//! 2. 字面量替换：用本次请求实际携带的密钥值做精确替换，对抗服务端在
//!    错误响应中回显请求凭据。密钥来源三路：敏感头（含 Bearer 剥壳）、
//!    URL query 敏感参数、模板执行器显式登记的 [`HttpRequest::declared_secrets`]
//!    （模板 DSL 允许把 apiKey 替换进任意自定义头/参数，按名猜不可靠，
//!    故由执行器从根登记）。各来源均附 URL-encoded 形式与稳定前缀
//!    （对抗服务端只回显 key 前段的场景）。
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
const DETAIL_BODY_LIMIT: usize = 2048;

/// 密钥稳定前缀的最小密钥长度与前缀长度：服务端只回显 key 前段
/// （如前 20 字符）时，前缀字面量兜底打码。前缀足够长（12+），
/// 在普通文本中误伤概率可忽略。
const PREFIX_MIN_SECRET_LEN: usize = 16;
const PREFIX_LEN: usize = 12;

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
            r#"(?i)("{}"\s*:\s*)"[^"]*""#,
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
            sensitive_body_field_source()
        ))
        .expect("query 字段打码正则合法")
    });
    &RE
}

/// header 名是否敏感：Debug 打码与密钥收集共用的子串判断。
///
/// 子串匹配（含 `X-Trace-Token` 等复合头名）是有意的保守放宽：
/// 宁可多打码不可漏打码，方向上只增不减。
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

/// 从本次请求收集真实密钥值（含 Bearer 剥壳、URL-encoded 与稳定前缀形态）。
///
/// - 敏感名头的值整体 + `Bearer ` 剥壳后的 token（cookie 除外：
///   整串 cookie 替换易误伤 body 中的普通子串，且余额类 API 罕用 cookie 认证）；
/// - URL query 中敏感名参数的值；
/// - 模板执行器显式登记的 [`HttpRequest::declared_secrets`]（模板 DSL
///   允许把 apiKey 替换进任意自定义头/参数，按名猜不可靠）；
/// - 长度 < 4 字符的值跳过（过短字面量替换会产生大量误伤）；
/// - 长度 ≥ [`PREFIX_MIN_SECRET_LEN`] 的值追加稳定前缀（对抗服务端
///   只回显 key 前段的场景）。
fn secret_values(req: &HttpRequest) -> Vec<String> {
    let mut set = BTreeSet::new();
    let mut add = |value: &str| {
        let chars = value.chars().count();
        if chars < 4 {
            return;
        }
        set.insert(value.to_string());
        // 近似 encodeURIComponent：多数密钥字符集（字母数字 -_.~）下等价
        let encoded: String = url::form_urlencoded::byte_serialize(value.as_bytes()).collect();
        if encoded != value {
            set.insert(encoded);
        }
        // 稳定前缀：服务端只回显 key 前段（如前 20 字符）时的兜底
        if chars >= PREFIX_MIN_SECRET_LEN {
            let prefix: String = value.chars().take(PREFIX_LEN).collect();
            set.insert(prefix);
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

    for declared in &req.declared_secrets {
        add(declared);
    }

    // 按字符长度降序替换：整值必须先于其前缀（BTreeSet 字典序里前缀
    // 恰好排在整值之前，先替换前缀会打断整值匹配、残留密钥后半段）。
    // 已知边界：服务端主动截断回显（只回显 key 前段）时，截断点之后
    // 超出前缀长度的片段仍可能残留——不足以重建完整 key，接受。
    let mut values: Vec<String> = set.into_iter().collect();
    values.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
    values
}

/// `Bearer <token>` 剥壳（scheme 大小写不敏感）。
fn bearer_token(header_value: &str) -> Option<&str> {
    let mut parts = header_value.splitn(2, char::is_whitespace);
    let scheme = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = parts.next()?.trim();
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

    /// 契约：大写/混合大小写字段名形态同样打码（PascalCase 风格 API 现实存在）。
    #[test]
    fn uppercase_field_shapes_are_redacted() {
        let req = HttpRequest::get("https://api.example.com");
        let body = r#"{"API_KEY":"CASESECRET123","Api-Key":"case2secret","Token":"tok"}"#;
        let out = redact_body(body, &req);
        assert!(!out.contains("CASESECRET123"), "大写字段值泄漏：{out}");
        assert!(!out.contains("case2secret"), "混合大小写泄漏：{out}");
        assert!(!out.contains("\"tok\""), "大写 Token 值泄漏：{out}");
    }

    /// 契约：裸 `key=` query 形态打码（HTML 错误页回显 `?key=` 链接的常见形态）；
    /// 词边界仍防 `monkey=` 误伤。
    #[test]
    fn bare_key_query_shape_is_redacted() {
        let req = HttpRequest::get("https://api.example.com");
        let out = redact_body("see ?key=short1234 & monkey=zzz", &req);
        assert!(!out.contains("short1234"), "key= 值泄漏：{out}");
        assert!(out.contains("monkey=zzz"), "非敏感名不应误伤：{out}");
    }

    /// 契约：服务端只回显密钥前段（如前 20 字符）时，稳定前缀兜底打码。
    #[test]
    fn partial_prefix_echo_is_redacted() {
        let req = HttpRequest::get("https://api.example.com").bearer("sk-live-secret-000111222333");
        let out = redact_body("bad key sk-live-secret-00011 (first 20 chars)", &req);
        assert!(!out.contains("sk-live-secret-00011"), "前段回显泄漏：{out}");
    }

    /// 契约（回归）：完整密钥回显时整值先于前缀替换——密钥后半段
    /// （机密主体）不得因前缀抢先替换而残留。
    #[test]
    fn full_echo_redacts_entire_secret_not_prefix_only() {
        let req = HttpRequest::get("https://api.example.com").bearer("sk-live-secret-000111222333");
        let out = redact_body("echo sk-live-secret-000111222333 here", &req);
        assert!(
            !out.contains("sk-live-secret-000111222333"),
            "整值泄漏：{out}"
        );
        assert!(
            !out.contains("-000111222333"),
            "后半段残留（前缀先替换回归）：{out}"
        );
        assert_eq!(out, "echo <redacted> here");
    }

    /// 契约：Bearer scheme 任意大小写均可剥壳（BEARER/bEaReR）。
    #[test]
    fn bearer_scheme_is_case_insensitive() {
        let req = HttpRequest::get("https://api.example.com")
            .header("Authorization", "BEARER sk-upper-case-secret");
        let out = redact_body("echo sk-upper-case-secret", &req);
        assert!(
            !out.contains("sk-upper-case-secret"),
            "BEARER 剥壳失败：{out}"
        );
    }

    /// 契约：模板登记的 declared_secrets 参与字面量替换——apiKey 被替换进
    /// 任意自定义头/参数（敏感名判断覆盖不到）时，整值回显同样打码。
    #[test]
    fn declared_secrets_are_redacted() {
        let mut req =
            HttpRequest::get("https://api.example.com").header("X-Custom-Auth", "anything");
        req.declared_secrets
            .push("custom-position-secret-99".into());
        let out = redact_body("echo custom-position-secret-99 here", &req);
        assert!(
            !out.contains("custom-position-secret-99"),
            "登记密钥泄漏：{out}"
        );
    }
}
