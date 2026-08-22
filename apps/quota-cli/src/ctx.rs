//! 命令执行上下文：配置路径 + 凭据库后端。
//!
//! [`Ctx`] 刻意保持轻薄：vault 与引擎不缓存——`open_vault` 幂等
//! （同一 store 读出同一主密钥，每次调用等价），引擎每进程只建一次
//! （调用方持有 owned 后传引用）。`natives`/`list` 等纯本地命令
//! 不触碰任何后端。

use std::path::PathBuf;
use std::sync::Arc;

use quota_core::{KeyringStore, QueryEngine, SecretStore, Vault};

pub struct Ctx {
    pub config_path: PathBuf,
    store: Arc<dyn SecretStore>,
}

impl Ctx {
    /// 生产上下文：主密钥后端为系统凭据库（keyring）。
    pub fn production(config_path: PathBuf) -> Self {
        Self {
            config_path,
            store: Arc::new(KeyringStore::new()),
        }
    }

    /// 测试上下文：注入内存后端，不触碰真实系统凭据库。
    #[cfg(test)]
    pub fn with_store(config_path: PathBuf, store: Arc<dyn SecretStore>) -> Self {
        Self { config_path, store }
    }

    /// 打开凭据保险库（幂等：同一 store 恒得同一主密钥，可重复调用）。
    pub fn open_vault(&self) -> Result<Vault, String> {
        Vault::open(self.store.as_ref()).map_err(|e| format!("凭据保险库打开失败：{e}"))
    }

    /// 构造生产查询引擎（reqwest + 15 秒超时）。每进程调用一次即可。
    pub fn new_engine(&self) -> Result<QueryEngine, String> {
        QueryEngine::with_default_client().map_err(|e| format!("查询引擎初始化失败：{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：open_vault 幂等——两次打开的保险库互通（密文可互解）。
    #[test]
    fn open_vault_is_idempotent() {
        let ctx = Ctx::with_store(
            PathBuf::from("unused.json"),
            Arc::new(quota_core::InMemoryStore::new()),
        );
        let v1 = ctx.open_vault().unwrap();
        let v2 = ctx.open_vault().unwrap();
        let ct = v1.encrypt("secret", "p").unwrap();
        assert_eq!(v2.decrypt(&ct, "p").unwrap(), "secret");
    }
}
