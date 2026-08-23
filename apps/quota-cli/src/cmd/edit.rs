//! `quota edit`：编辑条目——向导（回车=保持）或 `--enable/--disable` 快捷路径。

use dialoguer::{Confirm, Input, theme::ColorfulTheme};
use quota_core::AppConfig;
use quota_core::config::{ProviderEntry, ProviderKind};
use quota_core::template::{self, TemplateConfig};

use crate::ctx::Ctx;
use crate::io;
use crate::lang::Lang;
use crate::texts::{self, T, t};

/// 编辑输入（向导收集后的结果；None/Keep 语义 = 保持不变）。
pub enum BaseUrlEdit {
    Keep,
    Set(String),
    Clear,
}

pub struct EditInput {
    /// None = 保持当前名称。
    pub name: Option<String>,
    pub base_url: BaseUrlEdit,
    /// None = 保持当前模板。
    pub template: Option<TemplateConfig>,
    pub enabled: bool,
}

/// 将编辑输入应用到条目（纯函数，向导逻辑的可测内核）。
/// 模板/base_url 的修改只对 template 条目生效，native 条目忽略。
pub fn apply_edit(entry: &mut ProviderEntry, input: &EditInput) {
    if let Some(name) = &input.name {
        if !name.trim().is_empty() {
            entry.name = name.trim().to_string();
        }
    }
    if matches!(entry.kind, ProviderKind::Template(_)) {
        match &input.base_url {
            BaseUrlEdit::Keep => {}
            BaseUrlEdit::Set(v) => entry.base_url = Some(v.clone()),
            BaseUrlEdit::Clear => entry.base_url = None,
        }
        if let Some(new_tpl) = &input.template {
            if let ProviderKind::Template(tpl) = &mut entry.kind {
                **tpl = new_tpl.clone();
            }
        }
    }
    entry.enabled = input.enabled;
}

