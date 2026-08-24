//! QuickJS 沙箱脚本查询（M4）：`{request, extractor}` 两阶段协议，兜底
//! 声明式模板覆盖不了的复杂平台（多窗口聚合、特殊签名）。
//!
//! 六环节（继承 cc-switch 生产实践，docs/CC-Switch调研报告.md）：
//! ① 变量替换——代码字符串层面注入 `{{apiKey}}`/`{{baseUrl}}`，脚本
//!    作者接触不到真实凭据，脚本可安全分享；
//! ② 沙箱内第一次 eval——脚本定义全局 `request()` 返回请求描述对象
//!    （沙箱无网络/文件系统/timer，不注入任何宿主 API）；
//! ③ URL 安全校验（与模板同一档：https / loopback / 显式 allowInsecure）；
//! ④ 宿主构造 HttpRequest（apiKey 从根登记 declared_secrets）发请求，
//!    错误双轨分类与脱敏经 `provider::fetch_json` 统一收口；
//! ⑤ 沙箱内第二次 eval——响应 JSON 传入全局 `extract(resp)`；
//! ⑥ 产物校验转 UsageData（JSON 往返天然滤掉 NaN/Infinity）。
//!
//! 沙箱限制（不低于参考实现）：内存 16MiB、栈 256KiB、单次 eval 5 秒
//! CPU 中断器。eval 是同步 CPU 执行，tokio 的引擎级超时只能在 await 点
//! 取消——故 eval 置于 `spawn_blocking` 线程，由中断器在限内自行终止
//! （引擎超时先行返回时，blocking 线程随后自然回收）。
//!
//! 安全：JS 异常消息与脚本产物都可能回显注入代码的明文凭据，全部错误
//! 路径经 `provider::redact_error_message` 收口（模板同款契约）。

use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, CaughtError, Context, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::{HttpClient, HttpRequest, Method};
use crate::model::{QueryError, UsageData};
use crate::template;

/// 脚本源码大小上限（64KB：配置内嵌源码，防误粘巨型文件）。
pub const MAX_CODE_BYTES: usize = 64 * 1024;

/// 沙箱内存上限（QuickJS 堆，不低于 cc-switch 基线）。
const MEMORY_LIMIT_BYTES: usize = 16 * 1024 * 1024;
/// 沙箱栈上限。
const MAX_STACK_BYTES: usize = 256 * 1024;
/// 单次 eval 的 CPU 时限（默认；测试经 `execute_with_eval_budget` 注入缩短值）。
const DEFAULT_EVAL_BUDGET: Duration = Duration::from_secs(5);
/// 保存期干跑校验的 CPU 时限。
const VALIDATE_EVAL_BUDGET: Duration = Duration::from_secs(2);
/// 干跑注入的假变量值（贴近真实凭据形状，本身无敏感性）。
const DUMMY_API_KEY: &str = "qt-dryrun-dummy-key";
const DUMMY_BASE_URL: &str = "https://dryrun.invalid";

/// 脚本查询配置（`kind: "script"` 条目的载荷）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScriptConfig {
    /// JS 源码：须定义全局 `request()` 与 `extract(resp)`。
    pub code: String,
    /// 放开 URL 的 https/loopback 限制（与模板同语义）。
    #[serde(default)]
    pub allow_insecure: bool,
}

/// 脚本是否用到 `{{apiKey}}`（GUI/CLI 判断 key 必填，与模板同语义）。
pub fn uses_api_key(config: &ScriptConfig) -> bool {
    config
        .code
        .split("{{")
        .skip(1)
        .filter_map(|rest| rest.split("}}").next())
        .any(|var| var.trim() == "apiKey")
}

/// 静态校验失败（保存脚本时），带字段定位——与 TemplateError 同构，
/// GUI/CLI 可复用同一 DTO 形状。
#[derive(Debug, thiserror::Error)]
#[error("字段 {field}：{reason}")]
pub struct ScriptError {
    pub field: String,
    pub reason: String,
}

/// 沙箱内执行错误（blocking 线程产出，映射为 QueryError 前的中间形态）。
/// `Js` 的消息来自脚本可控内容，映射时必须过脱敏。
enum EvalError {
    Js(String),
    Interrupted,
    OutOfMemory,
    /// 产物不符合协议（消息由宿主构造，不含脚本产物内容）。
    Shape(String),
}

