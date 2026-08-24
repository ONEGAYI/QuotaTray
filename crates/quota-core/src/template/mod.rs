//! 声明式模板（M2）：零代码接入任意平台的余额查询。
//!
//! 模板是一份 JSON 配置：描述"发什么请求、从响应取哪些字段、做哪些算术"。
//! 执行期无任何 eval；保存时经 [`validate`] 静态校验，执行期只做查表取值。
//!
//! DSL 示例（方案预研 §5.4）：
//!
//! ```json
//! {
//!   "request": {
//!     "url": "{{baseUrl}}/v1/user/info",
//!     "headers": { "Authorization": "Bearer {{apiKey}}" }
//!   },
//!   "extract": {
//!     "remaining": "$.data.totalBalance",
//!     "unit": { "const": "CNY" }
//!   },
//!   "transforms": [{ "op": "multiply", "field": "remaining", "by": 0.01 }]
//! }
//! ```

mod path;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http::{HttpClient, HttpRequest, Method};
use crate::model::{QueryError, UsageData};

/// 支持的模板变量（`{{apiKey}}` / `{{baseUrl}}`）。
pub(crate) const KNOWN_VARS: &[&str] = &["apiKey", "baseUrl"];

// ---- DSL 结构 -----------------------------------------------------------

/// 模板完整配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TemplateConfig {
    pub request: TemplateRequest,
    /// 单对象模式的字段映射；`windowsFrom` 存在时被忽略（每个窗口自带 extract）。
    #[serde(default)]
    pub extract: ExtractSpec,
    #[serde(default)]
    pub transforms: Vec<Transform>,
    /// 多窗口模式的数组来源路径（如 `$.limits`，其值必须是数组）。
    #[serde(default)]
    pub windows_from: Option<String>,
    /// 多窗口模式：每个窗口产出一条 UsageData（如 5 小时窗 + 周窗）。
    #[serde(default)]
    pub windows: Vec<WindowSpec>,
    /// 显式放开 http:// 非 loopback 端点（默认拒绝，GUI 应带警告）。
    #[serde(default)]
    pub allow_insecure: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TemplateRequest {
    #[serde(default)]
    pub method: TemplateMethod,
    /// 支持 `{{baseUrl}}` / `{{apiKey}}` 变量。
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateMethod {
    #[serde(rename = "GET")]
    #[default]
    Get,
    #[serde(rename = "POST")]
    Post,
}

impl From<TemplateMethod> for Method {
    fn from(m: TemplateMethod) -> Self {
        match m {
            TemplateMethod::Get => Method::Get,
            TemplateMethod::Post => Method::Post,
        }
    }
}

/// UsageData 各字段的来源：JSONPath 或常量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldSource {
    /// 如 `"$.data.totalBalance"`。
    Path(String),
    /// 如 `{ "const": "CNY" }`。
    Const { r#const: Value },
}

/// 字段映射集合。数值字段经 transforms 后填充 UsageData。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExtractSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_name: Option<FieldSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<FieldSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<FieldSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<FieldSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<FieldSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_valid: Option<FieldSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_message: Option<FieldSource>,
}

/// 受限算术变换，按数组顺序应用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transform {
    Multiply {
        field: NumField,
        by: f64,
    },
    Divide {
        field: NumField,
        by: f64,
    },
    Add {
        field: NumField,
        by: f64,
    },
    Sub {
        field: NumField,
        by: f64,
    },
    /// 四舍五入到指定小数位（默认 2）。
    Round {
        field: NumField,
        #[serde(default)]
        digits: Option<u32>,
    },
}

/// transform 的目标数值字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumField {
    Total,
    Used,
    Remaining,
}

/// 多窗口模式中的单个窗口。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WindowSpec {
    /// 窗口名（填充 UsageData::plan_name）。
    pub name: String,
    pub extract: ExtractSpec,
    #[serde(default)]
    pub transforms: Vec<Transform>,
}

// ---- 静态校验 -----------------------------------------------------------

