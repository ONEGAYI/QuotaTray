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
    print_output(&AssistOutput::<Value> {
        schema_version: 1,
        ok: false,
        stage,
        diagnostics: vec![Diagnostic {
            code,
            field: field.into(),
            message,
        }],
        result: None,
    });
    1
}

fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))
}

/// 同时接受纯配置文件与 GUI 导出的 quotatray-assist-package。
fn decode_input(text: &str, expected_mode: AssistMode) -> Result<(String, Option<String>), String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("JSON 解析失败：{e}"))?;
    if value.get("format").and_then(Value::as_str) != Some("quotatray-assist-package") {
        return Ok((text.to_owned(), None));
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
    Ok((draft.to_owned(), sample))
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
        "template": {
            "variables": ["apiKey", "baseUrl"],
            "methods": ["GET", "POST"],
            "extractFields": ["planName", "total", "used", "remaining", "unit", "isValid", "invalidMessage"],
            "transforms": ["multiply", "divide", "add", "sub", "round"]
        },
        "script": {
            "requiredFunctions": ["request", "extract"],
            "networkApisAvailable": false,
            "maxCodeBytes": quota_core::script::MAX_CODE_BYTES,
            "usageFields": ["plan_name", "total", "used", "remaining", "unit", "reset_at", "is_valid", "invalid_message", "extra"]
        }
    })
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
    let (draft, _) = match decode_input(&text, mode) {
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
    let (draft, package_sample) = match decode_input(&text, mode) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_input_exposes_draft_and_sample_but_no_agent_claim() {
        let package = serde_json::json!({
            "format": "quotatray-assist-package",
            "version": 1,
            "mode": "template",
            "draft": "{\"request\":{\"url\":\"https://a.com\"},\"extract\":{\"remaining\":\"$.a\"}}",
            "responseSample": "{\"a\":1}"
        });
        let (draft, sample) = decode_input(&package.to_string(), AssistMode::Template).unwrap();
        assert!(draft.contains("remaining"));
        assert_eq!(sample.as_deref(), Some("{\"a\":1}"));
        assert_eq!(schema_value()["security"]["providesAgent"], false);
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
}
