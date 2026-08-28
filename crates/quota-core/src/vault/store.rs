//! 主密钥的存储后端抽象。
//!
//! 生产环境用 [`KeyringStore`]（Windows Credential Manager /
//! macOS Keychain / Linux Secret Service / Android Keystore，经 keyring-core
//! 与平台原生 Store）；
//! 便携版用 [`FileStore`]（方案 A：`Data/portable.key` 包内密钥）；
//! 单元测试与无凭据库环境用 [`InMemoryStore`]。

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use base64::{Engine, engine::general_purpose::STANDARD as B64};

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("凭据存储访问失败：{0}")]
    Store(String),
    #[error("凭据存储中的主密钥异常：{0}")]
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

/// 原生凭据库类型。用于把目标平台与具体 Store 的选择固定为可测试契约。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStoreKind {
    Windows,
    SecretService,
    AppleKeychain,
    AppleProtected,
    AndroidKeystore,
    Unsupported,
}

pub fn native_store_kind(target_os: &str) -> NativeStoreKind {
    match target_os {
        "windows" => NativeStoreKind::Windows,
        "linux" => NativeStoreKind::SecretService,
        "macos" => NativeStoreKind::AppleKeychain,
        "ios" => NativeStoreKind::AppleProtected,
        "android" => NativeStoreKind::AndroidKeystore,
        _ => NativeStoreKind::Unsupported,
    }
}

static NATIVE_STORE_INIT: OnceLock<Result<(), String>> = OnceLock::new();

fn ensure_native_store() -> Result<(), VaultError> {
    let kind = native_store_kind(std::env::consts::OS);
    NATIVE_STORE_INIT
        .get_or_init(install_native_store)
        .clone()
        .map_err(|error| VaultError::Store(format!("{kind:?}：{error}")))
}

#[cfg(target_os = "windows")]
fn install_native_store() -> Result<(), String> {
    let store = windows_native_keyring_store::Store::new().map_err(|e| e.to_string())?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_native_store() -> Result<(), String> {
    let store = zbus_secret_service_keyring_store::Store::new().map_err(|e| e.to_string())?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_native_store() -> Result<(), String> {
    let store = apple_native_keyring_store::keychain::Store::new().map_err(|e| e.to_string())?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(target_os = "ios")]
fn install_native_store() -> Result<(), String> {
    let store = apple_native_keyring_store::protected::Store::new().map_err(|e| e.to_string())?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(target_os = "android")]
fn install_native_store() -> Result<(), String> {
    let store = android_native_keyring_store::Store::new().map_err(|e| e.to_string())?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "android"
)))]
fn install_native_store() -> Result<(), String> {
    Err(format!(
        "当前平台没有原生凭据库后端：{:?}",
        native_store_kind(std::env::consts::OS)
    ))
}

/// 系统凭据库后端（keyring-core + 平台原生 Store）。
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
        ensure_native_store()?;
        let entry = keyring_core::Entry::new(self.service, self.user)
            .map_err(|e| VaultError::Store(e.to_string()))?;
        match entry.get_password() {
            Ok(encoded) => {
                let raw = B64
                    .decode(encoded)
                    .map_err(|e| VaultError::CorruptedMasterKey(e.to_string()))?;
                Ok(Some(raw))
            }
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(VaultError::Store(e.to_string())),
        }
    }

    fn set(&self, key: &[u8]) -> Result<(), VaultError> {
        ensure_native_store()?;
        let entry = keyring_core::Entry::new(self.service, self.user)
            .map_err(|e| VaultError::Store(e.to_string()))?;
        entry
            .set_password(&B64.encode(key))
            .map_err(|e| VaultError::Store(e.to_string()))
    }
}

#[cfg(test)]
mod native_store_contract_tests {
    use super::*;

    #[test]
    fn native_store_kind_covers_desktop_and_android_targets() {
        assert_eq!(native_store_kind("windows"), NativeStoreKind::Windows);
        assert_eq!(native_store_kind("linux"), NativeStoreKind::SecretService);
        assert_eq!(native_store_kind("macos"), NativeStoreKind::AppleKeychain);
        assert_eq!(native_store_kind("ios"), NativeStoreKind::AppleProtected);
        assert_eq!(
            native_store_kind("android"),
            NativeStoreKind::AndroidKeystore
        );
        assert_eq!(native_store_kind("freebsd"), NativeStoreKind::Unsupported);
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

/// 文件主密钥后端（便携版方案 A）：裸 32 字节二进制存于数据根内
/// （`Data/portable.key`）。
///
/// 安全边界：密钥与配置密文同目录，保密等级等同明文凭据；FAT/exFAT
/// 等介质的文件权限不作为安全承诺，仅以 `create_new` 原子创建防两
/// 实例首启互相覆盖（写入后回读校验见 `Vault::open` 的既有竞态防护）。
///
/// **红线护栏（AGENTS.md 安全红线 §5）**：本后端首次创建密钥前，调用端
/// 必须原样展示「Portable 固定安全提示」并取得用户显式确认——未确认
/// 不得构造本后端去触发 `Vault::open`（首启建钥流程属端侧职责）。
pub struct FileStore {
    path: PathBuf,
}

impl std::fmt::Debug for FileStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 路径可展示（不含密钥材料）；防未来加字段时无意泄密
        f.debug_struct("FileStore")
            .field("path", &self.path)
            .finish()
    }
}

