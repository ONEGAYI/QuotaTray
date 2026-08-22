//! HTTP 访问抽象：Provider 实现只依赖 [`HttpClient`] trait，
//! 单测注入 mock，生产使用 reqwest 实现。

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
}

// 手写 Debug：敏感头打码，防止请求日志（未来的排障手段）泄漏凭据。
impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(k, v)| {
                        (
                            k,
                            if is_sensitive_header(k) {
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

/// Debug/日志输出中必须打码的请求头（大小写不敏感）。
fn is_sensitive_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "x-api-key"
    )
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
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

    /// 安全契约：Debug 输出对敏感头打码，明文 token 不得出现。
    #[test]
    fn debug_masks_sensitive_headers() {
        let req = HttpRequest::get("https://example.com/api")
            .bearer("sk-plaintext-secret")
            .header("Accept", "application/json")
            .header("x-api-key", "another-secret");
        let dbg = format!("{req:?}");
        assert!(
            !dbg.contains("sk-plaintext-secret"),
            "Authorization 泄漏：{dbg}"
        );
        assert!(!dbg.contains("another-secret"), "x-api-key 泄漏：{dbg}");
        assert!(dbg.contains("***"), "应有打码占位：{dbg}");
        // 普通头不受影响
        assert!(dbg.contains("application/json"), "{dbg}");
    }
}
