//! `quota add`：添加供应商——交互向导或 `--json` 从 stdin 读入。

use dialoguer::{Input, Select, theme::ColorfulTheme};
use quota_core::AppConfig;
use quota_core::config::{ProviderEntry, ProviderKind};
use quota_core::template::{self, TemplateConfig};

use crate::ctx::Ctx;
use crate::idgen;
use crate::io;
use crate::lang::Lang;
use crate::texts::{self, T, t};

pub fn run(ctx: &Ctx, json_mode: bool) -> i32 {
    let lang = ctx.lang;
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };

    let entry = if json_mode {
        let mut text = String::new();
        if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text) {
            eprintln!("{}{}{e}", t(lang, T::Err), t(lang, T::StdinReadFail));
            return 1;
        }
        match parse_entry_json(&text, lang) {
            Ok(e) => e,
            Err(msg) => {
                eprintln!("{}{msg}", t(lang, T::Err));
                return 1;
            }
        }
    } else {
        let existing_ids: Vec<String> = cfg.providers.iter().map(|e| e.id.clone()).collect();
        match wizard(ctx, &existing_ids) {
            Ok(e) => e,
            Err(msg) => {
                eprintln!("{}{msg}", t(lang, T::Err));
                return 1;
            }
        }
    };

    if let Err(msg) = check_entry(&entry, &cfg, lang) {
        eprintln!("{}{msg}", t(lang, T::Err));
        return 1;
    }

    let id = entry.id.clone();
    let name = entry.name.clone();
    let key_missing = entry.api_key_enc.is_none();
    cfg.providers.push(entry);
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    println!("{}", texts::added(lang, &name, &id));
    if key_missing {
        println!("{}", texts::key_missing_hint(lang, &id));
    }
    0
}

/// 校验新条目：名称与 id 非空 + id 唯一（模板合法性已在解析时校验）。
fn check_entry(entry: &ProviderEntry, cfg: &AppConfig, lang: Lang) -> Result<(), String> {
    if entry.id.trim().is_empty() {
        return Err(t(lang, T::IdEmptyHint).into());
    }
    if entry.name.trim().is_empty() {
        return Err(t(lang, T::NameEmpty).into());
    }
    if cfg.providers.iter().any(|e| e.id == entry.id) {
        return Err(texts::id_exists(lang, &entry.id));
    }
    Ok(())
}

/// 解析 `--json` 模式的 stdin 输入并做静态校验。
///
/// 安全红线：输入含非空 `api_key_enc` 直接拒绝——CLI 不经手密文，
/// 凭据只能经 `set-key`（vault 加密）写入。
pub fn parse_entry_json(text: &str, lang: Lang) -> Result<ProviderEntry, String> {
    let entry: ProviderEntry = serde_json::from_str(text).map_err(|e| {
        format!(
            "{}{e}{}",
            t(lang, T::JsonParseFail),
            t(lang, T::EntryJsonFields)
        )
    })?;
    if entry
        .api_key_enc
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return Err(t(lang, T::ApiKeyEncRejected).into());
    }
    if let ProviderKind::Template(tpl) = &entry.kind {
        template::validate(tpl).map_err(|e| format!("{}{e}", t(lang, T::TplValidateFail)))?;
    }
    Ok(entry)
}