impl FileStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl SecretStore for FileStore {
    fn get(&self) -> Result<Option<Vec<u8>>, VaultError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                if bytes.len() == super::cipher::KEY_LEN {
                    Ok(Some(bytes))
                } else {
                    // 0 字节可能是初始化被掉电/并发中断（create_new 与
                    // write 之间的窗口），也可能是密钥文件被误清空。
                    // 有意不自愈删除重建：静默换钥会让既有密文（若有）
                    // 永久不可解，交给用户按文案处置更安全
                    Err(VaultError::CorruptedMasterKey(format!(
                        "密钥文件长度应为 32 字节，实际 {} 字节：{}（若为 0 字节且首次运\
                        行，可能是初始化被中断或另一实例正在初始化；确认无既有配置密\
                        文后，删除该文件并重启可重新初始化）",
                        bytes.len(),
                        self.path.display()
                    )))
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(VaultError::Store(format!(
                "读取密钥文件失败：{e}（{}）",
                self.path.display()
            ))),
        }
    }

    fn set(&self, key: &[u8]) -> Result<(), VaultError> {
        if key.len() != super::cipher::KEY_LEN {
            return Err(VaultError::CorruptedMasterKey(format!(
                "主密钥长度应为 32 字节，实际 {} 字节",
                key.len()
            )));
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                VaultError::Store(format!("创建数据目录失败：{e}（{}）", self.path.display()))
            })?;
        }
        // create_new：文件已存在即失败——两实例同时首启时后者不覆盖前者
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
            .map_err(|e| {
                VaultError::Store(format!(
                    "创建密钥文件失败：{e}（{}；若已存在，可能是另一实例正在初始化）",
                    self.path.display()
                ))
            })?;
        file.write_all(key).map_err(|e| {
            VaultError::Store(format!("写入密钥文件失败：{e}（{}）", self.path.display()))
        })?;
        // 刷新文件内容到磁盘（U 盘等可移除介质）；目录项的持久性仍受
        // FAT/exFAT 元数据惰性回写限制——这是 std 层能做到的最优近似
        file.sync_all().map_err(|e| {
            VaultError::Store(format!(
                "刷新密钥文件到磁盘失败：{e}（{}）",
                self.path.display()
            ))
        })?;
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

    /// 安全契约：Debug 输出不携带主密钥字节。
    #[test]
    fn debug_never_exposes_master_key() {
        let store = InMemoryStore::new();
        store.set(b"super-secret-key-bytes-32-bytes!!!").unwrap();
        let dbg = format!("{store:?}");
        assert_eq!(dbg, "InMemoryStore", "Debug 不得输出内部状态：{dbg}");
    }

    /// FileStore 测试沙箱（Windows 上句柄存活时删不掉，drop 后清理由
    /// 各用例自行处理）。
    fn file_store_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qt-filestore-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 契约：文件后端持久化——跨实例（"重新插入 U 盘"）读到同一密钥；
    /// 未初始化时 None。
    #[test]
    fn file_store_persists_across_instances() {
        let dir = file_store_dir("persist");
        let key_path = dir.join("Data").join("portable.key");
        let first = FileStore::new(key_path.clone());
        assert_eq!(first.get().unwrap(), None, "密钥未创建前为 None");
        first.set(b"0123456789abcdef0123456789abcdef").unwrap();
        // 新实例 = 新进程语义：仅凭文件恢复
        let second = FileStore::new(key_path.clone());
        assert_eq!(
            second.get().unwrap(),
            Some(b"0123456789abcdef0123456789abcdef".to_vec())
        );
        assert!(key_path.is_file(), "密钥真实落盘（父目录自动创建）");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：密钥文件长度异常 → CorruptedMasterKey（确定性失败，
    /// 不得静默当作未初始化重新生成——那会让既有密文永久不可解）。
    #[test]
    fn file_store_rejects_bad_length() {
        let dir = file_store_dir("corrupt");
        let key_path = dir.join("portable.key");
        std::fs::write(&key_path, vec![0u8; 31]).unwrap();
        let store = FileStore::new(key_path.clone());
        let err = store.get().unwrap_err();
        assert!(
            matches!(err, VaultError::CorruptedMasterKey(_)),
            "长度异常应为 CorruptedMasterKey：{err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：0 字节密钥文件（初始化被中断/被误清空的高发混淆）同样
    /// 报 CorruptedMasterKey，且文案带重新初始化的处置指引。
    #[test]
    fn file_store_empty_file_is_corrupted_with_recovery_hint() {
        let dir = file_store_dir("empty");
        let key_path = dir.join("portable.key");
        std::fs::write(&key_path, b"").unwrap();
        let store = FileStore::new(key_path);
        let err = store.get().unwrap_err();
        assert!(
            matches!(err, VaultError::CorruptedMasterKey(ref m)
                if m.contains("重新初始化")),
            "空文件应给出处置指引：{err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：密钥路径是目录（fs::read 会失败）→ Store 确定性错误，
    /// 不得误判为未初始化（否则会静默在别处重建密钥）。
    #[test]
    fn file_store_key_path_is_directory_maps_to_store_error() {
        let dir = file_store_dir("isdir");
        let key_path = dir.join("portable.key");
        std::fs::create_dir(&key_path).unwrap();
        let store = FileStore::new(key_path);
        let err = store.get().unwrap_err();
        assert!(
            matches!(err, VaultError::Store(_)),
            "目录路径应为 Store 错误而非 None/panic：{err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：set 方向同样校验长度——非 32 字节的密钥被拒且不落盘。
    #[test]
    fn file_store_set_rejects_bad_length() {
        let dir = file_store_dir("setlen");
        let key_path = dir.join("portable.key");
        let store = FileStore::new(key_path.clone());
        let err = store.set(&[0u8; 31]).unwrap_err();
        assert!(
            matches!(err, VaultError::CorruptedMasterKey(_)),
            "set 长度校验：{err}"
        );
        assert!(!key_path.exists(), "拒绝时不落盘");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：FileStore 是哑容器——仅长度校验，全零 32 字节照样放行
    /// （是否拒绝弱密钥是生成侧的职责，恢复侧不做内容审查）。
    #[test]
    fn file_store_accepts_thirty_two_zero_bytes() {
        let dir = file_store_dir("zeros");
        let key_path = dir.join("portable.key");
        std::fs::write(&key_path, vec![0u8; 32]).unwrap();
        let store = FileStore::new(key_path);
        assert_eq!(store.get().unwrap(), Some(vec![0u8; 32]));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 安全契约：FileStore 的 Debug 输出只含路径，不含密钥字节。
    #[test]
    fn file_store_debug_hides_key_material() {
        let dir = file_store_dir("debug");
        let key_path = dir.join("portable.key");
        let store = FileStore::new(key_path.clone());
        store.set(b"0123456789abcdef0123456789abcdef").unwrap();
        let dbg = format!("{store:?}");
        assert!(dbg.contains("portable.key"), "路径可展示：{dbg}");
        assert!(
            !dbg.contains("0123456789abcdef"),
            "Debug 不得携带密钥字节：{dbg}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：set 用 create_new——已存在密钥文件时拒绝覆盖（两实例首启
    /// 竞态由 Vault::open 的回读校验兜底，此处只验证不覆盖）。
    #[test]
    fn file_store_set_refuses_overwrite() {
        let dir = file_store_dir("overwrite");
        let key_path = dir.join("portable.key");
        std::fs::write(&key_path, vec![1u8; 32]).unwrap();
        let store = FileStore::new(key_path.clone());
        assert!(store.set(b"0123456789abcdef0123456789abcdef").is_err());
        assert_eq!(
            std::fs::read(&key_path).unwrap(),
            vec![1u8; 32],
            "原密钥不被覆盖"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：FileStore 全链路经 Vault::open——首启生成、重开解密一致
    /// （便携版"即插即用"的最小语义闭环）。
    #[test]
    fn vault_open_with_file_store_roundtrip() {
        use crate::Vault;
        let dir = file_store_dir("vault");
        let key_path = dir.join("portable.key");
        let ciphertext = {
            let vault = Vault::open(&FileStore::new(key_path.clone())).unwrap();
            vault.encrypt("secret-api-key", "provider-1").unwrap()
        };
        // 新实例重开（同一文件密钥）可解密
        let vault = Vault::open(&FileStore::new(key_path)).unwrap();
        assert_eq!(
            vault.decrypt(&ciphertext, "provider-1").unwrap(),
            "secret-api-key"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
