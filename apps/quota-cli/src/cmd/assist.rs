//! `quota assist`：面向外部 Agent 的稳定、无凭据调试接口。

use std::path::{Path, PathBuf};

use clap::ValueEnum;
use quota_core::{ScriptConfig, TemplateConfig, UsageData};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum AssistMode {
    Template,
    Script,
}

impl AssistMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Template => "template",
            Self::Script => "script",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostic {
    code: &'static str,
    field: String,
    message: String,
    /// 可选排查详情（如已脱敏的响应体片段）；None 时省略不输出。
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AssistOutput<T: Serialize> {
    schema_version: u32,
    ok: bool,
    stage: &'static str,
    diagnostics: Vec<Diagnostic>,
    result: Option<T>,
}

fn print_output<T: Serialize>(output: &AssistOutput<T>) {
    // Value 均由 serde 构造；序列化失败仅可能是编程错误，退化为固定 JSON。
    println!(
        "{}",
        serde_json::to_string_pretty(output).unwrap_or_else(|_| {
            r#"{"schemaVersion":1,"ok":false,"stage":"internal","diagnostics":[{"code":"QT_ASSIST_SERIALIZE","field":"(output)","message":"输出序列化失败"}],"result":null}"#.into()
        })
    );
}

fn fail(stage: &'static str, code: &'static str, field: &str, message: String) -> i32 {
    fail_with_detail(stage, code, field, message, None)
}

/// 带可选排查详情的失败输出；返回退出码由调用方决定的版本见 [`fail_exit`]。
fn fail_with_detail(
    stage: &'static str,
    code: &'static str,
    field: &str,
    message: String,
    detail: Option<String>,
) -> i32 {
    print_output(&AssistOutput::<Value> {
        schema_version: 1,
        ok: false,
        stage,
        diagnostics: vec![Diagnostic {
            code,
            field: field.into(),
            message,
            detail,
        }],
        result: None,
    });
    1
}

/// 失败输出 + 显式退出码（assist test 的查询失败按三分约定返回 0/1/2）。
fn fail_exit(
    stage: &'static str,
    code: &'static str,
    field: &str,
    message: String,
    detail: Option<String>,
    exit: i32,
) -> i32 {
    fail_with_detail(stage, code, field, message, detail);
    exit
}

fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))
}

/// 同时接受纯配置文件与 GUI 导出的 quotatray-assist-package。
/// 返回 (draft, responseSample, entryId)；后两者仅诊断包携带（可缺省）。
fn decode_input(
    text: &str,
    expected_mode: AssistMode,
) -> Result<(String, Option<String>, Option<String>), String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("JSON 解析失败：{e}"))?;
    if value.get("format").and_then(Value::as_str) != Some("quotatray-assist-package") {
        return Ok((text.to_owned(), None, None));
    }
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("不支持的诊断包版本（当前仅支持 version=1）".into());
    }
    let mode = value
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if mode != expected_mode.as_str() {
        return Err(format!(
            "诊断包模式为 {mode}，命令指定模式为 {}",
            expected_mode.as_str()
        ));
    }
    let draft = value
        .get("draft")
        .and_then(Value::as_str)
        .ok_or_else(|| "诊断包缺少字符串字段 draft".to_string())?;
    let sample = value
        .get("responseSample")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let entry_id = value
        .get("entryId")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned);
    Ok((draft.to_owned(), sample, entry_id))
}