/// 保存前静态校验：结构、路径语法、变量名、除零、模式一致性。
/// 返回 Err 时带定位信息，直接透出给 CLI/GUI。
pub fn validate(config: &TemplateConfig) -> Result<(), TemplateError> {
    let ctx = |field: &str| format!("模板字段 {field}：");
    let fail = |field: &str, reason: String| TemplateError::Validation {
        field: field.to_string(),
        reason,
    };

    // 请求 URL 非空 + 变量合法
    if config.request.url.trim().is_empty() {
        return Err(fail("request.url", "URL 不能为空".into()));
    }
    check_vars(&config.request.url, "request.url")?;
    for (name, value) in &config.request.headers {
        check_vars(value, &format!("request.headers.{name}"))?;
    }
    if let Some(body) = &config.request.body {
        check_vars(body, "request.body")?;
    }

    // 模式一致性：windows 与 windowsFrom 必须成对出现
    match (&config.windows_from, &config.windows) {
        (Some(_), w) if w.is_empty() => {
            return Err(fail(
                "windows",
                "windowsFrom 存在时 windows 不能为空".into(),
            ));
        }
        (None, w) if !w.is_empty() => {
            return Err(fail(
                "windowsFrom",
                "windows 非空时必须提供 windowsFrom 数组路径".into(),
            ));
        }
        _ => {}
    }

    if let Some(from) = &config.windows_from {
        check_path(from, "windowsFrom")?;
        for (i, w) in config.windows.iter().enumerate() {
            if w.name.trim().is_empty() {
                return Err(fail(&format!("windows[{i}].name"), "窗口名不能为空".into()));
            }
            check_extract(&w.extract, &format!("windows[{i}].extract"))?;
            check_transforms(&w.transforms, &format!("windows[{i}].transforms"))?;
        }
    } else {
        // 单对象模式：extract 必须有意义
        check_extract(&config.extract, "extract")?;
        check_transforms(&config.transforms, "transforms")?;
    }
    let _ = ctx; // 保留占位
    Ok(())
}

fn check_extract(extract: &ExtractSpec, field: &str) -> Result<(), TemplateError> {
    let has_numeric =
        extract.total.is_some() || extract.used.is_some() || extract.remaining.is_some();
    if !has_numeric {
        return Err(TemplateError::Validation {
            field: field.to_string(),
            reason: "至少提供 total / used / remaining 中的一个数值字段".into(),
        });
    }
    for (name, src) in [
        ("planName", &extract.plan_name),
        ("total", &extract.total),
        ("used", &extract.used),
        ("remaining", &extract.remaining),
        ("unit", &extract.unit),
        ("isValid", &extract.is_valid),
        ("invalidMessage", &extract.invalid_message),
    ] {
        if let Some(FieldSource::Path(p)) = src {
            check_path(p, &format!("{field}.{name}"))?;
        }
    }
    Ok(())
}

fn check_transforms(transforms: &[Transform], field: &str) -> Result<(), TemplateError> {
    for (i, t) in transforms.iter().enumerate() {
        let at = format!("{field}[{i}]");
        match t {
            Transform::Divide { by, .. } if *by == 0.0 => {
                return Err(TemplateError::Validation {
                    field: at,
                    reason: "除数为 0".into(),
                });
            }
            Transform::Multiply { by, .. }
            | Transform::Divide { by, .. }
            | Transform::Add { by, .. }
            | Transform::Sub { by, .. } => {
                if !by.is_finite() {
                    return Err(TemplateError::Validation {
                        field: at,
                        reason: "操作数必须为有限数".into(),
                    });
                }
            }
            Transform::Round { .. } => {}
        }
    }
    Ok(())
}

fn check_path(p: &str, field: &str) -> Result<(), TemplateError> {
    path::parse_path(p)
        .map(|_| ())
        .map_err(|reason| TemplateError::Validation {
            field: field.to_string(),
            reason,
        })
}

/// 扫描字符串中的 `{{var}}`，未知变量报错（模板保存时即发现笔误）。
fn check_vars(s: &str, field: &str) -> Result<(), TemplateError> {
    for var in extract_var_names(s) {
        if !KNOWN_VARS.contains(&var.as_str()) {
            return Err(TemplateError::Validation {
                field: field.to_string(),
                reason: format!("未知变量 {{{{{var}}}}}（支持 {KNOWN_VARS:?}）"),
            });
        }
    }
    Ok(())
}

fn extract_var_names(s: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        rest = &rest[start + 2..];
        if let Some(end) = rest.find("}}") {
            vars.push(rest[..end].trim().to_string());
            rest = &rest[end + 2..];
        } else {
            break;
        }
    }
    vars
}

