//! 预置平台（native provider）：trait、注册表与共用解析工具。
//!
//! 平台实现的端点与字段映射依据 `docs/CC-Switch调研报告.md` §4.2。

pub mod deepseek;
pub mod kimi;
pub mod kimi_coding;
pub mod openrouter;
pub mod siliconflow;
pub mod zhipu;

pub use deepseek::DeepSeek;
pub use kimi::{KIMI_CN, KIMI_GLOBAL, Kimi};
pub use kimi_coding::{KIMI_CODE_CN, KIMI_CODE_GLOBAL, KimiCode};
pub use openrouter::OpenRouter;
pub use siliconflow::{SILICONFLOW_CN, SILICONFLOW_GLOBAL, SiliconFlow};
pub use zhipu::{ZAI, ZHIPU, ZhipuApi};

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use serde_json::Value;

use crate::config::{Credentials, PlanVariant};
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
    /// 实现内部不得直接依赖具体 HTTP 栈；`variant` 为条目声明的套餐
    /// 变体（订阅型平台据此过滤限额窗口，按量平台忽略）。
    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
        variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError>;
}

/// 全部预置平台（进程内固化，避免每次查找重建注册表）。
static REGISTRY: LazyLock<Vec<Arc<dyn NativeProvider>>> = LazyLock::new(|| {
    vec![
        Arc::new(DeepSeek),
        Arc::new(SILICONFLOW_CN),
        Arc::new(SILICONFLOW_GLOBAL),
        Arc::new(OpenRouter),
        Arc::new(KIMI_CN),
        Arc::new(KIMI_GLOBAL),
        Arc::new(KIMI_CODE_CN),
        Arc::new(KIMI_CODE_GLOBAL),
        Arc::new(ZHIPU),
        Arc::new(ZAI),
    ]
});

/// 全部预置平台。
pub fn all() -> Vec<Arc<dyn NativeProvider>> {
    REGISTRY.clone()
}

/// 按 id 查找预置平台。
pub fn find(id: &str) -> Option<Arc<dyn NativeProvider>> {
    all().into_iter().find(|p| p.meta().id == id)
}

/// 该 native 平台是否支持套餐变体声明（[`crate::config::PlanVariant`]）。
/// 当前仅智谱系订阅套餐使用（v1 无周限 / v2+ 有周限），其他平台查询
/// 忽略该字段；UI/向导据此决定是否展示变体选择。
pub fn supports_plan_variant(id: &str) -> bool {
    matches!(id, "zhipu" | "zai")
}

/// 全部平台元信息。
pub fn metas() -> Vec<NativeMeta> {
    all().into_iter().map(|p| p.meta()).collect()
}

// ---- 共用工具 -----------------------------------------------------------

/// 发送请求并解析 JSON。
///
/// 错误映射：网络/超时 → 瞬时；请求非法（配置错误）→ 确定性；
/// 非 2xx 按状态码分类并尽量透出响应体中的错误说明。
pub(crate) async fn fetch_json(
    http: &dyn HttpClient,
    req: HttpRequest,
) -> Result<Value, QueryError> {
    let resp = http.execute(req).await.map_err(|e| match &e {
        crate::http::HttpError::Timeout | crate::http::HttpError::Network(_) => {
            QueryError::transient(e.to_string())
        }
        crate::http::HttpError::InvalidRequest(_) => QueryError::deterministic(e.to_string()),
    })?;
    if !resp.is_success() {
        return Err(status_error_with_body(resp.status, &resp.body));
    }
    serde_json::from_str(&resp.body).map_err(|_| QueryError::deterministic("响应不是合法 JSON"))
}

/// HTTP 状态码 → 错误双轨分类：
/// 408（请求超时）/429（限流）/5xx = 瞬时（可重试）；
/// 其余 4xx（401/403/404/402 等）= 确定性（认证失效、欠费、端点错误，重试无意义）。
/// 响应体含 `error.message` 时附加到文案（远端内容不含本地凭据，可透出）。
pub(crate) fn status_error_with_body(status: u16, body: &str) -> QueryError {
    let transient = status == 408 || status == 429 || (500..600).contains(&status);
    let message = match error_detail(body) {
        Some(detail) => format!("HTTP {status}: {detail}"),
        None => format!("HTTP {status}"),
    };
    if transient {
        QueryError::transient(message)
    } else {
        QueryError::deterministic(message)
    }
}