fn schema_value() -> Value {
    serde_json::json!({
        "format": "quotatray-assist-schema",
        "version": 1,
        "quotaTrayVersion": env!("CARGO_PKG_VERSION"),
        "security": {
            "requiresApiKey": false,
            "performsNetworkRequests": false,
            "providesAgent": false
        },
        "assistTest": {
            "purpose": "quota assist test --mode <m> --input <pkg>",
            "usesStoredCredentials": true,
            "performsNetworkRequests": true,
            "credentialFlow": "diagnostic package entryId -> saved provider entry -> vault ciphertext decrypted in-process only; credentials never appear in command output"
        },
        "template": {
            "variables": ["apiKey", "apiKey2", "baseUrl"],
            "methods": ["GET", "POST"],
            "extractFields": ["planName", "total", "used", "remaining", "unit", "isValid", "invalidMessage"],
            "transforms": ["multiply", "divide", "add", "sub", "round"],
            "example": example_template()
        },
        "script": {
            "requiredFunctions": ["request", "extract"],
            "networkApisAvailable": false,
            "maxCodeBytes": quota_core::script::MAX_CODE_BYTES,
            "usageFields": ["plan_name", "total", "used", "remaining", "unit", "reset_at", "is_valid", "invalid_message", "extra"],
            "variables": ["apiKey", "apiKey2", "baseUrl"],
            "example": example_script()
        }
    })
}

/// schema 内嵌的完整可用模板示例：直接演示 DSL 全部关键形态——
/// `transforms` 是模板顶层键（作用于提取后的字段，支持字段名/常数运算），
/// `extract` 字段值只有纯 JSONPath 字符串与 `{"const": ...}` 两种。
fn example_template() -> Value {
    serde_json::json!({
        "request": {
            "method": "GET",
            "url": "{{baseUrl}}/api/user/self",
            "headers": {
                "Authorization": "Bearer {{apiKey}}",
                "New-Api-User": "{{apiKey2}}"
            }
        },
        "extract": {
            "planName": "$.data.display_name",
            "used": "$.data.used_quota",
            "remaining": "$.data.quota",
            "unit": { "const": "USD" },
            "isValid": "$.success"
        },
        "transforms": [
            { "op": "divide", "field": "remaining", "by": 500000 },
            { "op": "round", "field": "remaining", "digits": 2 }
        ]
    })
}

/// schema 内嵌的最小可用脚本示例（双凭据变量注入演示）。
/// extract 字段名为 snake_case（plan_name/is_valid），与脚本产物解析器
/// 的键名一致（schema 的 usageFields 声明同口径）。
fn example_script() -> String {
    r#"function request() {
  return {
    url: "{{baseUrl}}/api/user/self",
    headers: {
      "Authorization": "Bearer {{apiKey}}",
      "New-Api-User": "{{apiKey2}}"
    }
  };
}
function extract(resp) {
  return {
    plan_name: resp.data.display_name,
    remaining: resp.data.quota / 500000,
    used: resp.data.used_quota / 500000,
    unit: "USD",
    is_valid: resp.success === true
  };
}"#
    .to_owned()
}

pub fn run_schema() -> i32 {
    print_output(&AssistOutput {
        schema_version: 1,
        ok: true,
        stage: "schema",
        diagnostics: Vec::new(),
        result: Some(schema_value()),
    });
    0
}

pub fn run_validate(mode: AssistMode, input: PathBuf) -> i32 {
    let text = match read_text(&input) {
        Ok(value) => value,
        Err(message) => return fail("read", "QT_ASSIST_READ", "input", message),
    };
    let (draft, _, _) = match decode_input(&text, mode) {
        Ok(value) => value,
        Err(message) => return fail("parse", "QT_ASSIST_PACKAGE", "input", message),
    };
    let result = match mode {
        AssistMode::Template => {
            let config: TemplateConfig = match serde_json::from_str(&draft) {
                Ok(value) => value,
                Err(e) => return fail("parse", "QT_TEMPLATE_JSON", "(json)", e.to_string()),
            };
            quota_core::template::validate(&config).map_err(|e| match e {
                quota_core::TemplateError::Validation { field, reason } => (field, reason),
            })
        }
        AssistMode::Script => {
            let config: ScriptConfig = match serde_json::from_str(&draft) {
                Ok(value) => value,
                Err(e) => return fail("parse", "QT_SCRIPT_JSON", "(json)", e.to_string()),
            };
            quota_core::script::validate(&config).map_err(|e| (e.field, e.reason))
        }
    };
    match result {
        Ok(()) => {
            print_output(&AssistOutput {
                schema_version: 1,
                ok: true,
                stage: "validate",
                diagnostics: Vec::new(),
                result: Some(serde_json::json!({ "mode": mode.as_str() })),
            });
            0
        }
        Err((field, message)) => fail("validate", "QT_ASSIST_INVALID", &field, message),
    }
}