/// 交互向导：名称 → 类型 → （模板 JSON + base_url）→ key（可跳过）。
/// `existing_ids` 为当前配置中的全部条目 id（新 id 生成需避开）。
fn wizard(ctx: &Ctx, existing_ids: &[String]) -> Result<ProviderEntry, String> {
    let lang = ctx.lang;
    let theme = ColorfulTheme::default();

    println!("{}", t(lang, T::PasteHintA));
    println!("{}", t(lang, T::PasteHintB));

    let name = Input::<String>::with_theme(&theme)
        .with_prompt(t(lang, T::NamePromptAdd))
        .validate_with(|s: &String| {
            if s.trim().is_empty() {
                Err(t(lang, T::NameEmpty))
            } else {
                Ok(())
            }
        })
        .interact_text()
        .map_err(|e| format!("{}{e}", t(lang, T::InputReadFail)))?;

    let metas = quota_core::provider::metas();
    // 连接符随语言（zh 全角双破折号 / en 单破折号），与 TemplateOption 排版一致
    let mut items: Vec<String> = metas
        .iter()
        .map(|m| format!("{} {} {}", m.id, t(lang, T::Dash), m.name))
        .collect();
    items.push(t(lang, T::TemplateOption).into());
    let sel = Select::with_theme(&theme)
        .items(&items)
        .default(0)
        .with_prompt(t(lang, T::TypePrompt))
        .interact()
        .map_err(|e| format!("{}{e}", t(lang, T::SelectReadFail)))?;

    let (kind, base_url) = if sel < metas.len() {
        (
            ProviderKind::Native {
                provider: metas[sel].id.to_string(),
            },
            None,
        )
    } else {
        let tpl = prompt_template(lang)?;
        let raw = Input::<String>::with_theme(&theme)
            .with_prompt(t(lang, T::BaseUrlPromptAdd))
            .allow_empty(true)
            .interact_text()
            .map_err(|e| format!("{}{e}", t(lang, T::InputReadFail)))?;
        let base_url = raw.trim().to_string();
        (
            ProviderKind::Template(Box::new(tpl)),
            (!base_url.is_empty()).then_some(base_url),
        )
    };

    // key 可跳过（回车空值，稍后 set-key 补配）；读取失败与主动跳过区分开
    let key = io::read_secret(t(lang, T::KeyPromptSkip), lang)
        .map_err(|e| format!("{}{e}", t(lang, T::KeyReadFail)))?;
    assemble_entry(
        ctx,
        name.trim().to_string(),
        kind,
        base_url,
        Some(key.trim().to_string()),
        existing_ids,
    )
}

/// 组装向导条目。
///
/// **顺序契约：id 必须先于 `set_api_key` 确定**——密文 AAD 绑定条目 id，
/// 曾因先加密后生成 id（AAD=空串）导致向导创建的条目全部解密失败，
/// 由 [`tests::wizard_entry_decrypts`] 锁定。
pub fn assemble_entry(
    ctx: &Ctx,
    name: String,
    kind: ProviderKind,
    base_url: Option<String>,
    key: Option<String>,
    existing_ids: &[String],
) -> Result<ProviderEntry, String> {
    let lang = ctx.lang;
    let mut entry = ProviderEntry {
        id: idgen::unique_id(existing_ids).map_err(|e| format!("{}{e}", t(lang, T::IdGenFail)))?,
        name,
        kind,
        enabled: true,
        api_key_enc: None,
        base_url,
        pricing: None,
    };
    if let Some(k) = key.as_deref().filter(|k| !k.is_empty()) {
        let vault = ctx.open_vault()?;
        entry
            .set_api_key(&vault, k)
            .map_err(|e| format!("{}{e}", t(lang, T::EncryptFail)))?;
    }
    Ok(entry)
}

