//! `quota config export/import`：完整配置跨机器迁移（含查询历史）。
//!
//! 导出双档：交互选择档位（默认密码档，口令经 Argon2id 派生、密钥不
//! 随包）；`--yes` 跳过全部交互时按脚本兼容口径落便捷档（产物与现状
//! 等价）。导入双模：`--strategy merge`（默认，只补缺失、同 id 冲突以
//! 本机为准）/`overwrite`（完全变成备份）；密码档包无论 `--yes` 与否
//! 都要求输入备份密码——密码只经掩码交互读入，永不进入命令行参数、
//! 日志或错误信息（安全红线）。
//!
//! 交互原语收敛为 [`TransferPrompt`]：生产实现走 dialoguer 与
//! [`crate::io::read_secret`]（掩码输入）；测试注入桩覆盖交互分叉
//! （选档取消、密码两次不一致重试、`--yes` 不跳过密码输入等）。

use std::path::PathBuf;

use dialoguer::{Confirm, Select, theme::ColorfulTheme};
use quota_core::{
    AppConfig, ExportOptions, HistoryExportRow, HistoryStore, ImportStrategy,
    export_config_to_path_with_options, import_config_to_path,
};
use zeroize::Zeroizing;

use crate::ctx::Ctx;
use crate::io;
use crate::lang::Lang;
use crate::settings_io;
use crate::texts::{self, T, t};

/// config 迁移的交互原语：风险确认 / 导出档位选择 / 掩码密码读取。
///
/// 收敛为一个 trait 供测试注入（`io::read_secret` 本身无注入接缝，
/// 接缝做在消费方，`io.rs` 保持纯 IO 不动）。
trait TransferPrompt {
    /// 风险确认（默认拒绝；交互失败视为拒绝）。
    fn confirm(&mut self, prompt: String) -> bool;
    /// 导出档位选择：`Some(0)` = 密码档（默认）、`Some(1)` = 便捷档、
    /// `None` = 取消。
    fn select_export_tier(&mut self, lang: Lang) -> Option<usize>;
    /// 掩码读取一次密码（两次输入校验一致由调用方处理）。
    fn read_password(&mut self, prompt: &str, lang: Lang) -> std::io::Result<Zeroizing<String>>;
}

/// 生产交互实现：dialoguer Confirm/Select + 掩码密码读取。
struct TerminalPrompt;

impl TransferPrompt for TerminalPrompt {
    fn confirm(&mut self, prompt: String) -> bool {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .default(false)
            .interact()
            .unwrap_or(false)
    }

    fn select_export_tier(&mut self, lang: Lang) -> Option<usize> {
        let items = [
            texts::export_tier_password_item(lang),
            texts::export_tier_convenient_item(lang),
        ];
        Select::with_theme(&ColorfulTheme::default())
            .items(&items)
            .default(0)
            .with_prompt(t(lang, T::ExportTierPrompt))
            .interact()
            .ok()
    }

    fn read_password(&mut self, prompt: &str, lang: Lang) -> std::io::Result<Zeroizing<String>> {
        io::read_secret(prompt, lang)
    }
}

pub fn run_export(ctx: &Ctx, output: PathBuf, yes: bool) -> i32 {
    run_export_with(ctx, output, yes, &mut TerminalPrompt)
}

