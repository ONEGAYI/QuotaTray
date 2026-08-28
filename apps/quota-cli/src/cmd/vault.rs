//! `quota vault status`：主密钥健康检查（安装态查系统凭据库、便携态查
//! 密钥文件的可读性）。
//!
//! 副作用：主密钥不存在时，健康检查内部的 `Vault::open` 会顺带完成
//! 首次初始化（生成并写入，幂等）——安装态与正常首次运行行为一致；
//! 便携态正常路径已在启动门控完成初始化（此处的 NotInit 是文件被
//! 删除后的兜底展示）。

use quota_core::Vault;

use crate::ctx::Ctx;
use crate::texts::{T, t};

pub fn run(ctx: &Ctx) -> i32 {
    let lang = ctx.lang;
    let store = ctx.store();

    // 便携态文案与后端一致（FileStore），避免误导用户去查凭据管理器
    let (ok, not_init, read_fail, hint) = if ctx.is_portable() {
        (
            T::VaultStoreOkPortable,
            T::VaultStoreNotInitPortable,
            T::VaultStoreReadFailPortable,
            T::VaultStoreHintPortable,
        )
    } else {
        (
            T::VaultStoreOk,
            T::VaultStoreNotInit,
            T::VaultStoreReadFail,
            T::VaultStoreHint,
        )
    };

    match store.get() {
        Ok(Some(_)) => println!("{}", t(lang, ok)),
        Ok(None) if ctx.is_portable() => {
            // 便携态密钥缺失：初始化必须走首启门控（固定安全提示 +
            // 显式确认），status 作为健康检查不做静默重建——与
            // FileStore 对损坏密钥「不自愈换钥」的哲学一致
            println!("{}", t(lang, not_init));
            eprintln!("{}", t(lang, T::PortableKeyMissingHint));
            return 1;
        }
        Ok(None) => println!("{}", t(lang, not_init)),
        Err(e) => {
            eprintln!("{}{e}", t(lang, read_fail));
            eprintln!("{}", t(lang, hint));
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

    /// 契约：便携上下文（FileStore 已初始化）status 全绿——退出码与
    /// 安装态一致，文案由 is_portable 分流（输出内容不在此断言）。
    #[test]
    fn status_with_portable_ctx_is_healthy() {
        let root =
            std::env::temp_dir().join(format!("quota-cli-vault-port-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = quota_core::FileStore::new(quota_core::portable_key_path(&root));
        quota_core::Vault::open(&store).unwrap();
        let ctx = Ctx::portable(root.clone(), Lang::Zh);
        assert_eq!(run(&ctx), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 契约：便携态密钥缺失（启动门控后被删）时 status 不静默重建——
    /// 报「尚未初始化」+ 初始化指引并以非零退出，密钥文件保持缺失。
    #[test]
    fn status_portable_missing_key_does_not_recreate() {
        let root =
            std::env::temp_dir().join(format!("quota-cli-vault-miss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = quota_core::FileStore::new(quota_core::portable_key_path(&root));
        quota_core::Vault::open(&store).unwrap();
        std::fs::remove_file(quota_core::portable_key_path(&root)).unwrap();
        let ctx = Ctx::portable(root.clone(), Lang::Zh);
        assert_eq!(run(&ctx), 1, "密钥缺失 = 确定性失败而非静默重建");
        assert!(
            !quota_core::portable_key_path(&root).exists(),
            "status 不得重建密钥"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
