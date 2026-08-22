//! `quota vault status`：主密钥健康检查（系统凭据库可读性）。

use quota_core::Vault;

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx) -> i32 {
    let store = ctx.store();

    match store.get() {
        Ok(Some(_)) => println!("系统凭据库：可读（主密钥已存在）"),
        Ok(None) => println!("系统凭据库：可读（主密钥尚未初始化，将在首次加密时生成）"),
        Err(e) => {
            eprintln!("系统凭据库读取失败：{e}");
            eprintln!(
                "（Windows 请检查凭据管理器可用性；Linux 需要 Secret Service / gnome-keyring）"
            );
            return 1;
        }
    }

    match Vault::open(store) {
        Ok(_) => println!("保险库：健康（加解密就绪）"),
        Err(e) => {
            eprintln!("保险库打开失败：{e}");
            return 1;
        }
    }
    println!("配置文件：{}", ctx.config_path.display());
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::Ctx;
    use quota_core::InMemoryStore;
    use std::sync::Arc;

    /// 契约：注入内存后端时 status 全绿（可读 → open 成功）。
    #[test]
    fn status_with_injected_store_is_healthy() {
        let dir = std::env::temp_dir().join(format!("quota-cli-vault-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = Ctx::with_store(dir.join("config.json"), Arc::new(InMemoryStore::new()));
        assert_eq!(run(&ctx), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
