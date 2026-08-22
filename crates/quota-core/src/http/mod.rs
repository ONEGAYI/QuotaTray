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
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
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
}

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, HttpError>;
}
