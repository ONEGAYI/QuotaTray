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