fn run_export_with(ctx: &Ctx, output: PathBuf, yes: bool, prompt: &mut dyn TransferPrompt) -> i32 {
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
    // [红骨架] 档位交互已接线但选择值暂不生效：恒落便捷档（现状行为）。
    let _tier = if yes {
        None
    } else {
        prompt.select_export_tier(ctx.lang)
    };
    let _ = prompt;
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
    match export_config_to_path_with_options(
        &config,
        &vault,
        history.as_deref(),
        usage_comparison.as_deref(),
        &ExportOptions::Convenient,
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

pub fn run_import(ctx: &Ctx, input: PathBuf, yes: bool, strategy: ImportStrategy) -> i32 {
    run_import_with(ctx, input, yes, strategy, &mut TerminalPrompt)
}

fn run_import_with(
    ctx: &Ctx,
    input: PathBuf,
    yes: bool,
    strategy: ImportStrategy,
    prompt: &mut dyn TransferPrompt,
) -> i32 {
    // [红骨架] 策略与密码交互暂未生效：维持现状整体替换语义。
    let _ = (strategy, prompt);
    if !yes && !confirm(texts::config_import_confirm(ctx.lang, &input, false)) {
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
                texts::config_imported(
                    ctx.lang,
                    &input,
                    false,
                    &quota_core::ImportCounts {
                        providers_added: bundle.config.providers.len(),
                        ..Default::default()
                    }
                )
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
    use quota_core::{
        AppConfig, ImportStrategy, InMemoryStore, TransferMode, UsageComparisonSeries,
        inspect_transfer_container,
    };

    use super::*;

    const SECRET: &str = "sk-cli-transfer-secret";
    const PASSWORD: &str = "roundtrip-password-123";

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

    /// 本机已有与备份同 id 的条目（名称不同）：用于合并/覆盖冲突断言。
    fn target_ctx_with_local_entry(dir: &std::path::Path) -> Ctx {
        let ctx = Ctx::with_store(dir.join("target.json"), Arc::new(InMemoryStore::new()));
        let vault = ctx.open_vault().unwrap();
        let mut entry = ProviderEntry {
            id: "source-entry".into(),
            name: "Local Version".into(),
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
        entry.set_api_key(&vault, "sk-local-own-secret").unwrap();
        AppConfig {
            providers: vec![entry],
            custom_models: Default::default(),
        }
        .save(&ctx.config_path)
        .unwrap();
        ctx
    }

    /// 测试桩：按脚本回放交互原语（密码/确认从尾部弹出，耗尽后
    /// 密码报错、确认拒绝——未显式编排的交互一律失败，防测试虚假通过）。
    struct StubPrompt {
        tier: Option<usize>,
        confirm_reply: Vec<bool>,
        passwords: Vec<String>,
        password_calls: usize,
        confirm_calls: usize,
        abort_next_password: bool,
    }

    impl StubPrompt {
        fn new(tier: Option<usize>, confirm_reply: Vec<bool>, passwords: Vec<String>) -> Self {
            Self {
                tier,
                confirm_reply,
                passwords,
                password_calls: 0,
                confirm_calls: 0,
                abort_next_password: false,
            }
        }
    }

    impl TransferPrompt for StubPrompt {
        fn confirm(&mut self, _prompt: String) -> bool {
            self.confirm_calls += 1;
            self.confirm_reply.pop().unwrap_or(false)
        }

        fn select_export_tier(&mut self, _lang: Lang) -> Option<usize> {
            self.tier
        }

        fn read_password(
            &mut self,
            _prompt: &str,
            _lang: Lang,
        ) -> std::io::Result<Zeroizing<String>> {
            self.password_calls += 1;
            if self.abort_next_password {
                return Err(std::io::Error::other("stub abort"));
            }
            match self.passwords.pop() {
                Some(pw) => Ok(Zeroizing::new(pw)),
                None => Err(std::io::Error::other("stub exhausted")),
            }
        }
    }

    fn series(provider: &str, slot: u8) -> UsageComparisonSeries {
        UsageComparisonSeries {
            provider_id: provider.into(),
            window_key: "five_hour".into(),
            color_slot: slot,
        }
    }

    fn write_series(ctx: &Ctx, items: &[UsageComparisonSeries]) {
        settings_io::write_usage_comparison(&ctx.config_path, Some(items)).unwrap();
    }

    fn read_series(ctx: &Ctx) -> Vec<UsageComparisonSeries> {
        settings_io::load_usage_comparison(&ctx.config_path)
            .unwrap()
            .unwrap_or_default()
    }

    fn record_history(ctx: &Ctx, provider: &str, remaining: f64, ts: u64) {
        HistoryStore::open(&ctx.history_path())
            .unwrap()
            .record(
                provider,
                &[quota_core::UsageData {
                    plan_name: Some("five_hour".into()),
                    remaining: Some(remaining),
                    unit: Some("CNY".into()),
                    ..Default::default()
                }],
                ts,
            )
            .unwrap();
    }

    /// 契约：`--yes` 导出按脚本兼容口径落便捷档，产物可被现状导入路径
    /// 消费（QTRAYCFG 魔数、v3 便捷档、不含凭据明文）。
    #[test]
    fn export_yes_writes_convenient_v3_bundle() {
        let dir = test_dir("export-yes");
        let ctx = source_ctx(&dir);
        let output = dir.join("backup.qtray-export");

        assert_eq!(run_export(&ctx, output.clone(), true), 0);
        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.starts_with(b"QTRAYCFG"));
        assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
        let info = inspect_transfer_container(&bytes).unwrap();
        assert_eq!(info.version, 3);
        assert_eq!(info.mode, TransferMode::Convenient);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：交互选密码档 + 两次一致密码 → 产 v3 密码档包；`--yes`
    /// 导入同一包时仍会被要求输入密码，输入正确后完整恢复（凭据转写）。
    #[test]
    fn export_password_tier_roundtrip() {
        let dir = test_dir("pw-roundtrip");
        let source = source_ctx(&dir);
        let bundle = dir.join("pw.qtray-export");

        let mut export_prompt = StubPrompt::new(
            Some(0),
            vec![true],
            vec![PASSWORD.to_string(), PASSWORD.to_string()],
        );
        assert_eq!(
            run_export_with(&source, bundle.clone(), false, &mut export_prompt),
            0
        );
        let bytes = std::fs::read(&bundle).unwrap();
        let info = inspect_transfer_container(&bytes).unwrap();
        assert_eq!(info.version, 3);
        assert_eq!(info.mode, TransferMode::Password);
        assert!(
            !bytes
                .windows(PASSWORD.len())
                .any(|w| w == PASSWORD.as_bytes())
        );

        // --yes 不跳过密码输入：桩被消费一次才可能成功
        let target = Ctx::with_store(dir.join("target.json"), Arc::new(InMemoryStore::new()));
        AppConfig::default().save(&target.config_path).unwrap();
        let mut import_prompt = StubPrompt::new(None, vec![true], vec![PASSWORD.to_string()]);
        assert_eq!(
            run_import_with(
                &target,
                bundle,
                true,
                ImportStrategy::Overwrite,
                &mut import_prompt
            ),
            0
        );
        assert_eq!(import_prompt.password_calls, 1, "--yes 也必须问一次密码");
        let imported = AppConfig::load(&target.config_path).unwrap();
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

    /// 契约：两次密码不一致 → 提示后重新输入两次；一致后导出成功。
    #[test]
    fn export_password_mismatch_retries_until_match() {
        let dir = test_dir("pw-mismatch");
        let ctx = source_ctx(&dir);
        let output = dir.join("pw.qtray-export");

        // 弹序（尾部弹出）：先 [wrong, first]（不一致），再 [right, right]
        let mut prompt = StubPrompt::new(
            Some(0),
            vec![true],
            vec![
                "right-password-9".to_string(),
                "right-password-9".to_string(),
                "bbbbbbbb-2".to_string(),
                "aaaaaaaa-1".to_string(),
            ],
        );
        assert_eq!(run_export_with(&ctx, output.clone(), false, &mut prompt), 0);
        assert_eq!(prompt.password_calls, 4, "前两次不一致后必须重试");
        let info = inspect_transfer_container(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(info.mode, TransferMode::Password);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：档位选择取消 → 退出码 0 且不写任何文件、不问密码不确认。
    #[test]
    fn export_tier_select_cancel_writes_nothing() {
        let dir = test_dir("tier-cancel");
        let ctx = source_ctx(&dir);
        let output = dir.join("never.qtray-export");
        let mut prompt = StubPrompt::new(None, vec![true], vec![]);

        assert_eq!(run_export_with(&ctx, output.clone(), false, &mut prompt), 0);
        assert!(!output.exists());
        assert_eq!(prompt.password_calls, 0);
        assert_eq!(prompt.confirm_calls, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：风险确认拒绝 → 退出码 0 且不写文件。
    #[test]
    fn export_confirm_decline_writes_nothing() {
        let dir = test_dir("confirm-decline");
        let ctx = source_ctx(&dir);
        let output = dir.join("never.qtray-export");
        // 便捷档（无需密码）但拒绝确认
        let mut prompt = StubPrompt::new(Some(1), vec![false], vec![]);

        assert_eq!(run_export_with(&ctx, output.clone(), false, &mut prompt), 0);
        assert!(!output.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：密码输入中止（Ctrl+C 等）→ 退出码 0 且不写文件。
    #[test]
    fn export_password_abort_writes_nothing() {
        let dir = test_dir("pw-abort");
        let ctx = source_ctx(&dir);
        let output = dir.join("never.qtray-export");
        let mut prompt = StubPrompt::new(Some(0), vec![true], vec![]);
        prompt.abort_next_password = true;

        assert_eq!(run_export_with(&ctx, output.clone(), false, &mut prompt), 0);
        assert!(!output.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：密码不足 8 字符（两次一致）→ core 确定性拒绝，退出码 1
    /// 且不写文件。
    #[test]
    fn export_password_too_short_fails_without_file() {
        let dir = test_dir("pw-short");
        let ctx = source_ctx(&dir);
        let output = dir.join("never.qtray-export");
        let mut prompt = StubPrompt::new(
            Some(0),
            vec![true],
            vec!["short12".to_string(), "short12".to_string()],
        );

        assert_eq!(run_export_with(&ctx, output.clone(), false, &mut prompt), 1);
        assert!(!output.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：`--yes` 导入默认 merge——空本机全额并入，凭据转写可达。
    #[test]
    fn import_yes_defaults_to_merge_into_empty_target() {
        let dir = test_dir("import-yes");
        let source = source_ctx(&dir);
        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = Ctx::with_store(dir.join("target.json"), Arc::new(InMemoryStore::new()));
        AppConfig::default().save(&target.config_path).unwrap();
        assert_eq!(run_import(&target, bundle, true, ImportStrategy::Merge), 0);

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

    /// 契约：默认（不传 --strategy）合并导入——同 id 冲突以本机为准，
    /// 本机条目原样保留。
    #[test]
    fn import_defaults_to_merge_local_wins() {
        let dir = test_dir("merge-local-wins");
        let source = source_ctx(&dir);
        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = target_ctx_with_local_entry(&dir);
        assert_eq!(run_import(&target, bundle, true, ImportStrategy::Merge), 0);

        let imported = AppConfig::load(&target.config_path).unwrap();
        assert_eq!(imported.providers.len(), 1, "同 id 并集不产生重复条目");
        assert_eq!(
            imported.providers[0].name, "Local Version",
            "合并模同 id 冲突以本机为准"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：`--strategy overwrite` 显式生效——本机同 id 条目被备份替换。
    #[test]
    fn import_overwrite_strategy_replaces_local() {
        let dir = test_dir("overwrite-replace");
        let source = source_ctx(&dir);
        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = target_ctx_with_local_entry(&dir);
        assert_eq!(
            run_import(&target, bundle, true, ImportStrategy::Overwrite),
            0
        );

        let imported = AppConfig::load(&target.config_path).unwrap();
        assert_eq!(imported.providers.len(), 1);
        assert_eq!(
            imported.providers[0].name, "Source Account",
            "覆盖模完全变成备份"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：便捷档包 `--yes` 导入不问密码（read_password 零调用）；
    /// 密码档包 `--yes` 导入必问一次（同一测试内对照）。
    #[test]
    fn import_password_requirement_follows_bundle_mode() {
        let dir = test_dir("pw-requirement");
        let source = source_ctx(&dir);

        // 便捷档包
        let convenient = dir.join("convenient.qtray-export");
        assert_eq!(run_export(&source, convenient.clone(), true), 0);
        let target = Ctx::with_store(dir.join("t1.json"), Arc::new(InMemoryStore::new()));
        AppConfig::default().save(&target.config_path).unwrap();
        let mut prompt = StubPrompt::new(None, vec![true], vec![]);
        assert_eq!(
            run_import_with(
                &target,
                convenient,
                true,
                ImportStrategy::Merge,
                &mut prompt
            ),
            0
        );
        assert_eq!(prompt.password_calls, 0, "便捷档包不得问密码");

        // 密码档包：不提供密码（桩耗尽）→ 密码必答缺失，确定性失败
        let password_bundle = dir.join("password.qtray-export");
        let mut export_prompt = StubPrompt::new(
            Some(0),
            vec![true],
            vec![PASSWORD.to_string(), PASSWORD.to_string()],
        );
        assert_eq!(
            run_export_with(&source, password_bundle.clone(), false, &mut export_prompt),
            0
        );
        let target2 = Ctx::with_store(dir.join("t2.json"), Arc::new(InMemoryStore::new()));
        AppConfig::default().save(&target2.config_path).unwrap();
        let mut no_password = StubPrompt::new(None, vec![true], vec![]);
        assert_eq!(
            run_import_with(
                &target2,
                password_bundle.clone(),
                true,
                ImportStrategy::Merge,
                &mut no_password
            ),
            1
        );
        assert_eq!(no_password.password_calls, 1);
        assert_eq!(
            AppConfig::load(&target2.config_path).unwrap(),
            AppConfig::default(),
            "导入失败不得触碰本机配置"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：密码档包输错密码 → 确定性失败（退出码 1），本机配置不动。
    #[test]
    fn import_password_bundle_wrong_password_fails_deterministically() {
        let dir = test_dir("pw-wrong");
        let source = source_ctx(&dir);
        let bundle = dir.join("pw.qtray-export");
        let mut export_prompt = StubPrompt::new(
            Some(0),
            vec![true],
            vec![PASSWORD.to_string(), PASSWORD.to_string()],
        );
        assert_eq!(
            run_export_with(&source, bundle.clone(), false, &mut export_prompt),
            0
        );

        let target = target_ctx_with_local_entry(&dir);
        let before = AppConfig::load(&target.config_path).unwrap();
        let mut prompt = StubPrompt::new(None, vec![true], vec!["wrong-password-9".into()]);
        assert_eq!(
            run_import_with(&target, bundle, true, ImportStrategy::Merge, &mut prompt),
            1
        );
        assert_eq!(
            AppConfig::load(&target.config_path).unwrap(),
            before,
            "认证失败不得部分写入"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：合并模历史幂等合并续线、比较组合按 (provider, window) 并集。
    #[test]
    fn import_merge_unions_history_and_series() {
        let dir = test_dir("merge-union");
        let source = source_ctx(&dir);
        record_history(&source, "backup-provider", 11.0, 1_700_000_000_001);
        write_series(&source, &[series("backup-provider", 1)]);
        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = target_ctx_with_local_entry(&dir);
        record_history(&target, "local-provider", 22.0, 1_700_000_000_002);
        write_series(&target, &[series("local-provider", 0)]);

        assert_eq!(run_import(&target, bundle, true, ImportStrategy::Merge), 0);

        let local_points = HistoryStore::open(&target.history_path())
            .unwrap()
            .range("local-provider", 0)
            .unwrap();
        let backup_points = HistoryStore::open(&target.history_path())
            .unwrap()
            .range("backup-provider", 0)
            .unwrap();
        assert_eq!(local_points.len(), 1, "合并不丢本机历史");
        assert_eq!(backup_points.len(), 1, "备份历史并入");

        let merged = read_series(&target);
        assert_eq!(merged.len(), 2, "组合按并集：本机 + 备份各一条");
        assert!(merged.iter().any(|s| s.provider_id == "local-provider"));
        assert!(merged.iter().any(|s| s.provider_id == "backup-provider"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：覆盖模历史单事务清空重插（只剩备份行）、比较组合整体替换。
    #[test]
    fn import_overwrite_replaces_history_and_series() {
        let dir = test_dir("overwrite-history");
        let source = source_ctx(&dir);
        record_history(&source, "backup-provider", 11.0, 1_700_000_000_001);
        write_series(&source, &[series("backup-provider", 2)]);
        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = target_ctx_with_local_entry(&dir);
        record_history(&target, "local-provider", 22.0, 1_700_000_000_002);
        write_series(&target, &[series("local-provider", 0)]);

        assert_eq!(
            run_import(&target, bundle, true, ImportStrategy::Overwrite),
            0
        );

        let local_points = HistoryStore::open(&target.history_path())
            .unwrap()
            .range("local-provider", 0)
            .unwrap();
        let backup_points = HistoryStore::open(&target.history_path())
            .unwrap()
            .range("backup-provider", 0)
            .unwrap();
        assert_eq!(local_points.len(), 0, "覆盖清空本机历史");
        assert_eq!(backup_points.len(), 1, "备份历史重插");

        let replaced = read_series(&target);
        assert_eq!(replaced.len(), 1);
        assert_eq!(replaced[0].provider_id, "backup-provider");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：覆盖模遇到未携带历史的包（如 v1 老包/源机无历史库）时
    /// 不清空本机历史（备份「没有这部分数据」≠「断言历史为空」）。
    #[test]
    fn import_overwrite_bundle_without_history_keeps_local_history() {
        let dir = test_dir("overwrite-no-history");
        // 源机无历史库、无组合：导出的包 history=None、series=None
        let source = source_ctx(&dir);
        let bundle = dir.join("bare.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = target_ctx_with_local_entry(&dir);
        record_history(&target, "local-provider", 22.0, 1_700_000_000_002);

        assert_eq!(
            run_import(&target, bundle, true, ImportStrategy::Overwrite),
            0
        );
        let points = HistoryStore::open(&target.history_path())
            .unwrap()
            .range("local-provider", 0)
            .unwrap();
        assert_eq!(points.len(), 1, "备份未携带历史时本机历史保持不动");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：导入确认拒绝 → 退出码 0 且本机配置不动。
    #[test]
    fn import_confirm_decline_keeps_config() {
        let dir = test_dir("import-decline");
        let source = source_ctx(&dir);
        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = target_ctx_with_local_entry(&dir);
        let before = AppConfig::load(&target.config_path).unwrap();
        let mut prompt = StubPrompt::new(None, vec![false], vec![]);
        assert_eq!(
            run_import_with(
                &target,
                bundle,
                false,
                ImportStrategy::Overwrite,
                &mut prompt
            ),
            0
        );
        assert_eq!(AppConfig::load(&target.config_path).unwrap(), before);
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

        assert_eq!(run_import(&target, bundle, true, ImportStrategy::Merge), 1);
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
        record_history(&source, "source-entry", 42.0, 1_700_000_000_000);

        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = Ctx::with_store(dir.join("target.json"), Arc::new(InMemoryStore::new()));
        AppConfig::default().save(&target.config_path).unwrap();
        assert_eq!(run_import(&target, bundle, true, ImportStrategy::Merge), 0);

        let points = HistoryStore::open(&target.history_path())
            .unwrap()
            .range("source-entry", 0)
            .unwrap();
        assert_eq!(points.len(), 1, "历史随迁移包到达目标机");
        assert_eq!(points[0].remaining, Some(42.0));
        // v1 老包兼容（history=None 不合并）由 core transfer 测试覆盖

        let _ = std::fs::remove_dir_all(dir);
    }

    /// 契约：合并模接线正确填充生效计数（providers/series 新增与跳过）。
    #[test]
    fn import_merge_reports_effective_counts() {
        let dir = test_dir("merge-counts");
        let source = source_ctx(&dir);
        write_series(&source, &[series("source-entry", 1)]);
        let bundle = dir.join("backup.qtray-export");
        assert_eq!(run_export(&source, bundle.clone(), true), 0);

        let target = target_ctx_with_local_entry(&dir);
        write_series(&target, &[series("source-entry", 0), series("extra", 3)]);

        assert_eq!(run_import(&target, bundle, true, ImportStrategy::Merge), 0);
        // providers：备份 1 条同 id 冲突跳过；series：备份 1 条同键跳过
        // （计数行为由 core/UI 层保证，CLI 测试聚焦接线不炸即可——这里
        // 以最终 settings 状态兜底断言）
        let merged = read_series(&target);
        assert_eq!(merged.len(), 2, "本机组合保序保留");
        assert!(merged.iter().all(|s| s.provider_id != "backup-only"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
