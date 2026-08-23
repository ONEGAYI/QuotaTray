//! `quota add`：添加供应商——交互向导或 `--json` 从 stdin 读入。

use dialoguer::{Input, Select, theme::ColorfulTheme};
use quota_core::config::{ProviderEntry, ProviderKind};
use quota_core::template::{self, TemplateConfig};
use quota_core::{AppConfig, TemplateError};

use crate::ctx::Ctx;
use crate::idgen;
use crate::io;

pub fn run(ctx: &Ctx, json_mode: bool) -> i32 {
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("错误：{e}");
            return 1;
        }
    };

    let entry = if json_mode {
        let mut text = String::new();
        if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text) {
            eprintln!("错误：stdin 读取失败：{e}");
            return 1;
        }
        match parse_entry_json(&text) {
            Ok(e) => e,
            Err(msg) => {
                eprintln!("错误：{msg}");
                return 1;
            }
        }
    } else {
        let existing_ids: Vec<String> = cfg.providers.iter().map(|e| e.id.clone()).collect();
        match wizard(ctx, &existing_ids) {
            Ok(e) => e,
            Err(msg) => {
                eprintln!("错误：{msg}");
                return 1;
            }
        }
    };

    if let Err(msg) = check_entry(&entry, &cfg) {
        eprintln!("错误：{msg}");
        return 1;
    }

    let id = entry.id.clone();
    let name = entry.name.clone();
    let key_missing = entry.api_key_enc.is_none();
    cfg.providers.push(entry);
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("错误：{e}");
        return 1;
    }
    println!("已添加：{name}（id: {id}）");
    if key_missing {
        println!("提示：尚未配置 API key，运行 quota set-key {id}");
    }
    0
}

/// 校验新条目：名称与 id 非空 + id 唯一（模板合法性已在解析时校验）。
fn check_entry(entry: &ProviderEntry, cfg: &AppConfig) -> Result<(), String> {
    if entry.id.trim().is_empty() {
        return Err("id 不能为空（--json 模式需提供非空 id 字段）".into());
    }
    if entry.name.trim().is_empty() {
        return Err("名称不能为空".into());
    }
    if cfg.providers.iter().any(|e| e.id == entry.id) {
        return Err(format!("id {} 已存在", entry.id));
    }
    Ok(())
}

/// 解析 `--json` 模式的 stdin 输入并做静态校验。
///
/// 安全红线：输入含非空 `api_key_enc` 直接拒绝——CLI 不经手密文，
/// 凭据只能经 `set-key`（vault 加密）写入。
pub fn parse_entry_json(text: &str) -> Result<ProviderEntry, String> {
    let entry: ProviderEntry = serde_json::from_str(text)
        .map_err(|e| format!("JSON 解析失败：{e}（entry.json 需含 id、name、kind 字段）"))?;
    if entry
        .api_key_enc
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Err(
            "entry.json 不应包含 api_key_enc（密文不经手）；请移除该字段，用 quota set-key 配置凭据"
                .into(),
        );
    }
    if let ProviderKind::Template(tpl) = &entry.kind {
        template::validate(tpl).map_err(template_err)?;
    }
    Ok(entry)
}

fn template_err(e: TemplateError) -> String {
    format!("模板校验失败：{e}")
}

/// 交互向导：名称 → 类型 → （模板 JSON + base_url）→ key（可跳过）。
/// `existing_ids` 为当前配置中的全部条目 id（新 id 生成需避开）。
fn wizard(ctx: &Ctx, existing_ids: &[String]) -> Result<ProviderEntry, String> {
    let theme = ColorfulTheme::default();

    println!("粘贴提示：名称 / base_url 输入框请用 Shift+Ctrl+V 或鼠标右键（Ctrl+V 在此不生效）；");
    println!("          API key 输入支持 Ctrl+V 粘贴，回显为星号。");

    let name = Input::<String>::with_theme(&theme)
        .with_prompt("名称")
        .validate_with(|s: &String| {
            if s.trim().is_empty() {
                Err("名称不能为空")
            } else {
                Ok(())
            }
        })
        .interact_text()
        .map_err(|e| format!("输入读取失败：{e}"))?;

    let metas = quota_core::provider::metas();
    let mut items: Vec<String> = metas
        .iter()
        .map(|m| format!("{} —— {}", m.id, m.name))
        .collect();
    items.push("template —— 自定义 JSON 模板".into());
    let sel = Select::with_theme(&theme)
        .items(&items)
        .default(0)
        .with_prompt("类型")
        .interact()
        .map_err(|e| format!("选择读取失败：{e}"))?;

    let (kind, base_url) = if sel < metas.len() {
        (
            ProviderKind::Native {
                provider: metas[sel].id.to_string(),
            },
            None,
        )
    } else {
        let tpl = prompt_template()?;
        let raw = Input::<String>::with_theme(&theme)
            .with_prompt("base_url（模板 {{baseUrl}} 变量来源，可空）")
            .allow_empty(true)
            .interact_text()
            .map_err(|e| format!("输入读取失败：{e}"))?;
        let base_url = raw.trim().to_string();
        (
            ProviderKind::Template(Box::new(tpl)),
            (!base_url.is_empty()).then_some(base_url),
        )
    };

    let mut entry = ProviderEntry {
        id: String::new(),
        name: name.trim().to_string(),
        kind,
        enabled: true,
        api_key_enc: None,
        base_url,
    };

    // key 可跳过（回车空值，稍后 set-key 补配）；读取失败与主动跳过区分开
    let key = io::read_secret("API key（直接回车跳过；输入显示为星号）")
        .map_err(|e| format!("key 读取失败：{e}"))?;
    if !key.trim().is_empty() {
        let vault = ctx.open_vault()?;
        entry
            .set_api_key(&vault, key.trim())
            .map_err(|e| format!("凭据加密失败：{e}"))?;
    }

    entry.id = idgen::unique_id(existing_ids).map_err(|e| format!("id 生成失败：{e}"))?;
    Ok(entry)
}