/// 粘贴模板 JSON，解析 + 静态校验失败时提示并重试（Ctrl+C 放弃）。
fn prompt_template(lang: Lang) -> Result<TemplateConfig, String> {
    loop {
        let text = io::read_multiline_json(t(lang, T::PasteTemplateJson), lang)
            .map_err(|e| format!("{}{e}", t(lang, T::StdinReadFail)))?;
        match serde_json::from_str::<TemplateConfig>(&text) {
            Ok(tpl) => match template::validate(&tpl) {
                Ok(()) => return Ok(tpl),
                Err(e) => println!(
                    "{}{e}\n{}",
                    t(lang, T::ValidateFail),
                    t(lang, T::RetrySuffix)
                ),
            },
            Err(e) => println!(
                "{}{e}\n{}",
                t(lang, T::JsonParseFail),
                t(lang, T::RetrySuffix)
            ),
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
        for lang in [Lang::Zh, Lang::En] {
            let e = parse_entry_json(
                &entry_json(serde_json::json!({ "type": "native", "provider": "deepseek" })),
                lang,
            )
            .unwrap();
            assert_eq!(e.id, "e1");

            let tpl = serde_json::json!({
                "type": "template",
                "request": { "url": "https://a.com/x" },
                "extract": { "remaining": "$.a" }
            });
            let r = parse_entry_json(&entry_json(tpl), lang);
            assert!(r.is_ok(), "{lang:?}: {r:?}");
        }
    }

    /// 安全契约：携带 api_key_enc 的输入被拒绝（密文不经手）——双语文案。
    #[test]
    fn rejects_api_key_enc() {
        let raw = serde_json::json!({
            "id": "e1",
            "name": "x",
            "kind": { "type": "native", "provider": "deepseek" },
            "api_key_enc": "v1:AAAA"
        })
        .to_string();
        for lang in [Lang::Zh, Lang::En] {
            let err = parse_entry_json(&raw, lang).unwrap_err();
            assert!(err.contains("api_key_enc"), "{lang:?}: {err}");
            assert_eq!(err, t(lang, T::ApiKeyEncRejected), "{lang:?}: {err}");
        }
        // 空字符串视为未配置，放行
        let raw = serde_json::json!({
            "id": "e1",
            "name": "x",
            "kind": { "type": "native", "provider": "deepseek" },
            "api_key_enc": ""
        })
        .to_string();
        assert!(parse_entry_json(&raw, Lang::Zh).is_ok());
    }

    /// 契约：坏 JSON、非法模板在解析期被拒（双语文案）。
    #[test]
    fn rejects_bad_json_and_invalid_template() {
        for lang in [Lang::Zh, Lang::En] {
            let err = parse_entry_json("{ not json", lang).unwrap_err();
            assert!(
                err.starts_with(t(lang, T::JsonParseFail)),
                "{lang:?}: {err}"
            );

            let bad_tpl = serde_json::json!({
                "type": "template",
                "request": { "url": "https://a.com/x" },
                "extract": {}
            });
            let err = parse_entry_json(&entry_json(bad_tpl), lang).unwrap_err();
            assert!(
                err.starts_with(t(lang, T::TplValidateFail)),
                "{lang:?}: {err}"
            );
        }
    }

    /// 契约（顺序锁定）：向导组装的条目必须能被自己的 vault 解密——
    /// 曾因 set_api_key 先于 id 生成（AAD=空串）导致向导条目全部解密失败。
    #[test]
    fn wizard_entry_decrypts() {
        use crate::ctx::Ctx;
        use quota_core::InMemoryStore;
        use std::sync::Arc;

        let ctx = Ctx::with_store(
            std::path::PathBuf::from("unused.json"),
            Arc::new(InMemoryStore::new()),
        );
        let entry = assemble_entry(
            &ctx,
            "向导条目".into(),
            ProviderKind::Native {
                provider: "deepseek".into(),
            },
            None,
            Some("sk-wizard-key".into()),
            &[],
        )
        .unwrap();

        assert_eq!(entry.id.len(), 6, "id 应为 6 位：{entry:?}");
        let vault = ctx.open_vault().unwrap();
        let creds = entry.credentials(&vault).expect("向导条目必须可解密");
        assert_eq!(creds.api_key.as_str(), "sk-wizard-key");

        // key 为空/None = 跳过，不写密文
        let no_key = assemble_entry(
            &ctx,
            "无 key".into(),
            ProviderKind::Native {
                provider: "deepseek".into(),
            },
            None,
            None,
            &[],
        )
        .unwrap();
        assert!(no_key.api_key_enc.is_none());
    }

    /// 契约：名称空 / id 冲突被 check_entry 拦截（双语文案）。
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
            pricing: None,
        });

        for lang in [Lang::Zh, Lang::En] {
            let mut e = cfg.providers[0].clone();
            e.name = " ".into();
            assert_eq!(
                check_entry(&e, &cfg, lang).unwrap_err(),
                t(lang, T::NameEmpty)
            );

            let mut e2 = cfg.providers[0].clone();
            e2.id = "".into();
            assert_eq!(
                check_entry(&e2, &cfg, lang).unwrap_err(),
                t(lang, T::IdEmptyHint)
            );

            let e2 = ProviderEntry {
                id: "dup".into(),
                name: "ok".into(),
                kind: ProviderKind::Native {
                    provider: "deepseek".into(),
                },
                enabled: true,
                api_key_enc: None,
                base_url: None,
                pricing: None,
            };
            assert_eq!(
                check_entry(&e2, &cfg, lang).unwrap_err(),
                texts::id_exists(lang, "dup")
            );
        }
    }
}
