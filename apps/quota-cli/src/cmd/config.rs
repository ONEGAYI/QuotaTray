//! `quota config export/import`：完整配置跨机器迁移。

use std::path::PathBuf;

use dialoguer::{Confirm, theme::ColorfulTheme};
use quota_core::{AppConfig, export_config_to_path, import_config_to_path};

use crate::ctx::Ctx;
use crate::texts::{self, T, t};

pub fn run_export(ctx: &Ctx, output: PathBuf, yes: bool) -> i32 {
    let config = match AppConfig::load(&ctx.config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{}{e}", t(ctx.lang, T::Err));
            return 1;
        }
    };
    let vault = match ctx.open_vault() {
        Ok(vault) => vault,
        Err(e) => {
            eprintln!("{}{e}", t(ctx.lang, T::Err));
            return 1;
        }
    };
    if !yes && !confirm(texts::config_export_confirm(ctx.lang, &output)) {
        println!("{}", texts::cancelled(ctx.lang));
        return 0;
    }
    match export_config_to_path(&config, &vault, None, &output) {
        Ok(()) => {
            println!("{}", texts::config_exported(ctx.lang, &output));
            0
        }
        Err(e) => {
            eprintln!("{}{e}", t(ctx.lang, T::ConfigTransferFail));
            1
        }
    }
}

pub fn run_import(ctx: &Ctx, input: PathBuf, yes: bool) -> i32 {
    if !yes && !confirm(texts::config_import_confirm(ctx.lang, &input)) {
        println!("{}", texts::cancelled(ctx.lang));
        return 0;
    }
    let vault = match ctx.open_vault() {
        Ok(vault) => vault,
        Err(e) => {
            eprintln!("{}{e}", t(ctx.lang, T::Err));
            return 1;
        }
    };
    match import_config_to_path(&input, &vault, &ctx.config_path) {
        Ok(bundle) => {
            println!(
                "{}",
                texts::config_imported(ctx.lang, &input, bundle.config.providers.len())
            );
            0
        }
        Err(e) => {
            eprintln!("{}{e}", t(ctx.lang, T::ConfigTransferFail));
            1
        }
    }
}

fn confirm(prompt: String) -> bool {
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .default(false)
        .interact()
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use quota_core::config::{PlanVariant, ProviderEntry, ProviderKind};
    use quota_core::{AppConfig, InMemoryStore};

    use super::*;

    const SECRET: &str = "sk-cli-transfer-secret";

    fn test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quota-cli-config-transfer-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn source_ctx(dir: &std::path::Path) -> Ctx {
        let ctx = Ctx::with_store(dir.join("source.json"), Arc::new(InMemoryStore::new()));
        let vault = ctx.open_vault().unwrap();
        let mut entry = ProviderEntry {
            id: "source-entry".into(),
            name: "Source Account".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        };
        entry.set_api_key(&vault, SECRET).unwrap();
        AppConfig {
            providers: vec![entry],
            custom_models: Default::default(),
        }
        .save(&ctx.config_path)
        .unwrap();
        ctx
    }

    #[test]
    fn export_yes_writes_opaque_bundle() {
        let dir = test_dir("export");
        let ctx = source_ctx(&dir);
        let output = dir.join("backup.qtray-export");

        assert_eq!(run_export(&ctx, output.clone(), true), 0);
        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.starts_with(b"QTRAYCFG"));
        assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn import_yes_replaces_and_rewraps_for_target_vault() {
        let dir = test_dir("import");
        let source = source_ctx(&dir);
        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = Ctx::with_store(dir.join("target.json"), Arc::new(InMemoryStore::new()));
        AppConfig::default().save(&target.config_path).unwrap();
        assert_eq!(run_import(&target, bundle, true), 0);

        let imported = AppConfig::load(&target.config_path).unwrap();
        assert_eq!(imported.providers.len(), 1);
        assert_eq!(
            imported.providers[0]
                .credentials(&target.open_vault().unwrap())
                .unwrap()
                .api_key
                .as_str(),
            SECRET
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupted_import_does_not_replace_existing_config() {
        let dir = test_dir("corrupted");
        let target = Ctx::with_store(dir.join("target.json"), Arc::new(InMemoryStore::new()));
        let existing = AppConfig::default();
        existing.save(&target.config_path).unwrap();
        let bundle = dir.join("bad.qtray-export");
        std::fs::write(&bundle, b"not a transfer package").unwrap();

        assert_eq!(run_import(&target, bundle, true), 1);
        assert_eq!(AppConfig::load(&target.config_path).unwrap(), existing);
        let _ = std::fs::remove_dir_all(dir);
    }
}
