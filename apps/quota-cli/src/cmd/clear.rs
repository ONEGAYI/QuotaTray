//! `quota clear`：清空全部用户数据——供应商条目（含凭据密文）、峰谷
//! 定价、自定义模型库与查询历史；应用偏好（settings.json）与主密钥
//! 保留。确认提示默认否，`--yes` 跳过；非交互会话未显式 `--yes` 时
//! 确定性拒绝（破坏性操作不允许静默放行，与便携首启同口径）。

use dialoguer::{Confirm, theme::ColorfulTheme};
use quota_core::AppConfig;
use std::io::IsTerminal;

use crate::ctx::Ctx;
use crate::texts::{self, T, t};

/// 非交互门控（纯函数可测）：破坏性操作在非交互会话必须显式 `--yes`。
/// 抽纯函数是因为测试进程的 stdin 是否 tty 取决于运行环境（本地交互
/// 终端跑 cargo test 时是 tty），集成测试无法稳定覆盖拒绝分支。
fn non_tty_refused(yes: bool, stdin_is_tty: bool) -> bool {
    !yes && !stdin_is_tty
}

pub fn run(ctx: &Ctx, yes: bool) -> i32 {
    let lang = ctx.lang;
    // 终端判定先于确认：脚本/管道场景 dialoguer 无法交互，直接给出口径
    if non_tty_refused(yes, std::io::stdin().is_terminal()) {
        eprintln!("{}{}", t(lang, T::Err), t(lang, T::ClearNonTty));
        return 1;
    }
    if !yes {
        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(t(lang, T::ClearConfirm))
            .default(false)
            .interact()
            .unwrap_or(false);
        if !confirmed {
            println!("{}", texts::cancelled(lang));
            return 0;
        }
    }
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    cfg.clear_user_data();
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    // GUI 的余额快照缓存（cache.json）与配置同数据根：一并删除，防
    // GUI 下次启动把已删条目的快照恢复进结果表（清空后无查询触发
    // 快照过滤，残留不会自愈）；不存在时删除静默跳过
    let _ = std::fs::remove_file(ctx.config_path.with_file_name("cache.json"));
    // 历史是用户显式要求删除的一部分：失败如实报错退出（与桌面端
    // clear_all_data 同口径；区别于单条目删除的孤儿数据告警降级）；
    // 此时配置已清，重跑本命令幂等可补清历史
    if let Err(e) = quota_core::HistoryStore::open(&ctx.history_path()).and_then(|s| s.clear(None))
    {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    println!("{}", t(lang, T::ClearDone));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::Ctx;
    use quota_core::InMemoryStore;
    use quota_core::PlanVariant;
    use quota_core::config::{ProviderEntry, ProviderKind};
    use std::sync::Arc;

    fn test_ctx(tag: &str) -> Ctx {
        let dir =
            std::env::temp_dir().join(format!("quota-cli-clear-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(dir.join("history.db"));
        Ctx::with_store(path, Arc::new(InMemoryStore::new()))
    }

    fn native(id: &str) -> ProviderEntry {
        ProviderEntry {
            id: id.into(),
            name: format!("n-{id}"),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: Some("v1:密文占位".into()),
            api_key2_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        }
    }

    /// 契约：--yes 清空落盘——条目与密文字段消失、历史全清；幂等。
    /// （cfg.save 后 custom_models 空库不落盘，用 load 回读断言）
    #[test]
    fn clear_yes_wipes_config_and_history() {
        let ctx = test_ctx("yes");
        AppConfig {
            providers: vec![native("e1")],
            custom_models: Default::default(),
        }
        .save(&ctx.config_path)
        .unwrap();
        quota_core::HistoryStore::open(&ctx.history_path())
            .unwrap()
            .record("e1", &[quota_core::UsageData::default()], 1_700_000_000_000)
            .unwrap();

        assert_eq!(run(&ctx, true), 0);
        let back = AppConfig::load(&ctx.config_path).unwrap();
        assert_eq!(back, AppConfig::default(), "配置应回到出厂空态");
        let rows = quota_core::HistoryStore::open(&ctx.history_path())
            .unwrap()
            .range("e1", 0)
            .unwrap();
        assert!(rows.is_empty(), "历史应全清");
        // 幂等：空态再清仍成功
        assert_eq!(run(&ctx, true), 0);
    }

    /// 契约：--yes 顺带删除 GUI 快照缓存（cache.json），防 GUI 重启
    /// 恢复已删条目的旧快照。
    #[test]
    fn clear_yes_removes_snapshot_cache() {
        let ctx = test_ctx("snapshot");
        AppConfig {
            providers: vec![native("e1")],
            custom_models: Default::default(),
        }
        .save(&ctx.config_path)
        .unwrap();
        let snapshot = ctx.config_path.with_file_name("cache.json");
        std::fs::write(&snapshot, r#"{"entries":{"e1":{}}}"#).unwrap();
        assert_eq!(run(&ctx, true), 0);
        assert!(!snapshot.exists(), "快照缓存应被删除");
    }

    /// 契约：非交互门控纯函数——非 tty 无 --yes 必拒；--yes 放行；
    /// tty 走确认分支（集成行为的 stdin 是否 tty 取决于测试运行环境，
    /// 无法稳定覆盖拒绝分支，见 non_tty_refused 注释）。
    #[test]
    fn non_tty_gate() {
        assert!(non_tty_refused(false, false), "非 tty 无 yes 必拒");
        assert!(!non_tty_refused(true, false), "--yes 放行");
        assert!(!non_tty_refused(false, true), "tty 走确认分支");
    }
}
