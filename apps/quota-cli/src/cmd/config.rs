//! `quota config export/import`：完整配置跨机器迁移（含查询历史）。

use std::path::PathBuf;

use dialoguer::{Confirm, theme::ColorfulTheme};
use quota_core::{
    AppConfig, HistoryExportRow, HistoryStore, export_config_to_path_with_usage,
    import_config_to_path,
};

use crate::ctx::Ctx;
use crate::settings_io;
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
    // 历史随包携带；读失败降级为不带历史（导出主任务继续）。
    let history = read_history_rows(ctx);
    let usage_comparison = match settings_io::load_usage_comparison(&ctx.config_path) {
        Ok(value) => value,
        Err(e) => {
            eprintln!(
                "{}",
                texts::usage_comparison_transfer_degraded(ctx.lang, &e.to_string())
            );
            None
        }
    };
    match export_config_to_path_with_usage(
        &config,
        &vault,
        history.as_deref(),
        usage_comparison.as_deref(),
        &output,
    ) {
        Ok(()) => {
            println!("{}", texts::config_exported(ctx.lang, &output));
            0
        }
        Err(e) => {
            eprintln!("{}{e}", t(ctx.lang, T::ConfigTransferFail));
            // 超限常见根因是历史体积：附逃生提示（TooLarge 的 Display
            // 已含「超过 16 MiB 上限」文案，不重复打印）
            if matches!(e, quota_core::ConfigTransferError::TooLarge) {
                eprintln!("{}", texts::history_export_too_large_hint(ctx.lang));
            }
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
            merge_history(ctx, bundle.history.as_deref());
            if let Err(e) = settings_io::write_usage_comparison(
                &ctx.config_path,
                bundle.usage_comparison_series.as_deref(),
            ) {
                eprintln!(
                    "{}",
                    texts::usage_comparison_transfer_degraded(ctx.lang, &e.to_string())
                );
            }
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

/// 全量读取本机历史行（跨机器迁移用）；打不开/读失败告警并返回 None。
fn read_history_rows(ctx: &Ctx) -> Option<Vec<HistoryExportRow>> {
    let store = match HistoryStore::open(&ctx.history_path()) {
        Ok(store) => store,
        Err(e) => {
            eprintln!(
                "{}",
                texts::history_transfer_degraded(ctx.lang, &e.to_string())
            );
            return None;
        }
    };
    match store.export_rows() {
        Ok(rows) => Some(rows),
        Err(e) => {
            eprintln!(
                "{}",
                texts::history_transfer_degraded(ctx.lang, &e.to_string())
            );
            None
        }
    }
}

/// 迁移包携带的历史行幂等合并进本机历史库；失败仅告警（配置已导入成功）。
fn merge_history(ctx: &Ctx, rows: Option<&[HistoryExportRow]>) {
    let Some(rows) = rows else { return };
    if rows.is_empty() {
        return;
    }
    let store = match HistoryStore::open(&ctx.history_path()) {
        Ok(store) => store,
        Err(e) => {
            eprintln!(
                "{}",
                texts::history_transfer_degraded(ctx.lang, &e.to_string())
            );
            return;
        }
    };
    match store.merge_rows(rows) {
        Ok(()) => println!("{}", texts::history_merged(ctx.lang, rows.len())),
        Err(e) => eprintln!(
            "{}",
            texts::history_transfer_degraded(ctx.lang, &e.to_string())
        ),
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
            api_key2_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
            console_url: None,
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

    /// 契约：export 默认携带本机历史，import 幂等合并进目标机历史库
    /// （同条目 id 续线）。
    #[test]
    fn export_import_carries_history_rows() {
        let dir = test_dir("history");
        let source = source_ctx(&dir);
        // 源机积累两条历史点（单窗口一条时间线）
        HistoryStore::open(&source.history_path())
            .unwrap()
            .record(
                "source-entry",
                &[quota_core::UsageData {
                    plan_name: Some("five_hour".into()),
                    remaining: Some(42.0),
                    unit: Some("CNY".into()),
                    ..Default::default()
                }],
                1_700_000_000_000,
            )
            .unwrap();

        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = Ctx::with_store(dir.join("target.json"), Arc::new(InMemoryStore::new()));
        AppConfig::default().save(&target.config_path).unwrap();
        assert_eq!(run_import(&target, bundle, true), 0);

        let points = HistoryStore::open(&target.history_path())
            .unwrap()
            .range("source-entry", 0)
            .unwrap();
        assert_eq!(points.len(), 1, "历史随迁移包到达目标机");
        assert_eq!(points[0].remaining, Some(42.0));
        // v1 老包兼容（history=None 不合并）由 core transfer 测试覆盖

        let _ = std::fs::remove_dir_all(dir);
    }
}