/// 建一个受限沙箱执行 `f`。每次调用独立 Runtime：脚本状态不跨查询残留。
fn with_sandbox<T>(
    eval_budget: Duration,
    f: impl FnOnce(&rquickjs::Ctx<'_>) -> Result<T, EvalError>,
) -> Result<T, EvalError> {
    let rt = Runtime::new().map_err(|e| EvalError::Js(format!("沙箱初始化失败：{e}")))?;
    rt.set_memory_limit(MEMORY_LIMIT_BYTES);
    rt.set_max_stack_size(MAX_STACK_BYTES);
    let deadline = Instant::now() + eval_budget;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let ctx = Context::full(&rt).map_err(|e| EvalError::Js(format!("沙箱初始化失败：{e}")))?;
    ctx.with(|ctx| f(&ctx))
}

/// JS 异常 → 中间错误。OOM 在 QuickJS 中以异常形态浮出，按消息特征
/// 归类；中断器触发的不可捕获异常可能以内部错误或异常消息两种形态
/// 浮出（QuickJS-NG 实现细节），两类都归入 Interrupted（仅文案差异）。
fn js_error(err: CaughtError) -> EvalError {
    match err {
        CaughtError::Exception(exc) => {
            let msg = exc.message().unwrap_or_else(|| "脚本异常（无消息）".into());
            let lower = msg.to_ascii_lowercase();
            if lower.contains("memory") {
                EvalError::OutOfMemory
            } else if lower.contains("interrupt") {
                EvalError::Interrupted
            } else {
                EvalError::Js(msg)
            }
        }
        CaughtError::Value(_) => EvalError::Js("脚本抛出了非 Error 值".into()),
        CaughtError::Error(e) => EvalError::Js(format!("沙箱内部错误：{e}")),
    }
}

/// JS 产物出箱的统一通道：JSON 序列化（undefined/函数等不可序列化 → Shape；
/// JS 字符串拷出本身可失败，一并归入 Js）。Value 与 Ctx 须同源（同一沙箱）。
fn stringify_out<'js>(
    ctx: &rquickjs::Ctx<'js>,
    v: rquickjs::Value<'js>,
) -> Result<String, EvalError> {
    let js_str = ctx
        .json_stringify(v)
        .map_err(|e| EvalError::Js(format!("产物无法序列化：{e}")))?
        .ok_or_else(|| EvalError::Shape("产物不可 JSON 序列化（undefined/函数）".into()))?;
    js_str
        .to_string()
        .map_err(|e| EvalError::Js(format!("产物字符串拷出失败：{e}")))
}

/// 第一次 eval：执行脚本本体并调 `request()`，返回产物 JSON。
fn eval_request(code: &str, eval_budget: Duration) -> Result<String, EvalError> {
    with_sandbox(eval_budget, |ctx| {
        ctx.eval::<(), _>(code).catch(ctx).map_err(js_error)?;
        let request: rquickjs::Function = ctx
            .globals()
            .get("request")
            .map_err(|_| EvalError::Shape("缺少全局函数 request()".into()))?;
        let result = request
            .call::<(), rquickjs::Value>(())
            .catch(ctx)
            .map_err(js_error)?;
        stringify_out(ctx, result)
    })
}

/// 第二次 eval：执行脚本本体并把响应 JSON 传入 `extract(resp)`。
fn eval_extract(code: &str, resp_json: &str, eval_budget: Duration) -> Result<String, EvalError> {
    with_sandbox(eval_budget, |ctx| {
        ctx.eval::<(), _>(code).catch(ctx).map_err(js_error)?;
        let extract: rquickjs::Function = ctx
            .globals()
            .get("extract")
            .map_err(|_| EvalError::Shape("缺少全局函数 extract(resp)".into()))?;
        let resp = ctx
            .json_parse(resp_json)
            .map_err(|e| EvalError::Js(format!("响应传入沙箱失败：{e}")))?;
        let result = extract
            .call::<(rquickjs::Value,), rquickjs::Value>((resp,))
            .catch(ctx)
            .map_err(js_error)?;
        stringify_out(ctx, result)
    })
}