pub fn run_simulate(mode: AssistMode, input: PathBuf, response: Option<PathBuf>) -> i32 {
    let text = match read_text(&input) {
        Ok(value) => value,
        Err(message) => return fail("read", "QT_ASSIST_READ", "input", message),
    };
    let (draft, package_sample, _) = match decode_input(&text, mode) {
        Ok(value) => value,
        Err(message) => return fail("parse", "QT_ASSIST_PACKAGE", "input", message),
    };
    let response_text = match response {
        Some(path) => match read_text(&path) {
            Ok(value) => value,
            Err(message) => return fail("read", "QT_ASSIST_READ", "response", message),
        },
        None => match package_sample {
            Some(value) if !value.trim().is_empty() => value,
            _ => {
                return fail(
                    "parse",
                    "QT_ASSIST_RESPONSE_REQUIRED",
                    "response",
                    "请提供 --response，或在诊断包中填写 responseSample".into(),
                );
            }
        },
    };
    let response_value: Value = match serde_json::from_str(&response_text) {
        Ok(value) => value,
        Err(e) => return fail("parse", "QT_RESPONSE_JSON", "response", e.to_string()),
    };
    let rows: Result<Vec<UsageData>, String> = match mode {
        AssistMode::Template => serde_json::from_str::<TemplateConfig>(&draft)
            .map_err(|e| e.to_string())
            .and_then(|config| {
                quota_core::template::simulate(&config, &response_value)
                    .map_err(|e| e.message().to_string())
            }),
        AssistMode::Script => serde_json::from_str::<ScriptConfig>(&draft)
            .map_err(|e| e.to_string())
            .and_then(|config| {
                quota_core::script::simulate(&config, &response_value)
                    .map_err(|e| e.message().to_string())
            }),
    };
    match rows {
        Ok(rows) => {
            print_output(&AssistOutput {
                schema_version: 1,
                ok: true,
                stage: "simulate",
                diagnostics: Vec::new(),
                result: Some(rows),
            });
            0
        }
        Err(message) => fail("simulate", "QT_ASSIST_SIMULATE", "response", message),
    }
}

