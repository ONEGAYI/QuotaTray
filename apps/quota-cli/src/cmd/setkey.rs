//! `quota set-key`：隐藏输入读取 key，经 vault 加密写入配置。
//!
//! 不接受命令行参数形式的 key（避免进入 shell history）；
//! 管道 stdin 允许（`echo $KEY | quota set-key id`）。

use quota_core::AppConfig;

use crate::ctx::Ctx;
use crate::io;

pub fn run(ctx: &Ctx, id: String) -> i32 {
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("错误：{e}");
            return 1;
        }
    };
    let Some(entry) = cfg.providers.iter_mut().find(|e| e.id == id) else {
        eprintln!("错误：找不到条目 {id}");
        return 1;
    };

    let key = match io::read_secret("输入新的 API key（输入不回显）") {
        Ok(k) => k,
        Err(e) => {
            eprintln!("错误：key 读取失败：{e}");
            return 1;
        }
    };
    if key.trim().is_empty() {
        eprintln!("错误：输入为空，key 未变更（如需删除条目请用 quota remove）");
        return 1;
    }

    let vault = match ctx.open_vault() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("错误：{e}");
            return 1;
        }
    };
    let name = if let Err(e) = entry.set_api_key(&vault, key.trim()) {
        eprintln!("错误：凭据加密失败：{e}");
        return 1;
    } else {
        entry.name.clone()
    };
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("错误：{e}");
        return 1;
    }
    println!("已更新 key：{name}（{id}）");
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::Ctx;
    use quota_core::InMemoryStore;
    use std::sync::Arc;

    /// 契约：set-key 不存在的 id → 退出 1（在读取 stdin 之前拦截，测试无需喂 key）。
    #[test]
    fn setkey_missing_id_fails_before_input() {
        let dir = std::env::temp_dir().join(format!("quota-cli-sk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = Ctx::with_store(dir.join("config.json"), Arc::new(InMemoryStore::new()));
        assert_eq!(run(&ctx, "zzz".into()), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
