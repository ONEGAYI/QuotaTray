//! 主密钥的存储后端抽象。
//!
//! 生产环境用 [`KeyringStore`]（Windows Credential Manager /
//! macOS Keychain / Linux Secret Service，经 keyring crate）；
//! 单元测试与无凭据库环境用 [`InMemoryStore`]。

use std::sync::{Arc, RwLock};

use base64::{Engine, engine::general_purpose::STANDARD as B64};

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("系统凭据库访问失败：{0}")]
    Store(String),
    #[error("系统凭据库中存有异常的主密钥：{0}")]
    CorruptedMasterKey(String),
    #[error(transparent)]
    Cipher(#[from] super::cipher::CipherError),
    #[error("密文损坏：{reason}")]
    InvalidCiphertext { reason: String },
}

/// 主密钥存储后端。
pub trait SecretStore: Send + Sync {
    /// 读取主密钥；`Ok(None)` 表示尚未初始化（调用方将生成并 [`set`](Self::set)）。
    fn get(&self) -> Result<Option<Vec<u8>>, VaultError>;
    /// 写入主密钥（仅初始化时调用一次）。
    fn set(&self, key: &[u8]) -> Result<(), VaultError>;
}

/// 系统凭据库后端（keyring crate）。
///
/// 条目：service `QuotaTray` / user `master-key`，内容为 base64 的主密钥。
///
/// 注：无自动化测试——真实系统凭据库无法在 CI 中可靠 mock，
/// 行为验证依赖手动跑（Windows 凭据管理器可见 QuotaTray 条目）。
pub struct KeyringStore {
    service: &'static str,
    user: &'static str,
}

impl KeyringStore {
    pub fn new() -> Self {
        Self {
            service: "QuotaTray",
            user: "master-key",
        }
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringStore {
    fn get(&self) -> Result<Option<Vec<u8>>, VaultError> {
        let entry = keyring::Entry::new(self.service, self.user)
            .map_err(|e| VaultError::Store(e.to_string()))?;
        match entry.get_password() {
            Ok(encoded) => {
                let raw = B64
                    .decode(encoded)
                    .map_err(|e| VaultError::CorruptedMasterKey(e.to_string()))?;
                Ok(Some(raw))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(VaultError::Store(e.to_string())),
        }
    }

    fn set(&self, key: &[u8]) -> Result<(), VaultError> {
        let entry = keyring::Entry::new(self.service, self.user)
            .map_err(|e| VaultError::Store(e.to_string()))?;
        entry
            .set_password(&B64.encode(key))
            .map_err(|e| VaultError::Store(e.to_string()))
    }
}

/// 单测与 CI 用的内存后端。跨实例共享请克隆（内部 Arc）。
///
/// 仅用于测试：不落盘、随进程消失，生产路径误用会导致重启后密文不可解。
/// Debug 输出固定字面量——内部持有主密钥字节，不得进任何日志。
#[derive(Clone, Default)]
pub struct InMemoryStore {
    inner: Arc<RwLock<Option<Vec<u8>>>>,
}

impl std::fmt::Debug for InMemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InMemoryStore")
    }
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for InMemoryStore {
    fn get(&self) -> Result<Option<Vec<u8>>, VaultError> {
        Ok(self.inner.read().expect("in-memory store lock").clone())
    }

    fn set(&self, key: &[u8]) -> Result<(), VaultError> {
        *self.inner.write().expect("in-memory store lock") = Some(key.to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内存后端跨克隆共享状态（模拟"同一台机器"）。
    #[test]
    fn in_memory_store_shares_state_across_clones() {
        let a = InMemoryStore::new();
        let b = a.clone();
        assert_eq!(a.get().unwrap(), None);
        b.set(b"0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(
            a.get().unwrap(),
            Some(b"0123456789abcdef0123456789abcdef".to_vec())
        );
    }
}