/// request() 产物的宿主侧形状（宽容解析：未知字段忽略，额外信息留给作者）。
#[derive(Deserialize)]
struct RequestDesc {
    method: Option<String>,
    url: Option<String>,
    #[serde(default)]
    headers: std::collections::BTreeMap<String, String>,
    body: Option<String>,
}

/// 解析 request() 产物 JSON 并构造宿主请求（apiKey 从根登记脱敏，模板
/// 同款）。错误文案不携带产物内容（url 可能已含明文 key）。
fn parse_request_desc(json: &str, api_key: &str) -> Result<HttpRequest, QueryError> {
    let desc: RequestDesc = serde_json::from_str(json).map_err(|_| {
        QueryError::deterministic(
            "request() 产物形状不符（需对象：url 非空字符串、method/headers/body 可选，值均为字符串）",
        )
    })?;
    let url = desc
        .url
        .filter(|u| !u.trim().is_empty())
        .ok_or_else(|| QueryError::deterministic("request() 产物缺少 url（非空字符串）"))?;
    let method = match desc
        .method
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        None | Some("GET") => Method::Get,
        Some("POST") => Method::Post,
        // 不回显原值：method 是脚本产物字段，可能携带注入的明文凭据
        Some(_) => {
            return Err(QueryError::deterministic(
                "request().method 仅支持 GET/POST（收到其他值）",
            ));
        }
    };
    Ok(HttpRequest {
        method,
        url,
        headers: desc.headers.into_iter().collect(),
        body: desc.body,
        declared_secrets: vec![api_key.to_string()],
    })
}

/// 解析 extract() 产物 JSON → UsageData 列表（单对象或数组）。
/// 数值字段走 `parse_num` 语义（字符串数字兼容、拒非有限值——各平台
/// API 风格不一，与 native/模板同规则）；未知字段忽略（宽容）；
/// JSON 往返天然滤掉 NaN/Infinity（序列化为 null → None）。
fn parse_usage_output(json: &str) -> Result<Vec<UsageData>, QueryError> {
    let v: Value = serde_json::from_str(json)
        .map_err(|_| QueryError::deterministic("extract() 产物不是合法 JSON"))?;
    let items: Vec<&Value> = match &v {
        Value::Array(arr) if !arr.is_empty() => arr.iter().collect(),
        Value::Array(_) => {
            return Err(QueryError::deterministic(
                "extract() 返回了空数组（至少一条窗口数据）",
            ));
        }
        Value::Object(_) => vec![&v],
        _ => {
            return Err(QueryError::deterministic(
                "extract() 产物应为对象或对象数组",
            ));
        }
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let obj = item
            .as_object()
            .ok_or_else(|| QueryError::deterministic("extract() 产物应为对象或对象数组"))?;
        let num = |key: &str| crate::provider::parse_num(obj.get(key));
        let text = |key: &str| {
            obj.get(key)
                .and_then(Value::as_str)
                .map(std::borrow::ToOwned::to_owned)
        };
        let data = UsageData {
            plan_name: text("plan_name"),
            total: num("total"),
            used: num("used"),
            remaining: num("remaining"),
            unit: text("unit"),
            reset_at: obj.get("reset_at").and_then(crate::provider::parse_int),
            is_valid: obj.get("is_valid").and_then(Value::as_bool),
            invalid_message: text("invalid_message"),
            extra: obj.get("extra").cloned(),
        };
        // 与模板 check_extract 同语义：至少一个数值字段，防手误返回空壳
        if data.total.is_none() && data.used.is_none() && data.remaining.is_none() {
            return Err(QueryError::deterministic(
                "extract() 产物缺少任何数值字段（total/used/remaining 至少其一）",
            ));
        }
        out.push(data);
    }
    Ok(out)
}

/// 执行脚本查询。`api_key` 来自 vault 解密，`base_url` 为条目配置。
pub(crate) async fn execute(
    http: &dyn HttpClient,
    config: &ScriptConfig,
    api_key: &str,
    base_url: Option<&str>,
) -> Result<Vec<UsageData>, QueryError> {
    execute_with_eval_budget(http, config, api_key, base_url, DEFAULT_EVAL_BUDGET).await
}