/// 模板是否引用了 `{{apiKey}}`（容忍空格写法，与 validate/执行期同一解析）。
///
/// 供前端（如 GUI 试查前判断 key 是否必填）与 CLI 共用，
/// 避免各自做字面量扫描后与执行期语义漂移。
pub fn uses_api_key(config: &TemplateConfig) -> bool {
    let uses = |s: &str| extract_var_names(s).iter().any(|v| v == "apiKey");
    uses(&config.request.url)
        || config.request.headers.values().any(|v| uses(v))
        || config.request.body.as_deref().is_some_and(uses)
}

// ---- 错误 ---------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    /// 静态校验失败（保存模板时），带字段定位。
    #[error("字段 {field}：{reason}")]
    Validation { field: String, reason: String },
}

// ---- 变量替换与执行 ------------------------------------------------------

/// 执行模板查询。`api_key` 来自 vault 解密，`base_url` 为条目配置（均可缺省）。
pub(crate) async fn execute(
    http: &dyn HttpClient,
    config: &TemplateConfig,
    api_key: &str,
    base_url: Option<&str>,
) -> Result<Vec<UsageData>, QueryError> {
    let url = substitute(&config.request.url, api_key, base_url)?;
    let headers: Result<Vec<_>, _> = config
        .request
        .headers
        .iter()
        .map(|(k, v)| Ok((k.clone(), substitute(v, api_key, base_url)?)))
        .collect();
    let headers = headers?;
    let body = match &config.request.body {
        Some(b) => Some(substitute(b, api_key, base_url)?),
        None => None,
    };

    check_url_safety(&url, config.allow_insecure)?;

    let mut req = HttpRequest {
        method: config.request.method.into(),
        url,
        headers,
        body,
        // apiKey 可能被替换进任意自定义头/参数（敏感名判断覆盖不到），
        // 从根登记供错误详情脱敏做字面量替换；短占位值（如 "-"）在
        // 收集侧因长度 < 4 自然跳过
        declared_secrets: vec![api_key.to_string()],
    };
    req.headers
        .push(("Accept".into(), "application/json".into()));
    let root = crate::provider::fetch_json(http, req).await?;

    if let Some(from) = &config.windows_from {
        let items = path::resolve_path(&root, from)
            .map_err(bad_path)?
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                QueryError::deterministic(format!("windowsFrom 路径 {from} 的值不是数组"))
            })?;
        let mut result = Vec::with_capacity(items.len());
        for item in items {
            let w = config
                .windows
                .first()
                .ok_or_else(|| QueryError::deterministic("windows 为空"))?;
            // 多窗口共用第一个 WindowSpec 的映射（每条数组元素重复应用）
            let mut data = extract_usage(&w.extract, item)?;
            apply_transforms(&mut data, &w.transforms)?;
            data.plan_name = Some(w.name.clone());
            result.push(data);
        }
        if result.is_empty() {
            return Err(QueryError::deterministic("windowsFrom 数组为空"));
        }
        Ok(result)
    } else {
        let mut data = extract_usage(&config.extract, &root)?;
        apply_transforms(&mut data, &config.transforms)?;
        Ok(vec![data])
    }
}

fn bad_path(e: String) -> QueryError {
    QueryError::deterministic(format!("模板路径语法错误：{e}"))
}

/// 字符串变量替换：扫描 `{{ var }}` 占位（容忍内部空白，与 [`validate`] 的
/// 变量名解析一致），按表替换。
///
/// 安全：替换后的文本可能含明文凭据（如 URL query 中的 key），未知或未提供的
/// 变量报错时只指出变量名，绝不让替换结果进入错误信息。
fn substitute(s: &str, api_key: &str, base_url: Option<&str>) -> Result<String, QueryError> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let var = after[..end].trim();
                match var {
                    "apiKey" => out.push_str(api_key),
                    "baseUrl" => match base_url {
                        Some(base) => out.push_str(base),
                        None => {
                            return Err(QueryError::deterministic(
                                "模板变量 {{baseUrl}} 未提供（baseUrl 需在条目配置中填写）",
                            ));
                        }
                    },
                    other => {
                        return Err(QueryError::deterministic(format!(
                            "未知模板变量 {{{{{other}}}}}（支持 apiKey / baseUrl）"
                        )));
                    }
                }
                rest = &after[end + 2..];
            }
            None => {
                // 无闭合 }}：非变量占位，原样保留
                out.push_str("{{");
                rest = &rest[start + 2..];
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// URL 安全：默认仅允许 https 与 loopback；`allow_insecure` 显式放开。
///
/// 安全：拒绝文案不带 URL 全文（key 可能已被替换进 query string）。
fn check_url_safety(url: &str, allow_insecure: bool) -> Result<(), QueryError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| QueryError::deterministic(format!("URL 无法解析：{e}")))?;
    let is_loopback = matches!(
        parsed.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("::1") | Some("[::1]")
    );
    if parsed.scheme() == "https" || is_loopback {
        return Ok(());
    }
    if allow_insecure {
        Ok(())
    } else {
        Err(QueryError::deterministic(
            "URL 不是 https 也非 loopback；如确需使用请在模板中显式设置 allowInsecure: true",
        ))
    }
}

