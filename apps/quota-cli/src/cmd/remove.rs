//! `quota remove`：删除条目（确认提示，`--yes` 跳过）。

use dialoguer::{Confirm, theme::ColorfulTheme};
use quota_core::AppConfig;

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, id: String, yes: bool) -> i32 {
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("错误：{e}");
            return 1;
        }
    };
    let Some(pos) = cfg.providers.iter().position(|e| e.id == id) else {
        eprintln!("错误：找不到条目 {id}");
        return 1;
    };
    let entry = cfg.providers[pos].clone();

    if !yes {
        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("删除 {}（{id}）？其凭据密文将一并移除", entry.name))
            .default(false)
            .interact()
            .unwrap_or(false);
        if !confirmed {
            println!("已取消。");
            return 0;
        }
    }

    cfg.providers.remove(pos);
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("错误：{e}");
        return 1;
    }
    println!("已删除：{}（{id}）", entry.name);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::Ctx;
    use quota_core::InMemoryStore;
    use quota_core::config::{ProviderEntry, ProviderKind};
    use std::sync::Arc;

    fn test_ctx(tag: &str) -> Ctx {
        let dir = std::env::temp_dir().join(format!("quota-cli-rm-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let _ = std::fs::remove_file(&path);
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
            api_key_enc: None,
            base_url: None,
        }
    }

    /// 契约：--yes 删除落盘——配置文件中该条目消失。
    #[test]
    fn remove_yes_persists() {
        let ctx = test_ctx("yes");
        let cfg = AppConfig {
            providers: vec![native("e1"), native("e2")],
        };
        cfg.save(&ctx.config_path).unwrap();

        assert_eq!(run(&ctx, "e1".into(), true), 0);
        let after = AppConfig::load(&ctx.config_path).unwrap();
        assert_eq!(after.providers.len(), 1);
        assert_eq!(after.providers[0].id, "e2");
        let _ = std::fs::remove_dir_all(ctx.config_path.parent().unwrap());
    }

    /// 契约：删除不存在的 id → 退出 1。
    #[test]
    fn remove_missing_fails() {
        let ctx = test_ctx("missing");
        assert_eq!(run(&ctx, "zzz".into(), true), 1);
        let _ = std::fs::remove_dir_all(ctx.config_path.parent().unwrap());
    }
}
