//! `quota template test`：模板静态校验 + 真实试查一次。
//!
//! key 来源：`--entry` 复用已存条目（vault 解密）；
//! `--json` 模式配合 set-key 前的调试——模板从 stdin 读，
//! 模板引用 `{{apiKey}}` 时经 tty 交互输入（仅本次测试，不落盘）。

use quota_core::AppConfig;
use quota_core::config::{ProviderEntry, ProviderKind};
use quota_core::model::UsageData;
use quota_core::template::{self, TemplateConfig};

use crate::ctx::Ctx;
use crate::io;
use crate::render::fmt_num;

/// 试查用临时条目 id（不落盘，仅构造引擎入参）。
const TEST_ENTRY_ID: &str = "template-test";

pub async fn run(
    ctx: &Ctx,
    entry_id: Option<String>,
    json_mode: bool,
    base_url_override: Option<String>,
) -> i32 {
    // 1. 收集模板 + key + baseUrl
    let gathered = if let Some(id) = entry_id {
        match gather_from_entry(ctx, &id, base_url_override) {
            Ok(g) => g,
            Err(msg) => {
                eprintln!("错误：{msg}");
                return 1;
            }
        }
    } else if json_mode {
        match gather_from_stdin(base_url_override) {
            Ok(g) => g,
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
    if let Err(e) = template::validate(&gathered.template) {
        eprintln!("静态校验失败：{e}");
        return 1;
    }
    println!("静态校验通过");

    // 3. 真实试查（构造临时条目走引擎完整链路：加密→解密→HTTP→解析）
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
    let mut test_entry = ProviderEntry {
        id: TEST_ENTRY_ID.into(),
        name: "模板试查".into(),
        kind: ProviderKind::Template(Box::new(gathered.template.clone())),
        enabled: true,
        api_key_enc: None,
        base_url: gathered.base_url,
    };
    if let Err(e) = test_entry.set_api_key(&vault, &gathered.api_key) {
        eprintln!("错误：试查凭据加密失败：{e}");
        return 1;
    }

    match engine.query(&vault, &test_entry).await {
        Ok(rows) => {
            print_usage(&rows);
            0
        }
        Err(e) => {
            eprintln!("试查失败：{e}");
            1
        }
    }
}

struct Gathered {
    template: TemplateConfig,
    api_key: String,
    base_url: Option<String>,
}

fn gather_from_entry(
    ctx: &Ctx,
    id: &str,
    base_url_override: Option<String>,
) -> Result<Gathered, String> {
    let cfg = AppConfig::load(&ctx.config_path).map_err(|e| e.to_string())?;
    let entry = cfg
        .providers
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| format!("找不到条目 {id}"))?;
    let ProviderKind::Template(tpl) = &entry.kind else {
        return Err(format!("条目 {id} 不是 template 类型"));
    };
    let vault = ctx.open_vault()?;
    let key = entry
        .credentials(&vault)
        .map_err(|e| format!("凭据不可用：{}", e.message()))?
        .api_key
        .to_string();
    Ok(Gathered {
        template: (**tpl).clone(),
        api_key: key,
        base_url: base_url_override.or_else(|| entry.base_url.clone()),
    })
}

fn gather_from_stdin(base_url_override: Option<String>) -> Result<Gathered, String> {
    let text =
        io::read_multiline_json("粘贴模板 JSON").map_err(|e| format!("stdin 读取失败：{e}"))?;
    let tpl: TemplateConfig =
        serde_json::from_str(&text).map_err(|e| format!("JSON 解析失败：{e}"))?;
    let api_key = if template_needs_key(&tpl) {
        let k = io::read_secret("该模板引用 {{apiKey}}，输入测试用 key（仅本次，不落盘）")
            .map_err(|e| format!("key 读取失败：{e}"))?;
        if k.trim().is_empty() {
            return Err("key 为空；无 key 调试请改用 quota template test --entry".into());
        }
        k.trim().to_string()
    } else {
        String::new()
    };
    Ok(Gathered {
        template: tpl,
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
}
