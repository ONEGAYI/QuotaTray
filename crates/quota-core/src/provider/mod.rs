//! 预置平台（native provider）：trait、注册表与共用解析工具。
//!
//! 平台实现的端点与字段映射依据 `docs/CC-Switch调研报告.md` §4.2。

pub mod deepseek;
pub mod openrouter;
pub mod siliconflow;

pub use deepseek::DeepSeek;
pub use openrouter::OpenRouter;
pub use siliconflow::SiliconFlow;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::config::Credentials;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 预置平台元信息（供 CLI/GUI 列表展示）。
#[derive(Debug, Clone, PartialEq)]
pub struct NativeMeta {
    pub id: &'static str,
    pub name: &'static str,
}

/// 预置平台的原生查询实现。
#[async_trait]
pub trait NativeProvider: Send + Sync {
    fn meta(&self) -> NativeMeta;

    /// 查询该平台余额。`http` 由调用方注入（生产=reqwest，测试=mock），
    /// 实现内部不得直接依赖具体 HTTP 栈。
    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
    ) -> Result<Vec<UsageData>, QueryError>;
}

/// 全部预置平台。
pub fn all() -> Vec<Arc<dyn NativeProvider>> {
    vec![
        Arc::new(DeepSeek),
        Arc::new(SiliconFlow),
        Arc::new(OpenRouter),
    ]
}

/// 按 id 查找预置平台。
pub fn find(id: &str) -> Option<Arc<dyn NativeProvider>> {
    all().into_iter().find(|p| p.meta().id == id)
}

/// 全部平台元信息。
pub fn metas() -> Vec<NativeMeta> {
    all().into_iter().map(|p| p.meta()).collect()
}

// ---- 共用工具 -----------------------------------------------------------

/// 发送请求并解析 JSON。网络层失败 → 瞬时；非 2xx 与解析失败按状态码分类。
pub(crate) async fn fetch_json(
    http: &dyn HttpClient,
    req: HttpRequest,
) -> Result<Value, QueryError> {
    let resp = http
        .execute(req)
        .await
        .map_err(|e| QueryError::transient(e.to_string()))?;
    if !resp.is_success() {
        return Err(status_error(resp.status));
    }
    serde_json::from_str(&resp.body).map_err(|_| QueryError::deterministic("响应不是合法 JSON"))
}

/// HTTP 状态码 → 错误双轨分类：
/// 401/403/404 与其他 4xx = 确定性（认证失效/端点错误，重试无意义）；
/// 429 与 5xx = 瞬时（限流/服务端故障，可重试）。
pub(crate) fn status_error(status: u16) -> QueryError {
    let transient = status == 429 || (500..600).contains(&status);
    let message = format!("HTTP {status}");
    if transient {
        QueryError::transient(message)
    } else {
        QueryError::deterministic(message)
    }
}

/// 数值解析：兼容 JSON number 与字符串数字（各平台 API 风格不一）。
pub(crate) fn parse_num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// 响应结构不符合预期 → 确定性失败（带上平台名便于定位）。
pub(crate) fn parse_error(provider: &str, expected: &str) -> QueryError {
    QueryError::deterministic(format!("{provider} 响应缺少字段或格式异常：{expected}"))
}

#[cfg(test)]
pub(crate) mod testing {
    //! 平台实现共用的 mock HTTP 客户端。

    use async_trait::async_trait;
    use std::time::Duration;

    use crate::http::{HttpClient, HttpError, HttpRequest, HttpResponse};

    #[derive(Clone)]
    pub struct MockHttp {
        pub status: u16,
        pub body: String,
        pub network_fail: bool,
        pub delay: Option<Duration>,
    }

    impl MockHttp {
        pub fn ok(body: &str) -> Self {
            Self {
                status: 200,
                body: body.into(),
                network_fail: false,
                delay: None,
            }
        }

        pub fn status(status: u16) -> Self {
            Self {
                status,
                body: String::new(),
                network_fail: false,
                delay: None,
            }
        }

        pub fn fail() -> Self {
            Self {
                status: 0,
                body: String::new(),
                network_fail: true,
                delay: None,
            }
        }

        pub fn delayed(delay: Duration) -> Self {
            Self {
                status: 200,
                body: "{}".into(),
                network_fail: false,
                delay: Some(delay),
            }
        }
    }

    #[async_trait]
    impl HttpClient for MockHttp {
        async fn execute(&self, _req: HttpRequest) -> Result<HttpResponse, HttpError> {
            if let Some(d) = self.delay {
                tokio::time::sleep(d).await;
            }
            if self.network_fail {
                return Err(HttpError::Network("mock 网络故障".into()));
            }
            Ok(HttpResponse {
                status: self.status,
                body: self.body.clone(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 状态码分类契约：401/404 确定性，429/500/503 瞬时。
    #[test]
    fn status_classification() {
        for code in [401, 403, 404, 400] {
            assert!(!status_error(code).is_transient(), "{code} 应为确定性");
        }
        for code in [429, 500, 502, 503] {
            assert!(status_error(code).is_transient(), "{code} 应为瞬时");
        }
    }

    /// 数值解析契约：number 与字符串数字均可，其余为 None。
    #[test]
    fn number_parsing_flexibility() {
        assert_eq!(parse_num(Some(&serde_json::json!(1.5))), Some(1.5));
        assert_eq!(parse_num(Some(&serde_json::json!("42.50"))), Some(42.5));
        assert_eq!(parse_num(Some(&serde_json::json!(" 7 "))), Some(7.0));
        assert_eq!(parse_num(Some(&serde_json::json!(null))), None);
        assert_eq!(parse_num(Some(&serde_json::json!(true))), None);
        assert_eq!(parse_num(Some(&serde_json::json!("abc"))), None);
        assert_eq!(parse_num(None), None);
    }

    /// 注册表契约：id 唯一且可按 id 找到。
    #[test]
    fn registry_ids_unique_and_findable() {
        let metas = metas();
        let mut ids: Vec<_> = metas.iter().map(|m| m.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "存在重复的平台 id");
        for id in ids {
            assert!(find(id).is_some(), "id {id} 应能在注册表中找到");
        }
    }
}
