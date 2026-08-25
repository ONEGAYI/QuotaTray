//! 预置平台（native provider）：trait、注册表与共用解析工具。
//!
//! 平台实现的端点与字段映射依据 `docs/CC-Switch调研报告.md` §4.2。

pub mod deepseek;
pub mod kimi;
pub mod kimi_coding;
pub mod minimax;
pub mod novita;
pub mod openrouter;
pub mod siliconflow;
pub mod stepfun;
pub mod zhipu;
pub mod zhipu_metered;

pub use deepseek::DeepSeek;
pub use kimi::{KIMI_CN, KIMI_GLOBAL, Kimi};
pub use kimi_coding::{KIMI_CODE_CN, KIMI_CODE_GLOBAL, KimiCode};
pub use minimax::{MINIMAX_CN, MINIMAX_GLOBAL, MiniMax};
pub use novita::Novita;
pub use openrouter::OpenRouter;
pub use siliconflow::{SILICONFLOW_CN, SILICONFLOW_GLOBAL, SiliconFlow};
pub use stepfun::StepFun;
pub use zhipu::{ZAI, ZHIPU, ZhipuApi};
pub use zhipu_metered::{ZAI_API, ZHIPU_API, ZhipuMetered};

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
        Arc::new(ZHIPU_API),
        Arc::new(ZHIPU),
        Arc::new(ZAI_API),
        Arc::new(ZAI),
        Arc::new(StepFun),
        Arc::new(Novita),
        Arc::new(MINIMAX_CN),
        Arc::new(MINIMAX_GLOBAL),
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
    let resp = http.execute(req.clone()).await.map_err(|e| match &e {
        crate::http::HttpError::Timeout | crate::http::HttpError::Network(_) => {
            QueryError::transient(e.to_string())
        }
        crate::http::HttpError::InvalidRequest(_) => QueryError::deterministic(e.to_string()),
    })?;
    if !resp.is_success() {
        return Err(status_error_with_body(resp.status, &resp.body, &req));
    }
    parse_success_json(&req, &resp)
}

/// 对错误文案整体过脱敏后重建（保留分类与 detail）。用于 2xx 业务错误/
/// 取值错误的 message 可能携带响应体内容的场景（网关可能在业务错误
/// 说明或字段值中回显凭据，message 传播面比 detail 更广）。
pub(crate) fn redact_error_message(e: QueryError, req: &HttpRequest) -> QueryError {
    let message = crate::http::redact::redact_body(e.message(), req);
    let rebuilt = if e.is_transient() {
        QueryError::transient(message)
    } else {
        QueryError::deterministic(message)
    };
    match e.detail() {
        Some(d) => rebuilt.with_detail(d),
        None => rebuilt,
    }
}

/// 2xx 响应体 → JSON：解析失败为确定性错误，detail 携带 serde 位置
/// （行列，说明"怎么不合法"）与脱敏响应体片段（说明"响应长什么样"）。
pub(crate) fn parse_success_json(
    req: &HttpRequest,
    resp: &crate::http::HttpResponse,
) -> Result<Value, QueryError> {
    serde_json::from_str(&resp.body).map_err(|e| {
        QueryError::deterministic("响应不是合法 JSON").with_detail(format!(
            "JSON 解析错误：{e}\n响应体（已脱敏）：\n{}",
            crate::http::redact::redact_and_truncate(&resp.body, req)
        ))
    })
}

/// HTTP 状态码 → 错误双轨分类：
/// 408（请求超时）/429（限流）/5xx = 瞬时（可重试）；
/// 其余 4xx（401/403/404/402 等）= 确定性（认证失效、欠费、端点错误，重试无意义）。
/// 响应体含 `error.message` 时附加到文案并同样过脱敏（网关可能在错误说明中
/// 回显请求凭据，message 的传播面比 detail 更广：托盘/表格/卡片）；
/// detail 携带脱敏后的完整响应体片段（用户显式复制排查用）。
pub(crate) fn status_error_with_body(status: u16, body: &str, req: &HttpRequest) -> QueryError {
    let transient = status == 408 || status == 429 || (500..600).contains(&status);
    let message = match error_detail(body) {
        Some(detail) => {
            format!(
                "HTTP {status}: {}",
                crate::http::redact::redact_body(&detail, req)
            )
        }
        None => format!("HTTP {status}"),
    };
    let err = if transient {
        QueryError::transient(message)
    } else {
        QueryError::deterministic(message)
    };
    if body.trim().is_empty() {
        return err;
    }
    err.with_detail(format!(
        "HTTP {status} 响应体（已脱敏）：\n{}",
        crate::http::redact::redact_and_truncate(body, req)
    ))
}

