//! `quota template test`：模板静态校验 + 真实试查一次。
//!
//! key 来源：`--entry` 复用已存条目（直接沿用其密文，AAD 匹配零解密）；
//! `--json` 模式配合 set-key 前的调试——模板从 stdin 读，
//! 模板引用 `{{apiKey}}` 时经 tty 交互输入（仅本次测试，不落盘）。

use quota_core::AppConfig;
use quota_core::config::{ProviderEntry, ProviderKind};
use quota_core::model::{QueryError, UsageData};
use quota_core::template::{self, TemplateConfig};
use zeroize::Zeroizing;

use crate::ctx::Ctx;
use crate::io;
use crate::render::fmt_num;

/// stdin 模式试查用临时条目 id（不落盘，仅构造引擎入参）。
const TEST_ENTRY_ID: &str = "template-test";

/// 试查输入：`--entry` 沿用源条目（密文零解密），`--json` 收集明文 key。
enum TestSource {
    Entry(Box<ProviderEntry>),
    Stdin {
        template: Box<TemplateConfig>,
        api_key: Zeroizing<String>,
        base_url: Option<String>,
    },
}

pub async fn run(
    ctx: &Ctx,
    entry_id: Option<String>,
    json_mode: bool,
    base_url_override: Option<String>,
) -> i32 {
    // 1. 收集试查输入
    let source = if let Some(id) = entry_id {
        match build_from_entry(ctx, &id, base_url_override) {
            Ok(s) => s,
            Err(msg) => {
                eprintln!("错误：{msg}");
                return 1;
            }
        }
    } else if json_mode {
        match build_from_stdin(base_url_override) {
            Ok(s) => s,
            Err(msg) => {
                eprintln!("错误：{msg}");
                return 1;
            }
        }
    } else {
        eprintln!("错误：需要 --entry <id> 或 --json 之一（quota template test --help 查看）");
        return 1;
    };

    // 2. 静态校验
    let tpl_ref = match &source {
        TestSource::Entry(e) => match &e.kind {
            ProviderKind::Template(t) => t.as_ref(),
            _ => unreachable!("build_from_entry 已保证 template 类型"),
        },
        TestSource::Stdin { template, .. } => template,
    };
    if let Err(e) = template::validate(tpl_ref) {
        eprintln!("静态校验失败：{e}");
        return 1;
    }
    println!("静态校验通过");

    // 3. 真实试查（走引擎完整链路）
    let engine = match ctx.new_engine() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("错误：{e}");
            return 1;
        }
    };
    let vault = match ctx.open_vault() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("错误：{e}");
            return 1;
        }
    };
    let test_entry = match source {
        TestSource::Entry(e) => *e,
        TestSource::Stdin {
            template,
            api_key,
            base_url,
        } => {
            let mut e = ProviderEntry {
                id: TEST_ENTRY_ID.into(),
                name: "模板试查".into(),
                kind: ProviderKind::Template(template),
                enabled: true,
                api_key_enc: None,
                base_url,
            };
            if let Err(err) = e.set_api_key(&vault, api_key.trim()) {
                eprintln!("错误：试查凭据加密失败：{err}");
                return 1;
            }
            e
        }
    };

    match engine.query(&vault, &test_entry).await {
        Ok(rows) => {
            print_usage(&rows);
            0
        }
        Err(e) => {
            eprintln!("试查失败：{e}");
            exit_code_for(&e)
        }
    }
}

/// 试查失败的退出码：瞬时 → 2（可重试），确定性 → 1（spec §4 三分约定全局适用）。
fn exit_code_for(e: &QueryError) -> i32 {
    if e.is_transient() { 2 } else { 1 }
}

/// `--entry`：直接沿用源条目（含密文与 id，AAD 匹配零解密——
/// 密文不经手、明文不出 vault）。未配 key 的错误由引擎透出。
fn build_from_entry(
    ctx: &Ctx,
    id: &str,
    base_url_override: Option<String>,
) -> Result<TestSource, String> {
    let cfg = AppConfig::load(&ctx.config_path).map_err(|e| e.to_string())?;
    let entry = cfg
        .providers
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("找不到条目 {id}"))?;
    if !matches!(entry.kind, ProviderKind::Template(_)) {
        return Err(format!("条目 {id} 不是 template 类型"));
    }
    let mut test_entry = entry.clone();
    if let Some(b) = base_url_override {
        test_entry.base_url = Some(b);
    }
    Ok(TestSource::Entry(Box::new(test_entry)))
}

/// `--json`：模板从 stdin 读；引用 `{{apiKey}}` 时交互读 key。
fn build_from_stdin(base_url_override: Option<String>) -> Result<TestSource, String> {
    let text =
        io::read_multiline_json("粘贴模板 JSON").map_err(|e| format!("stdin 读取失败：{e}"))?;
    let template: TemplateConfig =
        serde_json::from_str(&text).map_err(|e| format!("JSON 解析失败：{e}"))?;
    let api_key = if template_needs_key(&template) {
        let k = io::read_secret(
            "该模板引用 {{apiKey}}，输入测试用 key（仅本次不落盘；输入显示为星号）",
        )
        .map_err(|e| format!("key 读取失败：{e}"))?;
        if k.trim().is_empty() {
            return Err("key 为空；无 key 调试请改用 quota template test --entry".into());
        }
        k
    } else {
        Zeroizing::new(String::new())
    };
    Ok(TestSource::Stdin {
        template: Box::new(template),
        api_key,
        base_url: base_url_override,
    })
}

/// 模板文本是否引用 `{{apiKey}}`（request 的 URL/头/体）。
pub fn template_needs_key(tpl: &TemplateConfig) -> bool {
    let mut texts = vec![tpl.request.url.as_str()];
    texts.extend(tpl.request.headers.values().map(String::as_str));
    if let Some(body) = &tpl.request.body {
        texts.push(body.as_str());
    }
    texts.iter().any(|t| t.contains("{{apiKey}}"))
}

fn print_usage(rows: &[UsageData]) {
    for d in rows {
        println!(
            "套餐={} 已用={} 剩余={} 单位={} 有效={}",
            d.plan_name.clone().unwrap_or_else(|| "-".into()),
            fmt_num(d.used),
            fmt_num(d.remaining),
            d.unit.clone().unwrap_or_else(|| "-".into()),
            match d.is_valid {
                Some(false) => "否".to_string(),
                _ => "是".to_string(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tpl(url: &str) -> TemplateConfig {
        serde_json::from_value(serde_json::json!({
            "request": { "url": url },
            "extract": { "remaining": "$.a" }
        }))
        .unwrap()
    }

    /// 契约：apiKey 引用检测——URL/头/体任一处引用即需要 key。
    #[test]
    fn detects_api_key_usage() {
        assert!(!template_needs_key(&tpl("https://a.com/x")));
        assert!(template_needs_key(&tpl("https://a.com/x?key={{apiKey}}")));

        let mut t = tpl("https://a.com/x");
        t.request
            .headers
            .insert("Authorization".into(), "Bearer {{apiKey}}".into());
        assert!(template_needs_key(&t));

        let mut t = tpl("https://a.com/x");
        t.request.body = Some(r#"{"token":"{{apiKey}}"}"#.into());
        assert!(template_needs_key(&t));
    }

    /// 契约：试查失败退出码分轨（spec §4 全局三分约定）。
    #[test]
    fn exit_code_reflects_error_track() {
        assert_eq!(exit_code_for(&QueryError::transient("timeout")), 2);
        assert_eq!(exit_code_for(&QueryError::deterministic("401")), 1);
    }
}