/// 真实试查一次（Agent 端测通道）：取诊断包 `entryId` 指向的已保存条目，
/// 用包内草稿覆盖其查询配置，其余字段（密文凭据两槽/baseUrl/代理开关）
/// 原样复用，走引擎完整链路真实请求。凭据密文不出 vault、不进入输出。
///
/// 与 schema/validate/simulate 的无凭据契约不同，本子命令显式使用已存
/// 凭据发起一次真实网络请求（schema 的 `assistTest` 段落同样声明）。
pub async fn run_test(
    ctx: &crate::ctx::Ctx,
    mode: AssistMode,
    input: PathBuf,
    base_url_override: Option<String>,
) -> i32 {
    let text = match read_text(&input) {
        Ok(value) => value,
        Err(message) => return fail("read", "QT_ASSIST_READ", "input", message),
    };
    let (draft, _, entry_id) = match decode_input(&text, mode) {
        Ok(value) => value,
        Err(message) => return fail("parse", "QT_ASSIST_PACKAGE", "input", message),
    };
    let Some(entry_id) = entry_id else {
        return fail(
            "parse",
            "QT_ASSIST_ENTRY_ID_REQUIRED",
            "entryId",
            "assist test 需要诊断包携带 entryId（在 GUI 中编辑已保存条目时生成的诊断包才有；未保存的新草稿请先用 QuotaTray 保存）".into(),
        );
    };
    let cfg = match quota_core::AppConfig::load(&ctx.config_path) {
        Ok(value) => value,
        Err(e) => return fail("read", "QT_ASSIST_READ", "config", e.to_string()),
    };
    let Some(entry) = cfg.providers.iter().find(|e| e.id == entry_id) else {
        return fail(
            "read",
            "QT_ASSIST_ENTRY_NOT_FOUND",
            "entryId",
            format!("本地配置中不存在条目 {entry_id}（可能与诊断包不是同一台机器）"),
        );
    };
    let mut test_entry = entry.clone();
    match mode {
        AssistMode::Template => {
            let config: TemplateConfig = match serde_json::from_str(&draft) {
                Ok(value) => value,
                Err(e) => return fail("parse", "QT_TEMPLATE_JSON", "(json)", e.to_string()),
            };
            if let Err(e) = quota_core::template::validate(&config) {
                let quota_core::TemplateError::Validation { field, reason } = e;
                return fail("validate", "QT_ASSIST_INVALID", &field, reason);
            }
            test_entry.kind = quota_core::ProviderKind::Template(Box::new(config));
        }
        AssistMode::Script => {
            let config: ScriptConfig = match serde_json::from_str(&draft) {
                Ok(value) => value,
                Err(e) => return fail("parse", "QT_SCRIPT_JSON", "(json)", e.to_string()),
            };
            if let Err(e) = quota_core::script::validate(&config) {
                return fail("validate", "QT_ASSIST_INVALID", &e.field, e.reason);
            }
            test_entry.kind = quota_core::ProviderKind::Script(Box::new(config));
        }
    }
    if let Some(base) = base_url_override {
        test_entry.base_url = Some(base);
    }

    let engine = match ctx.new_engine() {
        Ok(value) => value,
        Err(e) => return fail("internal", "QT_ASSIST_ENGINE", "(engine)", e),
    };
    let vault = match ctx.open_vault() {
        Ok(value) => value,
        Err(e) => return fail("internal", "QT_ASSIST_VAULT", "(vault)", e),
    };
    match engine.query(&vault, &test_entry).await {
        Ok(rows) => {
            print_output(&AssistOutput {
                schema_version: 1,
                ok: true,
                stage: "test",
                diagnostics: Vec::new(),
                result: Some(rows),
            });
            0
        }
        Err(e) => {
            // 退出码对齐全局三分约定：瞬时 2（可重试）/ 确定性 1；
            // detail 为已过 declared_secrets 脱敏的响应体片段，端测排查用
            let code = if e.is_transient() { 2 } else { 1 };
            fail_exit(
                "test",
                "QT_ASSIST_QUERY_FAILED",
                "(query)",
                e.message().to_string(),
                e.detail().map(str::to_string),
                code,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_input_exposes_draft_and_sample_but_no_agent_claim() {
        let package = serde_json::json!({
            "format": "quotatray-assist-package",
            "version": 1,
            "mode": "template",
            "entryId": "p7",
            "draft": "{\"request\":{\"url\":\"https://a.com\"},\"extract\":{\"remaining\":\"$.a\"}}",
            "responseSample": "{\"a\":1}"
        });
        let (draft, sample, entry_id) =
            decode_input(&package.to_string(), AssistMode::Template).unwrap();
        assert!(draft.contains("remaining"));
        assert_eq!(sample.as_deref(), Some("{\"a\":1}"));
        assert_eq!(entry_id.as_deref(), Some("p7"));
        assert_eq!(schema_value()["security"]["providesAgent"], false);
    }

    /// 契约：纯配置输入（无 format 字段）不携带 entryId——assist test
    /// 对其给出 entryId 缺失引导而非误读。
    #[test]
    fn plain_config_input_has_no_entry_id() {
        let (draft, sample, entry_id) = decode_input(
            "{\"request\":{\"url\":\"https://a.com\"}}",
            AssistMode::Template,
        )
        .unwrap();
        assert!(draft.contains("a.com"));
        assert!(sample.is_none());
        assert!(entry_id.is_none());
    }

    /// 契约：schema 声明 assistTest 端测通道（用已存凭据、真实请求），
    /// 与 assist 家族默认的无凭据契约区分。
    #[test]
    fn schema_declares_assist_test_channel() {
        let schema = schema_value();
        assert_eq!(schema["assistTest"]["usesStoredCredentials"], true);
        assert_eq!(schema["assistTest"]["performsNetworkRequests"], true);
        assert_eq!(schema["security"]["performsNetworkRequests"], false);
    }

    /// 契约：assist test 的两条无网络失败路径——纯配置（无 entryId）与
    /// entryId 指向的条目在本机不存在，均为结构化失败输出 + 退出 1。
    #[tokio::test]
    async fn assist_test_rejects_missing_entry_id_and_unknown_entry() {
        use crate::ctx::Ctx;
        use quota_core::InMemoryStore;
        use std::sync::Arc;

        let dir = std::env::temp_dir().join(format!("quota-cli-at-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = Ctx::with_store(dir.join("config.json"), Arc::new(InMemoryStore::new()));

        // 纯配置（非诊断包）：无 entryId 可复用
        let plain = dir.join("plain.json");
        std::fs::write(
            &plain,
            r#"{"request":{"url":"https://a.com"},"extract":{"remaining":"$.a"}}"#,
        )
        .unwrap();
        assert_eq!(
            run_test(&ctx, AssistMode::Template, plain.clone(), None).await,
            1
        );

        // 诊断包带 entryId 但本机配置无此条目
        let pkg = dir.join("pkg.json");
        std::fs::write(
            &pkg,
            r#"{"format":"quotatray-assist-package","version":1,"mode":"template","entryId":"ghost","draft":"{\"request\":{\"url\":\"https://a.com\"},\"extract\":{\"remaining\":\"$.a\"}}"}"#,
        )
        .unwrap();
        assert_eq!(run_test(&ctx, AssistMode::Template, pkg, None).await, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_mode_mismatch_is_rejected() {
        let package = serde_json::json!({
            "format": "quotatray-assist-package",
            "version": 1,
            "mode": "script",
            "draft": "{}"
        });
        let err = decode_input(&package.to_string(), AssistMode::Template).unwrap_err();
        assert!(err.contains("模式"));
    }

    /// 契约：schema 内嵌示例必须本身合法（可反序列化 + 静态校验通过），
    /// 演示 transforms 顶层键与 const 字段两种形态——Agent 按示例写不再需要猜 DSL。
    /// 脚本示例额外过 simulate：extract 字段名必须能被产物解析器读回
    /// （snake_case，防 camelCase 静默丢字段的回归）。
    #[test]
    fn schema_examples_are_valid_configs() {
        let schema = schema_value();
        let template: quota_core::TemplateConfig =
            serde_json::from_value(schema["template"]["example"].clone()).unwrap();
        quota_core::template::validate(&template).unwrap();
        assert!(
            template.request.headers.contains_key("New-Api-User"),
            "示例应演示第二凭据槽"
        );

        let script_config = quota_core::ScriptConfig {
            code: example_script(),
            allow_insecure: false,
        };
        quota_core::script::validate(&script_config).unwrap();
        let sample = serde_json::json!({
            "success": true,
            "data": { "display_name": "demo", "quota": 500000, "used_quota": 100000 }
        });
        let rows = quota_core::script::simulate(&script_config, &sample).unwrap();
        assert_eq!(
            rows[0].plan_name.as_deref(),
            Some("demo"),
            "字段名应可解析回"
        );
        assert_eq!(rows[0].is_valid, Some(true), "is_valid 应可解析回");
        assert_eq!(rows[0].remaining, Some(1.0));
    }

    /// 契约：schema 声明的变量含 apiKey2（与 core KNOWN_VARS 一致）。
    #[test]
    fn schema_declares_api_key2_variable() {
        let schema = schema_value();
        assert!(
            schema["template"]["variables"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "apiKey2")
        );
    }
}