/// 从错误响应体提取说明（嵌套 `error.message` 为 OpenRouter/one-api 系
/// 主流形态；平铺 `{"error":"plain string"}` 为 one-api 系少数端点，作
/// 回退；非字符串形态不误提取，纯空白视为无信息），截断到 120 字符；
/// 调用方须过脱敏后才能拼进 message（网关可能在错误说明中回显请求凭据）。
fn error_detail(body: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    let error = parsed.get("error")?;
    let msg = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())?;
    // 先去首尾空白再截断：纯空白说明远端未给有效信息，回退裸状态码
    let trimmed: String = msg.trim().chars().take(120).collect();
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
        let req = HttpRequest::get("https://api.example.com");
        for code in [400, 401, 402, 403, 404, 422] {
            assert!(
                !status_error_with_body(code, "", &req).is_transient(),
                "{code} 应为确定性"
            );
        }
        for code in [408, 429, 500, 502, 503] {
            assert!(
                status_error_with_body(code, "", &req).is_transient(),
                "{code} 应为瞬时"
            );
        }
    }

    /// 状态码错误透出响应体说明（error.message）契约。
    #[test]
    fn status_error_includes_body_detail() {
        let req = HttpRequest::get("https://api.example.com");
        let body = r#"{"error":{"message":"Insufficient credits"}}"#;
        let err = status_error_with_body(402, body, &req);
        assert!(!err.is_transient());
        assert!(
            err.message().contains("Insufficient credits"),
            "实际：{err}"
        );
        // 非 JSON 响应体回退为纯状态码文案
        let plain = status_error_with_body(404, "<html>Not Found</html>", &req);
        assert_eq!(plain.message(), "HTTP 404");
    }

    /// 契约：one-api 系少数端点的平铺错误形态 `{"error":"plain string"}`
    /// 同样进入 message（嵌套 error.message 之后的回退）。
    #[test]
    fn status_error_includes_plain_string_error() {
        let req = HttpRequest::get("https://api.example.com");
        let body = r#"{"error":"Insufficient credits"}"#;
        let err = status_error_with_body(402, body, &req);
        assert!(
            err.message().contains("Insufficient credits"),
            "实际：{err}"
        );
        // 空串平铺与嵌套空串一致：回退纯状态码文案
        let empty = status_error_with_body(404, r#"{"error":""}"#, &req);
        assert_eq!(empty.message(), "HTTP 404");
    }

    /// 安全契约：平铺形态中的远端回显凭据同样在进入 message 前脱敏
    /// （脱敏在 status_error_with_body 收口，形态不应成为旁路）。
    #[test]
    fn plain_string_error_redacts_echoed_secret() {
        let req = HttpRequest::get("https://api.example.com").bearer("sk-live-secret-000");
        let body = r#"{"error":"Incorrect API key: sk-live-secret-000 provided"}"#;
        let err = status_error_with_body(401, body, &req);
        assert!(
            !err.message().contains("sk-live-secret-000"),
            "message 泄漏明文凭据：{}",
            err.message()
        );
        assert!(
            err.message().contains("<redacted>"),
            "应保留打码占位：{}",
            err.message()
        );
    }

    /// 契约：非字符串 error 形态（数字/数组/空对象/null，含嵌套 message
    /// 非字符串）不误提取，回退裸状态码文案——平铺回退依赖 serde_json
    /// 「非 Object 取值返回 None」语义，测试锁定防回归改写。
    #[test]
    fn non_string_error_shapes_fall_back_to_status_only() {
        let req = HttpRequest::get("https://api.example.com");
        for body in [
            r#"{"error":42}"#,
            r#"{"error":["a"]}"#,
            r#"{"error":{}}"#,
            r#"{"error":null}"#,
            r#"{"error":{"message":123}}"#,
        ] {
            let err = status_error_with_body(402, body, &req);
            assert_eq!(err.message(), "HTTP 402", "形态 {body} 不应误提取");
        }
    }

    /// 契约：纯空白的错误说明（嵌套或平铺）视为无有效信息，
    /// 回退裸状态码文案，不产出带尾随空白的 `HTTP 402:  `。
    #[test]
    fn whitespace_only_error_message_falls_back() {
        let req = HttpRequest::get("https://api.example.com");
        let err = status_error_with_body(402, r#"{"error":"   "}"#, &req);
        assert_eq!(err.message(), "HTTP 402");
        let err = status_error_with_body(402, r#"{"error":{"message":"  "}}"#, &req);
        assert_eq!(err.message(), "HTTP 402");
    }

    /// 安全契约：error.message 中的远端回显凭据在进入 message 前已脱敏
    /// （message 传播面比 detail 更广：托盘/CLI 表格/卡片）。
    #[test]
    fn status_error_message_redacts_echoed_secret() {
        let req = HttpRequest::get("https://api.example.com").bearer("sk-live-secret-000");
        let body = r#"{"error":{"message":"Incorrect API key: sk-live-secret-000 provided"}}"#;
        let err = status_error_with_body(401, body, &req);
        assert!(
            !err.message().contains("sk-live-secret-000"),
            "message 泄漏明文凭据：{}",
            err.message()
        );
        assert!(
            err.message().contains("<redacted>"),
            "应保留打码占位：{}",
            err.message()
        );
    }

    /// 安全契约：错误 detail 中的响应体已脱敏——服务端回显请求密钥时
    /// 只出现 `<redacted>` 占位，明文凭据不得出现。
    #[test]
    fn error_detail_body_is_redacted() {
        let req = HttpRequest::get("https://api.example.com").bearer("sk-live-secret-000");

        let err = status_error_with_body(403, "Forbidden: key sk-live-secret-000", &req);
        assert_eq!(err.message(), "HTTP 403");
        let detail = err.detail().expect("非空 body 应携带 detail");
        assert!(detail.contains("已脱敏"), "应标注脱敏来源：{detail}");
        assert!(
            !detail.contains("sk-live-secret-000"),
            "detail 泄漏明文凭据：{detail}"
        );

        // 空 body 不携带 detail
        assert!(
            status_error_with_body(500, "", &req).detail().is_none(),
            "空 body 不应有 detail"
        );
    }

    /// 契约：2xx 非 JSON 响应——确定性失败，detail 携带 serde 解析位置
    /// 与脱敏响应体（回显密钥同样打码）。
    #[test]
    fn parse_success_json_detail_includes_position_and_redacted_body() {
        let req = HttpRequest::get("https://api.example.com").bearer("sk-live-secret-000");
        let resp = crate::http::HttpResponse {
            status: 200,
            body: "<html>oops sk-live-secret-000</html>".into(),
        };
        let err = parse_success_json(&req, &resp).unwrap_err();
        assert!(!err.is_transient());
        assert_eq!(err.message(), "响应不是合法 JSON");
        let detail = err.detail().expect("应携带 detail");
        assert!(
            detail.contains("JSON 解析错误"),
            "应含 serde 位置说明：{detail}"
        );
        assert!(
            !detail.contains("sk-live-secret-000"),
            "detail 泄漏明文凭据：{detail}"
        );
        assert!(detail.contains("<redacted>"), "应含打码占位：{detail}");
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

    /// 智谱通用 API 按量计费与 Coding Plan 是独立凭据/查询语义；
    /// 新条目进入注册表，但不展示订阅套餐变体。
    #[test]
    fn registry_contains_zhipu_metered_without_plan_variant() {
        for id in ["zhipu_api", "zai_api"] {
            assert!(find(id).is_some(), "注册表缺少 {id}");
            assert!(!supports_plan_variant(id), "{id} 不应展示套餐变体");
        }
    }

    /// StepFun/Novita 按量余额与 MiniMax 固定 5h+周窗口协议，
    /// 均不展示套餐变体（MiniMax 周桶由响应 status 字段自描述）。
    #[test]
    fn registry_contains_batch1_providers_without_plan_variant() {
        for id in ["stepfun", "novita", "minimax", "minimax_global"] {
            assert!(find(id).is_some(), "注册表缺少 {id}");
            assert!(!supports_plan_variant(id), "{id} 不应展示套餐变体");
        }
    }
}