/// 带自定义 eval CPU 时限的执行（测试注入缩短值驱动中断契约）。
pub(crate) async fn execute_with_eval_budget(
    http: &dyn HttpClient,
    config: &ScriptConfig,
    api_key: &str,
    base_url: Option<&str>,
    eval_budget: Duration,
) -> Result<Vec<UsageData>, QueryError> {
    // 脱敏锚点：eval 阶段尚无真实请求，用仅含 declared_secrets 的最小
    // 参照（JS 异常消息可能回显注入代码的明文 key）
    let redact_anchor = HttpRequest {
        method: Method::Get,
        url: String::new(),
        headers: Vec::new(),
        body: None,
        declared_secrets: vec![api_key.to_string()],
    };

    // ① 变量替换（代码字符串层面，与模板同一函数与安全契约）
    let code = template::substitute(&config.code, api_key, base_url)?;

    // ② request 阶段（同步 eval → blocking 线程）
    let code_for_req = code.clone();
    let req_json = tokio::task::spawn_blocking(move || eval_request(&code_for_req, eval_budget))
        .await
        .map_err(|e| QueryError::transient(format!("脚本线程异常：{e}")))?
        .map_err(|e| eval_error_to_query(e, &redact_anchor))?;

    // ③④ 产物解析（含从根登记脱敏）+ URL 校验 + 发送。
    // 解析错误统一过脱敏：产物任何字段都可能携带注入的明文凭据
    let req = parse_request_desc(&req_json, api_key)
        .map_err(|e| crate::provider::redact_error_message(e, &redact_anchor))?;
    template::check_url_safety(&req.url, config.allow_insecure)?;
    let resp_root = crate::provider::fetch_json(http, req.clone()).await?;

    // ⑤ extract 阶段
    let resp_json = serde_json::to_string(&resp_root)
        .map_err(|e| QueryError::transient(format!("响应序列化失败：{e}")))?;
    let code_for_extract = code;
    let out_json = tokio::task::spawn_blocking(move || {
        eval_extract(&code_for_extract, &resp_json, eval_budget)
    })
    .await
    .map_err(|e| QueryError::transient(format!("脚本线程异常：{e}")))?
    .map_err(|e| eval_error_to_query(e, &req))?;

    // ⑥ 产物校验转 UsageData（错误统一过脱敏，脚本产物可能回显 key）
    parse_usage_output(&out_json).map_err(|e| crate::provider::redact_error_message(e, &req))
}

/// 中间错误 → 查询错误：全部确定性（脚本 bug 重试无变化）；
/// `Js` 消息脚本可控，必须过脱敏。
fn eval_error_to_query(e: EvalError, anchor: &HttpRequest) -> QueryError {
    match e {
        EvalError::Js(msg) => crate::provider::redact_error_message(
            QueryError::deterministic(format!("脚本执行失败：{msg}")),
            anchor,
        ),
        EvalError::Interrupted => QueryError::deterministic("脚本执行超过 CPU 时限，请检查死循环"),
        EvalError::OutOfMemory => QueryError::deterministic("脚本执行超过内存上限（16MiB）"),
        EvalError::Shape(reason) => {
            QueryError::deterministic(format!("脚本产物不符合协议：{reason}"))
        }
    }
}