/// 粘贴模板 JSON，解析 + 静态校验失败时提示并重试（Ctrl+C 放弃）。
fn prompt_template() -> Result<TemplateConfig, String> {
    loop {
        let text =
            io::read_multiline_json("粘贴模板 JSON").map_err(|e| format!("stdin 读取失败：{e}"))?;
        match serde_json::from_str::<TemplateConfig>(&text) {
            Ok(tpl) => match template::validate(&tpl) {
                Ok(()) => return Ok(tpl),
                Err(e) => println!("静态校验未通过：{e}\n请修正后重新粘贴（Ctrl+C 放弃）"),
            },
            Err(e) => println!("JSON 解析失败：{e}\n请修正后重新粘贴（Ctrl+C 放弃）"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_json(kind: serde_json::Value) -> String {
        serde_json::json!({
            "id": "e1",
            "name": "测试",
            "kind": kind,
            "enabled": true
        })
        .to_string()
    }

    /// 契约：合法 native / template entry 可解析。
    #[test]
    fn parses_valid_entry_json() {
        let e = parse_entry_json(&entry_json(
            serde_json::json!({ "type": "native", "provider": "deepseek" }),
        ))
        .unwrap();
        assert_eq!(e.id, "e1");

        let tpl = serde_json::json!({
            "type": "template",
            "request": { "url": "https://a.com/x" },
            "extract": { "remaining": "$.a" }
        });
        assert!(parse_entry_json(&entry_json(tpl)).is_ok());
    }

    /// 安全契约：携带 api_key_enc 的输入被拒绝（密文不经手）。
    #[test]
    fn rejects_api_key_enc() {
        let raw = serde_json::json!({
            "id": "e1",
            "name": "x",
            "kind": { "type": "native", "provider": "deepseek" },
            "api_key_enc": "v1:AAAA"
        })
        .to_string();
        let err = parse_entry_json(&raw).unwrap_err();
        assert!(err.contains("api_key_enc"), "{err}");
        // 空字符串视为未配置，放行
        let raw = serde_json::json!({
            "id": "e1",
            "name": "x",
            "kind": { "type": "native", "provider": "deepseek" },
            "api_key_enc": ""
        })
        .to_string();
        assert!(parse_entry_json(&raw).is_ok());
    }

    /// 契约：坏 JSON、非法模板在解析期被拒。
    #[test]
    fn rejects_bad_json_and_invalid_template() {
        assert!(parse_entry_json("{ not json").is_err());

        let bad_tpl = serde_json::json!({
            "type": "template",
            "request": { "url": "https://a.com/x" },
            "extract": {}
        });
        let err = parse_entry_json(&entry_json(bad_tpl)).unwrap_err();
        assert!(err.contains("模板校验失败"), "{err}");
    }

    /// 契约：名称空 / id 冲突被 check_entry 拦截。
    #[test]
    fn check_entry_rejects_blank_name_and_dup_id() {
        let mut cfg = AppConfig::default();
        cfg.providers.push(ProviderEntry {
            id: "dup".into(),
            name: "a".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
        });

        let mut e = cfg.providers[0].clone();
        e.name = " ".into();
        assert!(check_entry(&e, &cfg).unwrap_err().contains("名称"));

        let mut e2 = cfg.providers[0].clone();
        e2.id = "".into();
        assert!(check_entry(&e2, &cfg).unwrap_err().contains("id 不能为空"));

        let e2 = ProviderEntry {
            id: "dup".into(),
            name: "ok".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
        };
        assert!(check_entry(&e2, &cfg).unwrap_err().contains("已存在"));
    }
}
