//! HTTP 访问抽象：Provider 实现只依赖 [`HttpClient`] trait，
//! 单测注入 mock，生产使用 reqwest 实现。

pub mod redact;
pub mod reqwest;

pub use self::reqwest::ReqwestHttpClient;

use async_trait::async_trait;

/// 请求方法。M1 仅需 GET/POST（声明式模板与脚本引擎后续按需扩展）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

/// 与具体 HTTP 栈无关的请求描述。
#[derive(Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// 执行器显式登记的明文凭据（模板 DSL 允许把 apiKey 替换进任意自定义
    /// 头/参数，按敏感名猜不可靠，由模板执行器从根登记）。
    /// 仅供错误详情脱敏的字面量替换使用；不参与 Debug 输出与实际请求。
    pub(crate) declared_secrets: Vec<String>,
}

// 手写 Debug：敏感头与 URL query 打码，防止请求日志泄漏凭据
//（M2 模板支持自定义 URL 后，用户可能把 key 写进 query string；
// declared_secrets 与 body 中的模板替换结果同理不打码清单，均不输出）。
impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &mask_url_query(&self.url))
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(k, v)| {
                        (
                            k,
                            if redact::is_sensitive_header(k) {
                                "***"
                            } else {
                                v.as_str()
                            },
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .field("body", &self.body)
            .finish()
    }
}

/// URL 的 query 部分整体打码（参数名也不保留——key 本身可能出现在参数名中）。
fn mask_url_query(url: &str) -> String {
    match url.split_once('?') {
        None => url.to_string(),
        Some((base, _)) => format!("{base}?***"),
    }
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
            declared_secrets: Vec::new(),
        }
    }

    pub fn bearer(self, token: &str) -> Self {
        self.header("Authorization", &format!("Bearer {token}"))
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }
}

/// 与具体 HTTP 栈无关的响应。
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    /// 响应体的字节保真通道（与 body 同源）：body 是 lossy UTF-8 文本，
    /// 二进制协议（grok 的 gRPC-web+proto）必须走本字段，禁止从 body
    /// 还原字节——恰好构成合法 UTF-8 的二进制会静默损坏。
    pub raw: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    /// 网络层失败（DNS、连接中断、TLS 等）——瞬时。
    #[error("网络错误：{0}")]
    Network(String),
    /// 请求超时——瞬时。
    #[error("请求超时")]
    Timeout,
    /// 请求本身非法（URL 解析失败、重定向过多等配置错误）——确定性，重试无意义。
    #[error("请求非法：{0}")]
    InvalidRequest(String),
}

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, HttpError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 安全契约：Debug 输出对敏感头与 URL query 打码，明文 token 不得出现。
    #[test]
    fn debug_masks_sensitive_headers() {
        let req = HttpRequest::get("https://example.com/api?token=sk-plaintext-secret")
            .bearer("sk-plaintext-secret")
            .header("Accept", "application/json")
            .header("x-api-key", "another-secret");
        let dbg = format!("{req:?}");
        assert!(!dbg.contains("sk-plaintext-secret"), "凭据泄漏：{dbg}");
        assert!(!dbg.contains("another-secret"), "x-api-key 泄漏：{dbg}");
        assert!(!dbg.contains("token="), "URL query 泄漏：{dbg}");
        assert!(dbg.contains("***"), "应有打码占位：{dbg}");
        // 普通头不受影响
        assert!(dbg.contains("application/json"), "{dbg}");
        // 无 query 的 URL 原样输出
        let plain = format!("{:?}", HttpRequest::get("https://example.com/api"));
        assert!(plain.contains("https://example.com/api"), "{plain}");
    }

    /// 安全契约：敏感名判断为子串匹配的保守放宽——复合头名
    /// （X-Trace-Token 等）宁可多打码不可漏打码。
    #[test]
    fn debug_masks_compound_sensitive_header_names() {
        let req = HttpRequest::get("https://example.com/api")
            .header("X-Trace-Token", "trace-value-999")
            .header("X-Request-Signature", "sig-value-999");
        let dbg = format!("{req:?}");
        assert!(!dbg.contains("trace-value-999"), "复合头名漏打码：{dbg}");
        assert!(!dbg.contains("sig-value-999"), "签名头漏打码：{dbg}");
    }

    /// 安全契约：declared_secrets 不参与 Debug 输出。
    #[test]
    fn debug_never_outputs_declared_secrets() {
        let mut req = HttpRequest::get("https://example.com/api");
        req.declared_secrets
            .push("declared-plaintext-key-000".into());
        let dbg = format!("{req:?}");
        assert!(
            !dbg.contains("declared-plaintext-key-000"),
            "登记密钥泄漏：{dbg}"
        );
    }
}
