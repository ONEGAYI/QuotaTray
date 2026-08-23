//! `quota vault status`：主密钥健康检查（系统凭据库可读性）。
//!
//! 副作用：主密钥不存在时，健康检查内部的 `Vault::open` 会顺带完成
//! 首次初始化（生成并写入，幂等）——与正常首次运行行为一致。

use quota_core::Vault;

use crate::ctx::Ctx;
use crate::texts::{T, t};

pub fn run(ctx: &Ctx) -> i32 {
    let lang = ctx.lang;
    let store = ctx.store();

    match store.get() {
        Ok(Some(_)) => println!("{}", t(lang, T::VaultStoreOk)),
        Ok(None) => println!("{}", t(lang, T::VaultStoreNotInit)),
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::VaultStoreReadFail));
            eprintln!("{}", t(lang, T::VaultStoreHint));
            return 1;
        }
    }

    match Vault::open(store) {
        Ok(_) => println!("{}", t(lang, T::VaultHealthy)),
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::VaultOpenFail));
            return 1;
        }
    }
    println!(
        "{}{}",
        t(lang, T::ConfigFilePrefix),
        ctx.config_path.display()
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::Ctx;
    use crate::lang::Lang;
    use quota_core::InMemoryStore;
    use std::sync::Arc;

    /// 契约：注入内存后端时 status 全绿（可读 → open 成功），双语退出码一致。
    #[test]
    fn status_with_injected_store_is_healthy() {
        let dir = std::env::temp_dir().join(format!("quota-cli-vault-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for lang in [Lang::Zh, Lang::En] {
            let mut ctx = Ctx::with_store(dir.join("config.json"), Arc::new(InMemoryStore::new()));
            ctx.lang = lang;
            assert_eq!(run(&ctx), 0);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