/// 保存期静态校验：浅校验 + 干跑（假变量替换后 eval 脚本、验证两个全局
/// 函数存在、`request()` 产物形状——不发 HTTP、不调 `extract`）。
/// 干跑用假凭据，错误消息无泄露风险。
pub fn validate(config: &ScriptConfig) -> Result<(), ScriptError> {
    let err = |field: &str, reason: String| ScriptError {
        field: field.into(),
        reason,
    };
    if config.code.trim().is_empty() {
        return Err(err("code", "脚本源码为空".into()));
    }
    if config.code.len() > MAX_CODE_BYTES {
        return Err(err(
            "code",
            format!("脚本源码超过大小上限（{} KB）", MAX_CODE_BYTES / 1024),
        ));
    }
    // baseUrl 以假值提供：暴露未知变量占位，又不误报缺失
    let dry = template::substitute(&config.code, DUMMY_API_KEY, Some(DUMMY_BASE_URL))
        .map_err(|e| err("code", e.message().to_string()))?;

    let check = with_sandbox(VALIDATE_EVAL_BUDGET, |ctx| {
        ctx.eval::<(), _>(dry.as_str())
            .catch(ctx)
            .map_err(js_error)?;
        let both_defined: bool = ctx
            .eval("typeof request === 'function' && typeof extract === 'function'")
            .map_err(|e| EvalError::Js(format!("沙箱内部错误：{e}")))?;
        if !both_defined {
            return Err(EvalError::Shape(
                "缺少全局函数 request() 或 extract(resp)".into(),
            ));
        }
        let request: rquickjs::Function = ctx
            .globals()
            .get("request")
            .map_err(|_| EvalError::Shape("缺少全局函数 request()".into()))?;
        let result = request
            .call::<(), rquickjs::Value>(())
            .catch(ctx)
            .map_err(js_error)?;
        stringify_out(ctx, result)
    });
    let req_json = match check {
        Ok(json) => json,
        Err(EvalError::Js(msg)) => return Err(err("code", format!("脚本执行失败：{msg}"))),
        Err(EvalError::Interrupted) => {
            return Err(err("code", "脚本执行超过 CPU 时限，请检查死循环".into()));
        }
        Err(EvalError::OutOfMemory) => {
            return Err(err("code", "脚本执行超过内存上限（16MiB）".into()));
        }
        Err(EvalError::Shape(reason)) => return Err(err("request", reason)),
    };
    if let Err(e) = parse_request_desc(&req_json, DUMMY_API_KEY) {
        return Err(err("request", e.message().to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderKind;
    use crate::http::Method;
    use crate::provider::testing::MockHttp;

    const RESP: &str = r#"{"balance":"62.97","total":"100.00","quota_used":37}"#;

    fn config(code: &str) -> ScriptConfig {
        ScriptConfig {
            code: code.into(),
            allow_insecure: false,
        }
    }

    /// 基准脚本：POST + Bearer 头注入 + 单对象提取（字符串数字 → f64）。
    const OK_SCRIPT: &str = r#"
        function request() {
            return {
                method: "POST",
                url: "https://api.demo.com/v1/me",
                headers: { "Authorization": "Bearer {{apiKey}}" },
                body: null
            };
        }
        function extract(resp) {
            return [{ remaining: resp.balance, total: resp.total, unit: "CNY" }];
        }
    "#;

    /// 契约：两阶段 happy path——请求形状由脚本决定（method/头/凭据注入），
    /// 响应经 extractor 映射为 UsageData。
    #[tokio::test]
    async fn happy_path_two_phase() {
        let http = MockHttp::ok(RESP);
        let data = execute(&http, &config(OK_SCRIPT), "sk-live-secret-000", None)
            .await
            .unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].remaining, Some(62.97));
        assert_eq!(data[0].total, Some(100.0));
        assert_eq!(data[0].unit.as_deref(), Some("CNY"));

        let reqs = http.captured_requests();
        assert_eq!(reqs[0].method, Method::Post);
        assert_eq!(reqs[0].url, "https://api.demo.com/v1/me");
        assert!(
            reqs[0]
                .headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer sk-live-secret-000"),
            "apiKey 应注入脚本声明的头：{:?}",
            reqs[0].headers
        );
    }

    /// 契约：extractor 返回数组 → 多窗口；单对象 → 单条。
    #[tokio::test]
    async fn multi_window_and_single_object() {
        let multi = r#"
            function request() { return { url: "https://a.com" }; }
            function extract(resp) {
                return [
                    { plan_name: "five_hour", used: resp.quota_used, unit: "%" },
                    { plan_name: "week", used: resp.quota_used / 2, unit: "%" }
                ];
            }
        "#;
        let data = execute(&MockHttp::ok(RESP), &config(multi), "k", None)
            .await
            .unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].plan_name.as_deref(), Some("five_hour"));
        assert_eq!(data[1].used, Some(18.5));

        let single = r#"
            function request() { return { url: "https://a.com" }; }
            function extract(resp) { return { remaining: resp.balance }; }
        "#;
        let data = execute(&MockHttp::ok(RESP), &config(single), "k", None)
            .await
            .unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].remaining, Some(62.97));
    }

    /// 契约：{{baseUrl}} 变量 + URL 拼接（与模板同语义）。
    #[tokio::test]
    async fn base_url_variable() {
        let script = r#"
            function request() { return { url: "{{baseUrl}}/user/balance" }; }
            function extract(resp) { return { remaining: resp.balance }; }
        "#;
        let http = MockHttp::ok(RESP);
        let data = execute(&http, &config(script), "k", Some("https://api.demo.com"))
            .await
            .unwrap();
        assert_eq!(data[0].remaining, Some(62.97));
        assert_eq!(
            http.captured_requests()[0].url,
            "https://api.demo.com/user/balance"
        );
    }

    /// 安全契约：URL 规则与模板同档——http 非 loopback 拒绝、
    /// allowInsecure 放开、loopback 豁免。
    #[tokio::test]
    async fn url_safety_rules() {
        let script = r#"
            function request() { return { url: "http://api.demo.com/x" }; }
            function extract(resp) { return { remaining: resp.balance }; }
        "#;
        let err = execute(&MockHttp::ok(RESP), &config(script), "k", None)
            .await
            .unwrap_err();
        assert!(!err.is_transient() && err.message().contains("allowInsecure"));

        let mut cfg = config(script);
        cfg.allow_insecure = true;
        assert!(execute(&MockHttp::ok(RESP), &cfg, "k", None).await.is_ok());

        let loopback = r#"
            function request() { return { url: "http://127.0.0.1:8080/x" }; }
            function extract(resp) { return { remaining: resp.balance }; }
        "#;
        assert!(
            execute(&MockHttp::ok(RESP), &config(loopback), "k", None)
                .await
                .is_ok()
        );
    }

    /// 安全契约：脚本异常消息回显注入的明文 key → 进入 message 前已打码
    /// （模板同款红线；throw 消息是脚本可控内容）。
    #[tokio::test]
    async fn js_error_redacts_injected_key() {
        let script = r#"
            function request() {
                throw new Error("auth failed for key: " + "{{apiKey}}");
            }
            function extract(resp) { return { remaining: 1 }; }
        "#;
        let err = execute(
            &MockHttp::ok(RESP),
            &config(script),
            "sk-live-secret-000",
            None,
        )
        .await
        .unwrap_err();
        assert!(!err.is_transient());
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

    /// 契约：request() 产物形状错误 → 确定性失败（返回数字/缺 url/
    /// method 非法/缺 request 函数）。
    #[tokio::test]
    async fn request_shape_errors() {
        for (script, frag) in [
            (
                r#"function request(){ return 42; } function extract(r){ return {remaining:1}; }"#,
                "形状不符",
            ),
            (
                r#"function request(){ return { headers: {} }; } function extract(r){ return {remaining:1}; }"#,
                "缺少 url",
            ),
            (
                r#"function request(){ return { url: "https://a.com", method: "DELETE" }; } function extract(r){ return {remaining:1}; }"#,
                "GET/POST",
            ),
            (
                r#"function extract(r){ return {remaining:1}; }"#,
                "缺少全局函数 request()",
            ),
        ] {
            let err = execute(&MockHttp::ok(RESP), &config(script), "k", None)
                .await
                .unwrap_err();
            assert!(!err.is_transient(), "应确定性：{err}");
            assert!(err.message().contains(frag), "文案应含 {frag}：{err}");
        }
    }

    /// 安全契约：method 字段携带 `{{apiKey}}` 替换后的明文 → 错误消息与
    /// detail 都不回显（method 是脚本产物，值不可信）。
    #[tokio::test]
    async fn method_value_not_leaked() {
        let script = r#"
            function request(){ return { url: "https://a.com", method: "{{apiKey}}" }; }
            function extract(r){ return {remaining: 1}; }
        "#;
        let err = execute(
            &MockHttp::ok(RESP),
            &config(script),
            "sk-live-secret-000",
            None,
        )
        .await
        .unwrap_err();
        assert!(!err.is_transient());
        assert!(
            !err.message().contains("sk-live-secret-000"),
            "message 泄漏明文凭据：{}",
            err.message()
        );
        let detail = err.detail().unwrap_or_default().to_string();
        assert!(
            !detail.contains("sk-live-secret-000"),
            "detail 泄漏明文凭据：{detail}"
        );
    }

    /// 契约：reset_at 整数毫秒与字符串数字透传（parse_int 语义）；
    /// 浮点形态安静落 None（宁缺毋错——倒计时缺失优于错值）。
    #[tokio::test]
    async fn reset_at_integer_kept_fractional_dropped() {
        let mk = |expr: &str| {
            format!(
                r#"
                function request(){{ return {{ url: "https://a.com" }}; }}
                function extract(r){{ return {{ remaining: 1, reset_at: {expr} }}; }}
                "#
            )
        };
        let data = execute(
            &MockHttp::ok(RESP),
            &config(&mk("1893456000000")),
            "k",
            None,
        )
        .await
        .unwrap();
        assert_eq!(data[0].reset_at, Some(1893456000000));

        let data = execute(
            &MockHttp::ok(RESP),
            &config(&mk("\"1893456000000\"")),
            "k",
            None,
        )
        .await
        .unwrap();
        assert_eq!(data[0].reset_at, Some(1893456000000));

        let data = execute(
            &MockHttp::ok(RESP),
            &config(&mk("1893456000000.5")),
            "k",
            None,
        )
        .await
        .unwrap();
        assert_eq!(data[0].reset_at, None, "浮点载体应安静落 None");
    }

    /// 契约：产物循环引用 → JSON 序列化失败 → 确定性错误（固定文案，无泄漏）。
    #[tokio::test]
    async fn circular_output_rejected() {
        let script = r#"
            function request(){ return { url: "https://a.com" }; }
            function extract(r){ const a = {}; a.self = a; return a; }
        "#;
        let err = execute(&MockHttp::ok(RESP), &config(script), "k", None)
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("序列化"), "应指明序列化失败：{err}");
    }

    /// 契约：extract() 产物形状错误 → 确定性失败（空数组/无数值字段/
    /// 非对象/字段类型不符）。
    #[tokio::test]
    async fn extract_shape_errors() {
        for (script, frag) in [
            (
                r#"function request(){ return {url:"https://a.com"}; } function extract(r){ return []; }"#,
                "空数组",
            ),
            (
                r#"function request(){ return {url:"https://a.com"}; } function extract(r){ return {unit:"CNY"}; }"#,
                "数值字段",
            ),
            (
                r#"function request(){ return {url:"https://a.com"}; } function extract(r){ return 7; }"#,
                "对象或对象数组",
            ),
            (
                r#"function request(){ return {url:"https://a.com"}; } function extract(r){ return {remaining: {a:1}}; }"#,
                "数值字段",
            ),
        ] {
            let err = execute(&MockHttp::ok(RESP), &config(script), "k", None)
                .await
                .unwrap_err();
            assert!(!err.is_transient(), "应确定性：{err}");
            assert!(err.message().contains(frag), "文案应含 {frag}：{err}");
        }
    }

    /// 契约：死循环 → CPU 中断器终止 → 确定性失败（注入 100ms 缩短预算）。
    #[tokio::test]
    async fn infinite_loop_interrupted() {
        let script = r#"
            function request(){ while(true){} }
            function extract(r){ return {remaining: 1}; }
        "#;
        let err = execute_with_eval_budget(
            &MockHttp::ok(RESP),
            &config(script),
            "k",
            None,
            Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert!(!err.is_transient(), "脚本 bug 应确定性：{err}");
        assert!(err.message().contains("CPU"), "应指明 CPU 时限：{err}");
    }

    /// 契约：内存炸弹 → 16MiB 上限终止（分配循环在数百次迭代内触顶）。
    #[tokio::test]
    async fn memory_bomb_hits_limit() {
        let script = r#"
            function request(){ const a = []; while(true){ a.push("x".repeat(65536)); } }
            function extract(r){ return {remaining: 1}; }
        "#;
        let err = execute_with_eval_budget(
            &MockHttp::ok(RESP),
            &config(script),
            "k",
            None,
            Duration::from_secs(2),
        )
        .await
        .unwrap_err();
        assert!(
            err.message().contains("内存"),
            "应指明内存上限（判定失效时会以 CPU 时限文案失败）：{err}"
        );
    }

    /// 契约：未知 {{变量}} → 确定性失败，消息只指名变量不带替换结果。
    #[tokio::test]
    async fn unknown_var_rejected() {
        let script = r#"
            function request(){ return { url: "https://a.com/?t={{token}}" }; }
            function extract(r){ return {remaining: 1}; }
        "#;
        let err = execute(&MockHttp::ok(RESP), &config(script), "sk-secret", None)
            .await
            .unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("token"), "实际：{err}");
        assert!(!err.message().contains("sk-secret"));
    }

    /// 契约：JS 的 NaN/Infinity 经 JSON 序列化为 null → 字段安静落 None
    /// （不会污染 f64 序列化）。
    #[tokio::test]
    async fn nan_becomes_none() {
        let script = r#"
            function request(){ return { url: "https://a.com" }; }
            function extract(r){ return { remaining: NaN, used: 12 }; }
        "#;
        let data = execute(&MockHttp::ok(RESP), &config(script), "k", None)
            .await
            .unwrap();
        assert_eq!(data[0].remaining, None);
        assert_eq!(data[0].used, Some(12.0));
    }

    /// 契约：ScriptConfig 序列化形状（camelCase + 未知字段拒绝 + 默认值）
    /// 与 ProviderKind 的 "script" tag 分派。
    #[test]
    fn serde_config_shape() {
        let cfg: ScriptConfig =
            serde_json::from_str(r#"{"code":"function request(){}","allowInsecure":true}"#)
                .unwrap();
        assert!(cfg.allow_insecure);
        assert!(
            serde_json::from_str::<ScriptConfig>(r#"{"code":"x","other":1}"#).is_err(),
            "未知字段应拒绝"
        );
        let cfg: ScriptConfig = serde_json::from_str(r#"{"code":"x"}"#).unwrap();
        assert!(!cfg.allow_insecure, "allowInsecure 缺省为 false");

        // ProviderKind tag 分派 + 序列化往返
        let kind = ProviderKind::Script(Box::new(ScriptConfig {
            code: "function request(){}".into(),
            allow_insecure: false,
        }));
        let json = serde_json::to_value(&kind).unwrap();
        assert_eq!(json["type"], "script");
        assert_eq!(json["code"], "function request(){}");
        let back: ProviderKind = serde_json::from_value(json).unwrap();
        assert_eq!(back, kind);
    }

    /// 契约：uses_api_key 容忍占位内部空白（与 substitute 的解析一致）。
    #[test]
    fn uses_api_key_variants() {
        assert!(uses_api_key(&config("const u = '{{ apiKey }}';")));
        assert!(uses_api_key(&config("{{apiKey}}")));
        assert!(!uses_api_key(&config("function request(){}")));
        assert!(!uses_api_key(&config("{{baseUrl}}")));
    }

    /// 契约：保存期校验——空源码/超限/未知变量为浅校验；语法错误、
    /// 缺函数、request 产物形状由干跑暴露（不发 HTTP）。
    #[test]
    fn validate_layers() {
        // 浅校验
        let e = validate(&config("  ")).unwrap_err();
        assert!(e.reason.contains("为空"));
        let e = validate(&config(&format!(
            "function request(){{}} {}",
            "x".repeat(MAX_CODE_BYTES)
        )))
        .unwrap_err();
        assert!(e.reason.contains("上限"));
        let e = validate(&config(
            "const a = '{{nope}}'; function request(){} function extract(r){}",
        ))
        .unwrap_err();
        assert!(e.reason.contains("nope") && e.field == "code");

        // 干跑：语法错误
        let e = validate(&config("function request({}")).unwrap_err();
        assert!(!e.reason.is_empty());

        // 干跑：缺函数
        let e = validate(&config("function request(){}")).unwrap_err();
        assert!(e.reason.contains("extract"));

        // 干跑：request 产物缺 url
        let e = validate(&config(
            "function request(){ return {}; } function extract(r){ return {remaining:1}; }",
        ))
        .unwrap_err();
        assert!(e.reason.contains("url") && e.field == "request");

        // 合法脚本通过
        assert!(validate(&config(OK_SCRIPT)).is_ok());
    }
}
