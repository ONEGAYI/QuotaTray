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
        Self::new_with_proxy(fallback_timeout, None)
    }

    /// 带可选代理构造（更新检测等 GitHub 访问通道；业务查询仍走 [`Self::new`]）。
    /// 显式设置代理后 reqwest 不再叠加环境变量代理。
    pub fn new_with_proxy(
        fallback_timeout: Duration,
        proxy: Option<&str>,
    ) -> Result<Self, HttpError> {
        let mut builder = reqwest::Client::builder().timeout(fallback_timeout);
        if let Some(url) = proxy {
            let proxy = reqwest::Proxy::all(url)
                .map_err(|e| HttpError::Network(format!("代理配置无效：{e}")))?;
            builder = builder.proxy(proxy);
        }
        let client = builder
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
        let raw = resp.bytes().await.map_err(map_reqwest_err)?.to_vec();
        let body = String::from_utf8_lossy(&raw).into_owned();
        Ok(HttpResponse { status, body, raw })
    }
}

/// reqwest 错误 → HttpError。
///
/// 安全：reqwest 的 `Error::Display` 尾部附带完整 URL（可能含 query string 里的
/// 明文凭据），统一经 [`reqwest::Error::without_url`] 剥离后再转为文案——
/// 错误信息红线见 `model.rs`（不含凭据材料，可直接透出）。
fn map_reqwest_err(e: reqwest::Error) -> HttpError {
    // 先借用判定分类，再按需消耗 e 剥离 URL（其 None 分支拿不回原错误）
    let is_timeout = e.is_timeout();
    let is_cfg_error = e.is_builder() || e.is_redirect();
    let text = if e.url().is_some() {
        e.without_url().to_string()
    } else {
        e.to_string()
    };
    if is_timeout {
        HttpError::Timeout
    } else if is_cfg_error {
        // URL 非法/重定向过多是配置错误，重试无意义（M2 模板自定义 URL 后会实际暴露）
        HttpError::InvalidRequest(text)
    } else {
        HttpError::Network(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_with_proxy_builds_or_rejects_url() {
        assert!(
            ReqwestHttpClient::new_with_proxy(Duration::from_secs(5), None).is_ok(),
            "None 等价于 new()"
        );
        assert!(
            ReqwestHttpClient::new_with_proxy(
                Duration::from_secs(5),
                Some("http://127.0.0.1:7897")
            )
            .is_ok(),
            "合法代理 URL 应构造成功"
        );
        assert!(
            ReqwestHttpClient::new_with_proxy(Duration::from_secs(5), Some("not a url")).is_err(),
            "非法 URL（无 scheme）应返回 Err"
        );
    }

    /// 安全契约：网络错误文案不含请求 URL（key 可能写在 query string 中）。
    #[test]
    fn network_error_message_contains_no_url() {
        // 无网络环境下对内网保留地址发起请求，制造确定的连接错误
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client = ReqwestHttpClient::new(Duration::from_secs(5)).unwrap();
        let secret = "sk-leaked-secret";
        let err = rt.block_on(async {
            client
                .execute(HttpRequest {
                    method: super::super::Method::Get,
                    url: format!("https://192.0.2.1:1/x?token={secret}"), // TEST-NET-1，必失败
                    headers: vec![],
                    body: None,
                    declared_secrets: Vec::new(),
                })
                .await
                .expect_err("TEST-NET-1 请求必须失败")
        });
        let msg = err.to_string();
        assert!(!msg.contains(secret), "错误文案泄漏明文凭据：{msg}");
        assert!(!msg.contains("192.0.2.1"), "错误文案泄漏请求 URL：{msg}");
    }
}