pub fn run(ctx: &Ctx, id: String, enable: bool, disable: bool) -> i32 {
    let lang = ctx.lang;
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    let Some(pos) = cfg.providers.iter().position(|e| e.id == id) else {
        eprintln!("{}{}", t(lang, T::Err), texts::entry_not_found(lang, &id));
        return 1;
    };

    if enable || disable {
        cfg.providers[pos].enabled = enable;
        if let Err(e) = cfg.save(&ctx.config_path) {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
        println!(
            "{}",
            texts::state_changed(lang, enable, &cfg.providers[pos].name, &id)
        );
        return 0;
    }

    // 向导基于当前值快照收集输入（结束对 cfg 的借用后再落盘修改）
    let current = cfg.providers[pos].clone();
    let input = match collect_edit_input(&current, lang) {
        Ok(i) => i,
        Err(msg) => {
            eprintln!("{}{msg}", t(lang, T::Err));
            return 1;
        }
    };
    apply_edit(&mut cfg.providers[pos], &input);

    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    println!("{}", texts::saved(lang, &cfg.providers[pos].name, &id));
    0
}

/// 向导收集编辑输入：回车=保持；base_url 输入 `-` 清空；
/// 模板粘贴无效时警告并保持原模板，其余修改照常生效。
fn collect_edit_input(current: &ProviderEntry, lang: Lang) -> Result<EditInput, String> {
    let theme = ColorfulTheme::default();

    println!("{}", t(lang, T::PasteHintEdit));

    let name = Input::<String>::with_theme(&theme)
        .with_prompt(t(lang, T::NamePromptEdit))
        .with_initial_text(&current.name)
        .interact_text()
        .map_err(|e| format!("{}{e}", t(lang, T::InputReadFail)))?;

    let (base_url, template) = if matches!(current.kind, ProviderKind::Template(_)) {
        let cur_base = current.base_url.clone().unwrap_or_default();
        let raw = Input::<String>::with_theme(&theme)
            .with_prompt(t(lang, T::BaseUrlPromptEdit))
            .with_initial_text(&cur_base)
            .allow_empty(true)
            .interact_text()
            .map_err(|e| format!("{}{e}", t(lang, T::InputReadFail)))?;
        let base = match raw.trim() {
            "-" => BaseUrlEdit::Clear,
            "" => BaseUrlEdit::Keep,
            v => BaseUrlEdit::Set(v.to_string()),
        };
        if let ProviderKind::Template(tpl) = &current.kind {
            println!("{}", t(lang, T::CurrentTemplateLabel));
            println!("{}", serde_json::to_string_pretty(tpl).unwrap_or_default());
        }
        let text = io::read_multiline_json(t(lang, T::PasteNewTplPrompt), lang)
            .map_err(|e| format!("{}{e}", t(lang, T::StdinReadFail)))?;
        let template = match parse_replacement_template(&text, lang) {
            Ok(t) => Some(t),
            Err(msg) => {
                if !text.trim().is_empty() {
                    println!("{}{msg}", t(lang, T::InvalidTplKeep));
                }
                None
            }
        };
        (base, template)
    } else {
        (BaseUrlEdit::Keep, None)
    };

    let enabled = Confirm::with_theme(&theme)
        .with_prompt(t(lang, T::EnabledConfirm))
        .default(current.enabled)
        .interact()
        .unwrap_or(current.enabled);

    Ok(EditInput {
        name: Some(name),
        base_url,
        template,
        enabled,
    })
}

/// 解析替换用模板；空输入返回 Err 以便调用方区分"保持"与"无效"。
fn parse_replacement_template(text: &str, lang: Lang) -> Result<TemplateConfig, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(t(lang, T::EmptyInputKeep).into());
    }
    let tpl: TemplateConfig =
        serde_json::from_str(trimmed).map_err(|e| format!("{}{e}", t(lang, T::JsonParseFail)))?;
    template::validate(&tpl).map_err(|e| format!("{}{e}", t(lang, T::StaticCheckFail)))?;
    Ok(tpl)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template_entry() -> ProviderEntry {
        let tpl: TemplateConfig = serde_json::from_value(serde_json::json!({
            "request": { "url": "https://a.com/x" },
            "extract": { "remaining": "$.a" }
        }))
        .unwrap();
        ProviderEntry {
            id: "e1".into(),
            name: "旧名".into(),
            kind: ProviderKind::Template(Box::new(tpl)),
            enabled: true,
            api_key_enc: None,
            base_url: Some("https://old.com".into()),
            pricing: None,
        }
    }

    /// 契约：全 Keep 输入不改变任何字段。
    #[test]
    fn keep_all_changes_nothing() {
        let mut e = template_entry();
        apply_edit(
            &mut e,
            &EditInput {
                name: None,
                base_url: BaseUrlEdit::Keep,
                template: None,
                enabled: true,
            },
        );
        assert_eq!(e, template_entry());
    }

    /// 契约：各字段独立编辑（换名 / 清空 base_url / 换模板 / 禁用）。
    #[test]
    fn applies_field_edits() {
        let mut e = template_entry();
        let new_tpl: TemplateConfig = serde_json::from_value(serde_json::json!({
            "request": { "url": "https://b.com/y" },
            "extract": { "remaining": "$.b" }
        }))
        .unwrap();
        apply_edit(
            &mut e,
            &EditInput {
                name: Some("新名".into()),
                base_url: BaseUrlEdit::Clear,
                template: Some(new_tpl),
                enabled: false,
            },
        );
        assert_eq!(e.name, "新名");
        assert_eq!(e.base_url, None);
        assert!(!e.enabled);
        match &e.kind {
            ProviderKind::Template(t) => assert!(t.request.url.contains("b.com")),
            _ => panic!("kind 不应变"),
        }
    }

    /// 契约：空名称输入不覆盖现有名称。
    #[test]
    fn blank_name_keeps_old() {
        let mut e = template_entry();
        apply_edit(
            &mut e,
            &EditInput {
                name: Some("  ".into()),
                base_url: BaseUrlEdit::Keep,
                template: None,
                enabled: true,
            },
        );
        assert_eq!(e.name, "旧名");
    }

    /// 契约：替换模板解析——空输入与非法模板都拒绝，合法模板通过（双语文案）。
    #[test]
    fn replacement_template_parsing() {
        for lang in [Lang::Zh, Lang::En] {
            let empty_err = parse_replacement_template("", lang).unwrap_err();
            assert_eq!(empty_err, t(lang, T::EmptyInputKeep), "{lang:?}");

            let bad_err = parse_replacement_template("{ bad", lang).unwrap_err();
            assert!(
                bad_err.starts_with(t(lang, T::JsonParseFail)),
                "{lang:?}: {bad_err}"
            );

            // 合法结构但无数值字段 → 校验失败
            let bad = r#"{"request":{"url":"https://a.com"},"extract":{}}"#;
            let err = parse_replacement_template(bad, lang).unwrap_err();
            assert!(
                err.starts_with(t(lang, T::StaticCheckFail)),
                "{lang:?}: {err}"
            );

            let good = r#"{"request":{"url":"https://a.com"},"extract":{"remaining":"$.a"}}"#;
            assert!(parse_replacement_template(good, lang).is_ok());
        }
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use crate::ctx::Ctx;
    use quota_core::InMemoryStore;
    use std::sync::Arc;

    fn test_ctx(tag: &str) -> Ctx {
        let dir = std::env::temp_dir().join(format!("quota-cli-edit-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let _ = std::fs::remove_file(&path);
        Ctx::with_store(path, Arc::new(InMemoryStore::new()))
    }

    fn disabled_entry(id: &str) -> ProviderEntry {
        ProviderEntry {
            id: id.into(),
            name: "n".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: false,
            api_key_enc: None,
            base_url: None,
            pricing: None,
        }
    }

    /// 契约：--enable 非交互路径落盘（disabled → enabled）。
    #[test]
    fn enable_flag_persists() {
        let ctx = test_ctx("a");
        let cfg = AppConfig {
            providers: vec![disabled_entry("e1")],
        };
        cfg.save(&ctx.config_path).unwrap();

        assert_eq!(run(&ctx, "e1".into(), true, false), 0);
        assert!(AppConfig::load(&ctx.config_path).unwrap().providers[0].enabled);

        // 再禁用回去
        assert_eq!(run(&ctx, "e1".into(), false, true), 0);
        assert!(!AppConfig::load(&ctx.config_path).unwrap().providers[0].enabled);
        let _ = std::fs::remove_dir_all(ctx.config_path.parent().unwrap());
    }

    /// 契约：编辑不存在的 id → 退出 1。
    #[test]
    fn edit_missing_fails() {
        let ctx = test_ctx("b");
        assert_eq!(run(&ctx, "zzz".into(), false, false), 1);
        let _ = std::fs::remove_dir_all(ctx.config_path.parent().unwrap());
    }
}