/// 从响应 JSON 按 extract 规则构建单条 UsageData。
fn extract_usage(extract: &ExtractSpec, ctx: &Value) -> Result<UsageData, QueryError> {
    let num = |src: &Option<FieldSource>, name: &str| -> Result<Option<f64>, QueryError> {
        match src {
            None => Ok(None),
            Some(FieldSource::Const { r#const: v }) => {
                let n = crate::provider::parse_num(Some(v))
                    .ok_or_else(|| format_const_error(name, v))?;
                ensure_finite(n, name)
            }
            Some(FieldSource::Path(p)) => {
                let v = path::resolve_path(ctx, p)
                    .map_err(bad_path)?
                    .ok_or_else(|| format_path_missing(p, name))?;
                let n =
                    crate::provider::parse_num(Some(v)).ok_or_else(|| format_not_number(p, v))?;
                ensure_finite(n, name)
            }
        }
    };
    let text = |src: &Option<FieldSource>| -> Result<Option<String>, QueryError> {
        match src {
            None => Ok(None),
            Some(FieldSource::Const { r#const: v }) => Ok(Some(
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string()),
            )),
            Some(FieldSource::Path(p)) => {
                let v = path::resolve_path(ctx, p)
                    .map_err(bad_path)?
                    .ok_or_else(|| format_path_missing(p, "字段"))?;
                Ok(Some(
                    v.as_str()
                        .map(String::from)
                        .unwrap_or_else(|| v.to_string()),
                ))
            }
        }
    };
    let boolean = |src: &Option<FieldSource>| -> Result<Option<bool>, QueryError> {
        match src {
            None => Ok(None),
            Some(FieldSource::Const { r#const: v }) => v
                .as_bool()
                .map(Some)
                .ok_or_else(|| QueryError::deterministic(format!("const 值不是布尔：{v}"))),
            Some(FieldSource::Path(p)) => {
                let v = path::resolve_path(ctx, p)
                    .map_err(bad_path)?
                    .ok_or_else(|| format_path_missing(p, "字段"))?;
                v.as_bool()
                    .map(Some)
                    .ok_or_else(|| QueryError::deterministic(format!("路径 {p} 的值不是布尔：{v}")))
            }
        }
    };

    Ok(UsageData {
        plan_name: text(&extract.plan_name)?,
        total: num(&extract.total, "total")?,
        used: num(&extract.used, "used")?,
        remaining: num(&extract.remaining, "remaining")?,
        unit: text(&extract.unit)?,
        reset_at: None,
        is_valid: boolean(&extract.is_valid)?,
        invalid_message: text(&extract.invalid_message)?,
        extra: None,
    })
}

fn ensure_finite(n: f64, name: &str) -> Result<Option<f64>, QueryError> {
    if n.is_finite() {
        Ok(Some(n))
    } else {
        Err(QueryError::deterministic(format!(
            "字段 {name} 的值非有限数：{n}"
        )))
    }
}

fn format_const_error(name: &str, v: &Value) -> QueryError {
    QueryError::deterministic(format!("字段 {name} 的 const 值不是数字：{v}"))
}

fn format_path_missing(p: &str, name: &str) -> QueryError {
    QueryError::deterministic(format!("{name}：路径 {p} 在响应中不存在"))
}

fn format_not_number(p: &str, v: &Value) -> QueryError {
    QueryError::deterministic(format!("路径 {p} 的值不是数字：{v}"))
}

/// 按顺序应用算术变换；非有限中间结果立即失败。
fn apply_transforms(data: &mut UsageData, transforms: &[Transform]) -> Result<(), QueryError> {
    for t in transforms {
        let field_name = match t {
            Transform::Multiply { field, .. }
            | Transform::Divide { field, .. }
            | Transform::Add { field, .. }
            | Transform::Sub { field, .. }
            | Transform::Round { field, .. } => field,
        };
        let slot = match field_name {
            NumField::Total => &mut data.total,
            NumField::Used => &mut data.used,
            NumField::Remaining => &mut data.remaining,
        };
        let current = slot.ok_or_else(|| {
            QueryError::deterministic(format!(
                "transform 目标字段 {field_name:?} 未在 extract 中提供"
            ))
        })?;
        let next = match t {
            Transform::Multiply { by, .. } => current * by,
            Transform::Divide { by, .. } => current / by,
            Transform::Add { by, .. } => current + by,
            Transform::Sub { by, .. } => current - by,
            Transform::Round { digits, .. } => {
                let d = 10f64.powi(digits.unwrap_or(2) as i32);
                (current * d).round() / d
            }
        };
        if !next.is_finite() {
            return Err(QueryError::deterministic(format!(
                "transform 计算结果非有限数（{current:?} → {next}）"
            )));
        }
        *slot = Some(next);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    fn simple_template() -> TemplateConfig {
        serde_json::from_value(serde_json::json!({
            "request": {
                "url": "{{baseUrl}}/v1/user/info",
                "headers": { "Authorization": "Bearer {{apiKey}}" }
            },
            "extract": {
                "remaining": "$.data.totalBalance",
                "unit": { "const": "CNY" },
                "planName": { "const": "Demo" }
            }
        }))
        .unwrap()
    }

    // ---- 静态校验 ---------------------------------------------------

    /// 契约：合法模板通过校验（含 camelCase 字段、const、变量）。
    #[test]
    fn validate_accepts_valid_template() {
        validate(&simple_template()).unwrap();
    }

    /// 契约：未知字段拒绝（deny_unknown_fields）。
    #[test]
    fn unknown_fields_rejected() {
        let raw = serde_json::json!({
            "request": { "url": "https://a.com" },
            "extract": { "remaining": "$.a" },
            "whatIsThis": true
        });
        let err = serde_json::from_value::<TemplateConfig>(raw).unwrap_err();
        assert!(err.to_string().contains("whatIsThis"), "{err}");
    }

    /// 契约：无数值字段 / 除零 / 未知变量 / windows 不成对 均拒绝。
    #[test]
    fn validate_rejects_common_mistakes() {
        // 无数值字段
        let mut t = simple_template();
        t.extract.remaining = None;
        assert!(validate(&t).is_err());

        // 除零
        let mut t = simple_template();
        t.transforms = vec![Transform::Divide {
            field: NumField::Remaining,
            by: 0.0,
        }];
        assert!(matches!(
            validate(&t),
            Err(TemplateError::Validation { reason, .. }) if reason.contains("除数")
        ));

        // 未知变量
        let mut t = simple_template();
        t.request.url = "{{base_url}}/x".into();
        assert!(matches!(
            validate(&t),
            Err(TemplateError::Validation { field, .. }) if field == "request.url"
        ));

        // windows 非空但无 windowsFrom
        let mut t = simple_template();
        t.windows = vec![WindowSpec {
            name: "w".into(),
            extract: t.extract.clone(),
            transforms: vec![],
        }];
        assert!(validate(&t).is_err());
    }

    // ---- 执行 -------------------------------------------------------

    const RESP: &str = r#"{"code":20000,"data":{"totalBalance":"42.50","name":"u"}}"#;

    /// 契约：单对象执行——变量替换、路径取值、字符串数字解析、const 填充。
    #[tokio::test]
    async fn executes_single_object_template() {
        let t = simple_template();
        let data = execute(
            &MockHttp::ok(RESP),
            &t,
            "sk-key",
            Some("https://api.demo.com"),
        )
        .await
        .unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].remaining, Some(42.5));
        assert_eq!(data[0].unit.as_deref(), Some("CNY"));
        assert_eq!(data[0].plan_name.as_deref(), Some("Demo"));
    }

    /// 契约：transforms 按序应用（除法换算 + 四舍五入）。
    #[tokio::test]
    async fn applies_transforms_in_order() {
        let mut t = simple_template();
        t.transforms = vec![
            Transform::Divide {
                field: NumField::Remaining,
                by: 500_000.0,
            },
            Transform::Round {
                field: NumField::Remaining,
                digits: Some(4),
            },
        ];
        let data = execute(&MockHttp::ok(RESP), &t, "k", Some("https://a.com"))
            .await
            .unwrap();
        assert_eq!(data[0].remaining, Some(0.0001)); // 42.5 / 500000 = 0.000085 → round4 = 0.0001
    }

    /// 契约：多窗口展开——windowsFrom 数组每元素一条 UsageData。
    #[tokio::test]
    async fn expands_windows_array() {
        let t: TemplateConfig = serde_json::from_value(serde_json::json!({
            "request": { "url": "https://a.com/usage" },
            "windowsFrom": "$.limits",
            "windows": [{
                "name": "five_hour",
                "extract": {
                    "total": "$.limit",
                    "remaining": "$.remaining"
                },
                "transforms": [
                    { "op": "sub", "field": "total", "by": 0 }
                ]
            }]
        }))
        .unwrap();
        validate(&t).unwrap();
        let resp = r#"{"limits":[{"limit":100,"remaining":60},{"limit":500,"remaining":120}]}"#;
        let data = execute(&MockHttp::ok(resp), &t, "k", None).await.unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].total, Some(100.0));
        assert_eq!(data[0].remaining, Some(60.0));
        assert_eq!(data[0].plan_name.as_deref(), Some("five_hour"));
        assert_eq!(data[1].total, Some(500.0));
    }

    /// 安全契约：http 非 loopback 默认拒绝，allowInsecure 放开，loopback 豁免。
    #[tokio::test]
    async fn url_safety_rules() {
        let mut t = simple_template();
        t.request.url = "http://api.demo.com/x".into();
        let err = execute(&MockHttp::ok(RESP), &t, "k", None)
            .await
            .unwrap_err();
        assert!(!err.is_transient() && err.message().contains("allowInsecure"));

        t.allow_insecure = true;
        assert!(execute(&MockHttp::ok(RESP), &t, "k", None).await.is_ok());

        t.allow_insecure = false;
        t.request.url = "http://127.0.0.1:8080/x".into();
        assert!(execute(&MockHttp::ok(RESP), &t, "k", None).await.is_ok());
    }

    /// 契约：运行期变量缺失 / 路径缺失 / 非数字值 → 确定性失败。
    #[tokio::test]
    async fn runtime_errors_are_deterministic() {
        // baseUrl 变量但调用未提供 base_url → 指名报错（不带替换后全文）
        let t = simple_template();
        let err = execute(&MockHttp::ok(RESP), &t, "k", None)
            .await
            .unwrap_err();
        assert!(!err.is_transient() && err.message().contains("baseUrl"));

        // 路径缺失
        let mut t = simple_template();
        t.extract.remaining = Some(FieldSource::Path("$.data.nonexistent".into()));
        let err = execute(&MockHttp::ok(RESP), &t, "k", Some("https://a.com"))
            .await
            .unwrap_err();
        assert!(!err.is_transient() && err.message().contains("不存在"));

        // 值非数字
        let mut t = simple_template();
        t.extract.remaining = Some(FieldSource::Path("$.data.name".into()));
        let err = execute(&MockHttp::ok(RESP), &t, "k", Some("https://a.com"))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }

    /// 安全契约：apiKey 被替换进任意自定义头（敏感名判断覆盖不到）时，
    /// 非 JSON 错误响应的 detail 中回显已被 declared_secrets 字面量打码。
    #[tokio::test]
    async fn detail_redacts_apikey_echoed_from_custom_header_template() {
        let t: TemplateConfig = serde_json::from_value(serde_json::json!({
            "request": {
                "url": "{{baseUrl}}/v1/anything",
                "headers": { "X-Custom-Auth": "{{apiKey}}" }
            },
            "extract": { "remaining": "$.data.balance" }
        }))
        .unwrap();
        let mut http = MockHttp::ok("");
        http.body = "<html>auth failed: custom-echo-secret-99</html>".into();
        let err = execute(
            &http,
            &t,
            "custom-echo-secret-99",
            Some("https://api.demo.com"),
        )
        .await
        .unwrap_err();
        assert_eq!(err.message(), "响应不是合法 JSON");
        let detail = err.detail().expect("应携带 detail");
        assert!(
            !detail.contains("custom-echo-secret-99"),
            "detail 泄漏模板 apiKey：{detail}"
        );
        assert!(detail.contains("<redacted>"), "应含打码占位：{detail}");
    }

    /// 契约：HTTP 层错误分类透传（401 确定性、网络故障瞬时）。
    #[tokio::test]
    async fn http_errors_classified() {
        let t = simple_template();
        assert!(
            !execute(&MockHttp::status(401), &t, "k", Some("https://a.com"))
                .await
                .unwrap_err()
                .is_transient()
        );
        assert!(
            execute(&MockHttp::fail(), &t, "k", Some("https://a.com"))
                .await
                .unwrap_err()
                .is_transient()
        );
    }

    /// 安全契约：任何执行错误文案不得携带明文 key——
    /// key 可能被替换进 URL query 或已进入待替换串，错误只报变量名/原因。
    #[tokio::test]
    async fn error_messages_never_leak_api_key() {
        let key = "sk-plaintext-leak";

        // 场景 1：key 已替换进 URL query，http 非 loopback 被安全检查拒绝
        let mut t = simple_template();
        t.request.url = format!("http://api.demo.com/v1?token={key}");
        let err = execute(&MockHttp::ok(RESP), &t, key, None)
            .await
            .unwrap_err();
        assert!(!err.message().contains(key), "泄漏：{err}");

        // 场景 2：未知变量出现在 key 变量之后（key 已替换进缓冲）
        let mut t = simple_template();
        t.request.url = "{{apiKey}}-{{nope}}".to_string();
        let err = execute(&MockHttp::ok(RESP), &t, key, None)
            .await
            .unwrap_err();
        assert!(!err.message().contains(key), "泄漏：{err}");
        assert!(err.message().contains("nope"), "应指名未知变量：{err}");

        // 场景 3：未知变量在前、带空格写法（validate 接受的形态执行期同样报错而非静默）
        let mut t = simple_template();
        t.request.url = "{{ oops }}{{apiKey}}".into();
        let err = execute(&MockHttp::ok(RESP), &t, key, None)
            .await
            .unwrap_err();
        assert!(!err.message().contains(key), "泄漏：{err}");
    }

    /// 契约：带空格的 `{{ apiKey }}` 写法与无空格等价（validate 与执行期一致）。
    #[tokio::test]
    async fn spaced_variable_syntax_is_equivalent() {
        let mut t = simple_template();
        t.request.url = "{{ baseUrl }}/x?k={{ apiKey }}".into();
        t.request
            .headers
            .insert("Authorization".into(), "Bearer {{ apiKey }}".into());
        let data = execute(&MockHttp::ok(RESP), &t, "sk-key", Some("https://a.com"))
            .await
            .unwrap();
        assert_eq!(data[0].remaining, Some(42.5), "带空格变量应正常替换执行");
    }

    /// 契约：uses_api_key 与执行期变量解析同一语义（含带空格写法），
    /// 供 GUI 试查/CLI 判断 key 是否必填。
    #[test]
    fn uses_api_key_matches_variable_semantics() {
        let no = serde_json::from_str::<TemplateConfig>(
            r#"{"request":{"url":"https://a.com"},"extract":{"remaining":"$.a"}}"#,
        )
        .unwrap();
        assert!(!uses_api_key(&no));

        let in_url = serde_json::from_str::<TemplateConfig>(
            r#"{"request":{"url":"https://a.com?k={{ apiKey }}"},"extract":{"remaining":"$.a"}}"#,
        )
        .unwrap();
        assert!(uses_api_key(&in_url), "带空格的 url 引用应识别");

        let in_header = serde_json::from_str::<TemplateConfig>(
            r#"{"request":{"url":"https://a.com","headers":{"Authorization":"Bearer {{apiKey}}"}},"extract":{"remaining":"$.a"}}"#,
        )
        .unwrap();
        assert!(uses_api_key(&in_header), "header 引用应识别");

        let in_body = serde_json::from_str::<TemplateConfig>(
            r#"{"request":{"url":"https://a.com","body":"{{ apiKey }}"},"extract":{"remaining":"$.a"}}"#,
        )
        .unwrap();
        assert!(uses_api_key(&in_body), "body 引用应识别");
    }
}