/// 从错误响应体提取说明（OpenRouter/one-api 系惯例为 `error.message`），
/// 截断到 120 字符；远端返回的内容不含本地凭据，可安全透出。
fn error_detail(body: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    let msg = parsed.get("error")?.get("message")?.as_str()?;
    let trimmed: String = msg.chars().take(120).collect();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// 数值解析：兼容 JSON number 与字符串数字（各平台 API 风格不一），
/// 拒绝非有限值——`"NaN"`/`"Infinity"` 等在 Rust 中可成功 parse，
/// 会静默绕过比较判断并污染序列化（f64 非有限值序列化为 null）。
pub(crate) fn parse_num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()),
        _ => None,
    }
}

/// 整数字段解析：兼容 JSON number 与字符串数字（Kimi/SiliconFlow 的
/// 业务码字段历史上两种形态都出现过）。
pub(crate) fn parse_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
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

    pub struct MockHttp {
        pub status: u16,
        pub body: String,
        pub network_fail: bool,
        pub delay: Option<Duration>,
        /// 收到的请求记录（update 模块用它断言 User-Agent/Accept 等头契约）。
        /// Mutex 无 Clone，手动实现（克隆快照进新 Mutex）。
        captured: std::sync::Mutex<Vec<HttpRequest>>,
    }

    impl Clone for MockHttp {
        fn clone(&self) -> Self {
            Self {
                status: self.status,
                body: self.body.clone(),
                network_fail: self.network_fail,
                delay: self.delay,
                captured: std::sync::Mutex::new(self.captured_requests()),
            }
        }
    }

    impl MockHttp {
        pub fn ok(body: &str) -> Self {
            Self {
                status: 200,
                body: body.into(),
                network_fail: false,
                delay: None,
                captured: std::sync::Mutex::new(Vec::new()),
            }
        }

        pub fn status(status: u16) -> Self {
            Self {
                status,
                body: String::new(),
                network_fail: false,
                delay: None,
                captured: std::sync::Mutex::new(Vec::new()),
            }
        }

        pub fn fail() -> Self {
            Self {
                status: 0,
                body: String::new(),
                network_fail: true,
                delay: None,
                captured: std::sync::Mutex::new(Vec::new()),
            }
        }

        pub fn delayed(delay: Duration) -> Self {
            Self {
                status: 200,
                body: "{}".into(),
                network_fail: false,
                delay: Some(delay),
                captured: std::sync::Mutex::new(Vec::new()),
            }
        }

        /// 已捕获的请求快照。
        pub fn captured_requests(&self) -> Vec<HttpRequest> {
            self.captured.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl HttpClient for MockHttp {
        async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
            self.captured.lock().unwrap().push(req);
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

    /// 状态码分类契约：401/402/404 确定性（认证/欠费/端点错误），
    /// 408/429/5xx 瞬时（请求超时/限流/服务端故障，可重试）。
    #[test]
    fn status_classification() {
        for code in [400, 401, 402, 403, 404, 422] {
            assert!(
                !status_error_with_body(code, "").is_transient(),
                "{code} 应为确定性"
            );
        }
        for code in [408, 429, 500, 502, 503] {
            assert!(
                status_error_with_body(code, "").is_transient(),
                "{code} 应为瞬时"
            );
        }
    }

    /// 状态码错误透出响应体说明（error.message）契约。
    #[test]
    fn status_error_includes_body_detail() {
        let body = r#"{"error":{"message":"Insufficient credits"}}"#;
        let err = status_error_with_body(402, body);
        assert!(!err.is_transient());
        assert!(
            err.message().contains("Insufficient credits"),
            "实际：{err}"
        );
        // 非 JSON 响应体回退为纯状态码文案
        let plain = status_error_with_body(404, "<html>Not Found</html>");
        assert_eq!(plain.message(), "HTTP 404");
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

    /// 数值解析契约：拒绝非有限值——Rust 的 f64 parse 接受 "NaN"/"Infinity"
    /// 等字面量，放行会静默绕过比较判断且序列化为 null 污染快照。
    #[test]
    fn number_parsing_rejects_non_finite() {
        for bad in ["NaN", "nan", "Infinity", "-inf", "1e999"] {
            assert_eq!(
                parse_num(Some(&serde_json::json!(bad))),
                None,
                "{bad} 应被拒绝"
            );
        }
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

    /// Kimi Code 使用固定的 5h + 周窗口协议，不复用 GLM 套餐变体开关。
    #[test]
    fn registry_contains_kimi_code_without_plan_variant() {
        for id in ["kimi_code_cn", "kimi_code_global"] {
            assert!(find(id).is_some(), "注册表缺少 {id}");
            assert!(!supports_plan_variant(id), "{id} 不应展示 GLM 套餐变体");
        }
    }
}
