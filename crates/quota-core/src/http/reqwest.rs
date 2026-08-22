//! reqwest 生产实现。

use std::time::Duration;

use async_trait::async_trait;

use super::{HttpClient, HttpError, HttpRequest, HttpResponse};

/// 基于 reqwest 的客户端（rustls，无 native-tls 依赖）。
pub struct ReqwestHttpClient {
    client: reqwest::Client,
}

impl ReqwestHttpClient {
    /// `fallback_timeout` 是 HTTP 栈内的兜底超时；
    /// 业务级超时（区分瞬时/确定性错误）由 query 引擎的 tokio::time::timeout 承担。
    pub fn new(fallback_timeout: Duration) -> Result<Self, HttpError> {
        let client = reqwest::Client::builder()
            .timeout(fallback_timeout)
            .build()
            .map_err(|e| HttpError::Network(e.to_string()))?;
        Ok(Self { client })
    }
}

#[async_trait]
impl HttpClient for ReqwestHttpClient {
    async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
        let method = match req.method {
            super::Method::Get => reqwest::Method::GET,
            super::Method::Post => reqwest::Method::POST,
        };
        let mut request = self.client.request(method, &req.url);
        for (name, value) in &req.headers {
            request = request.header(name, value);
        }
        if let Some(body) = req.body {
            request = request.body(body);
        }
        let resp = request.send().await.map_err(map_reqwest_err)?;
        let status = resp.status().as_u16();
        let body = resp.text().await.map_err(map_reqwest_err)?;
        Ok(HttpResponse { status, body })
    }
}

fn map_reqwest_err(e: reqwest::Error) -> HttpError {
    if e.is_timeout() {
        HttpError::Timeout
    } else if e.is_builder() || e.is_redirect() {
        // URL 非法/重定向过多是配置错误，重试无意义（M2 模板自定义 URL 后会实际暴露）
        HttpError::InvalidRequest(e.to_string())
    } else {
        HttpError::Network(e.to_string())
    }
}
