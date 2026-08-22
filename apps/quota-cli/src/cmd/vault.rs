//! `quota vault status`：主密钥健康检查（系统凭据库可读性）。

use quota_core::{KeyringStore, SecretStore, Vault};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx) -> i32 {
    let store = KeyringStore::new();

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

    match Vault::open(&store) {
        Ok(_) => println!("保险库：健康（加解密就绪）"),
        Err(e) => {
            eprintln!("保险库打开失败：{e}");
            return 1;
        }
    }
    println!("配置文件：{}", ctx.config_path.display());
    0
}
