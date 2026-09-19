//! 凭据保险库：主密钥管理 + AES-256-GCM 加解密。
//!
//! 密钥层级（见 `docs/项目方案预研.md` §4.1）：
//!
//! ```text
//! 系统凭据库（SecretStore，经 keyring-core + 平台原生 Store）
//!   └─ 主密钥：32 字节随机，首次运行生成，永不落盘明文
//!         │ AES-256-GCM（AAD = 所属条目 id）
//!         ▼
//! config.json 中的凭据字段（v1:<base64(nonce||ciphertext||tag)>）
//! ```

mod cipher;
mod store;

use cipher::AesGcmCipher;
pub use cipher::CipherError;
// crate 内中转：迁移容器 v3 密码档布局常量与 cipher 实现共享同一 nonce 长度。
pub(crate) use cipher::NONCE_LEN;
pub use store::{FileStore, InMemoryStore, KeyringStore, SecretStore, VaultError};

/// 凭据保险库。持有主密钥（仅内存），提供加解密入口。
///
/// `aad` 参数绑定密文与其所属条目（如 provider id），
/// 密文被挪到其他条目时解密失败——防配置内密文字段错位/互换。
///
/// 刻意不实现 Clone：克隆会让主密钥材料在进程内不受控扩散。
#[derive(Debug)]
pub struct Vault {
    cipher: AesGcmCipher,
}

impl Vault {
    /// 使用显式密钥构造仅限 crate 内部使用的临时保险库。
    ///
    /// 迁移模块用它承载每次导出新生成的一次性迁移密钥；生产调用方不能借此
    /// 读取、注入或替换系统凭据库中的机器主密钥。
    pub(crate) fn from_master_key(master_key: &[u8]) -> Result<Self, VaultError> {
        Ok(Self {
            cipher: AesGcmCipher::new(master_key)?,
        })
    }

    /// 创建一次性临时保险库，同时返回需写入迁移容器的随机密钥。
    pub(crate) fn transient() -> Result<(Self, zeroize::Zeroizing<Vec<u8>>), VaultError> {
        let key = zeroize::Zeroizing::new(cipher::generate_master_key());
        let vault = Self::from_master_key(&key)?;
        Ok((vault, key))
    }

    /// 打开（或首次创建）保险库：从 `store` 读取主密钥，不存在则随机生成并写入。
    ///
    /// 写入后回读校验，检测同机多进程并发首次初始化的覆盖竞态
    /// （后写者覆盖先写者时，先写者在此报错而非用被覆盖的密钥加密数据）。
    ///
    /// 红线护栏：`store` 为 [`crate::FileStore`]（便携版）时，调用端必须
    /// 先完成「Portable 固定安全提示」的展示与用户显式确认，再进入本
    /// 函数——首启建钥的门控属端侧职责（AGENTS.md 安全红线 §5）。
    pub fn open(store: &dyn SecretStore) -> Result<Self, VaultError> {
        let key = match store.get()? {
            Some(key) => key,
            None => {
                let key = zeroize::Zeroizing::new(cipher::generate_master_key());
                store.set(&key)?;
                let confirmed = store
                    .get()?
                    .ok_or_else(|| VaultError::Store("主密钥写入后读取不到".into()))?;
                if confirmed.as_slice() != key.as_slice() {
                    return Err(VaultError::Store(
                        "主密钥初始化竞态：另一实例已写入不同的主密钥，请重试".into(),
                    ));
                }
                key.to_vec()
            }
        };
        Self::from_master_key(&key)
    }

    /// 加密明文，返回 `v1:<base64(...)>` 格式密文。
    pub fn encrypt(&self, plaintext: &str, aad: &str) -> Result<String, VaultError> {
        Ok(self.cipher.encrypt(plaintext.as_bytes(), aad.as_bytes())?)
    }

    /// 解密 `v1:` 密文。版本不识别、AAD 不匹配、密文被篡改均返回错误。
    pub fn decrypt(&self, ciphertext: &str, aad: &str) -> Result<String, VaultError> {
        let plain = self.cipher.decrypt(ciphertext, aad.as_bytes())?;
        String::from_utf8(plain).map_err(|_| VaultError::InvalidCiphertext {
            reason: "解密后不是合法 UTF-8".into(),
        })
    }

    /// crate 内部：生成随机 GCM nonce 并加密为裸密文（`ciphertext||tag`，无
    /// 版本前缀与 base64），返回 `(nonce, sealed)`。迁移容器 v3 密码档把
    /// nonce 与 Argon2id 参数一并写入容器头部，使全部解密参数随头部自描述。
    pub(crate) fn seal_with_random_nonce(
        &self,
        plaintext: &[u8],
        aad: &str,
    ) -> Result<([u8; cipher::NONCE_LEN], Vec<u8>), VaultError> {
        self.cipher
            .encrypt_with_detached_nonce(plaintext, aad.as_bytes())
            .map_err(VaultError::from)
    }

    /// crate 内部：用容器头部 nonce 解密裸密文，与
    /// [`Self::seal_with_random_nonce`] 成对。
    pub(crate) fn open_with_nonce(
        &self,
        sealed: &[u8],
        aad: &str,
        nonce: &[u8; cipher::NONCE_LEN],
    ) -> Result<zeroize::Zeroizing<Vec<u8>>, VaultError> {
        self.cipher
            .decrypt_with_nonce(sealed, aad.as_bytes(), nonce)
            .map(zeroize::Zeroizing::new)
            .map_err(VaultError::from)
    }
}

/// crate 内部：生成 32 字节密码学随机盐（迁移容器 v3 密码档 Argon2id salt）。
/// 复用 [`cipher::generate_master_key`] 是刻意的同构：盐与主密钥的生成
/// 需求完全一致（CSPRNG 均匀填充 32 字节），密钥学上等价、无相互派生
/// 关系——两者仅字节数巧合相同，独立随机生成互不影响安全性。
pub(crate) fn random_salt() -> [u8; cipher::KEY_LEN] {
    let salt = cipher::generate_master_key();
    let mut fixed = [0_u8; cipher::KEY_LEN];
    fixed.copy_from_slice(&salt);
    fixed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：首次 open 生成主密钥，再次 open 取到同一把（稳定性）。
    #[test]
    fn open_is_stable_across_reopen() {
        let store = InMemoryStore::new();
        let v1 = Vault::open(&store).unwrap();
        let v2 = Vault::open(&store).unwrap();
        let ct = v1.encrypt("secret", "provider-a").unwrap();
        assert_eq!(v2.decrypt(&ct, "provider-a").unwrap(), "secret");
    }

    /// 契约：不同 store（不同机器）主密钥独立，密文互不可解。
    #[test]
    fn master_keys_are_per_store_independent() {
        let machine_a = Vault::open(&InMemoryStore::new()).unwrap();
        let machine_b = Vault::open(&InMemoryStore::new()).unwrap();
        let ct = machine_a.encrypt("secret", "p").unwrap();
        assert!(machine_b.decrypt(&ct, "p").is_err());
    }

    /// 契约：密文带版本前缀 v1:，且同明文两次加密产生不同密文（nonce 随机）。
    #[test]
    fn ciphertext_is_versioned_and_non_deterministic() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let ct1 = vault.encrypt("secret", "p").unwrap();
        let ct2 = vault.encrypt("secret", "p").unwrap();
        assert!(ct1.starts_with("v1:"));
        assert_ne!(ct1, ct2);
    }

    /// 契约：AAD 不匹配（密文挪到其他条目）解密失败。
    #[test]
    fn aad_mismatch_fails() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let ct = vault.encrypt("secret", "provider-a").unwrap();
        assert!(vault.decrypt(&ct, "provider-b").is_err());
    }

    /// 契约：篡改密文字节解密失败（GCM 认证标签）。
    #[test]
    fn tampered_ciphertext_fails() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let ct = vault.encrypt("secret", "p").unwrap();
        let tampered = tamper_base64_payload(&ct);
        assert_ne!(ct, tampered);
        assert!(vault.decrypt(&tampered, "p").is_err());
    }

    /// 契约：未知的密文版本前缀报明确错误（为未来算法迁移留通道）。
    #[test]
    fn unknown_version_rejected() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let err = vault.decrypt("v2:AAAA", "p").unwrap_err();
        assert!(
            matches!(
                err,
                VaultError::Cipher(CipherError::UnsupportedVersion { .. })
            ),
            "expect UnsupportedVersion, got {err:?}"
        );
    }

    /// 契约：空明文可正常往返。
    #[test]
    fn empty_plaintext_roundtrips() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let ct = vault.encrypt("", "p").unwrap();
        assert_eq!(vault.decrypt(&ct, "p").unwrap(), "");
    }

    /// 翻转 base64 载荷中间某个字节，模拟篡改。
    fn tamper_base64_payload(ct: &str) -> String {
        let payload = ct.strip_prefix("v1:").expect("v1 prefix");
        let mut bytes = payload.as_bytes().to_vec();
        let mid = bytes.len() / 2;
        bytes[mid] = match bytes[mid] {
            b'A' => b'B',
            _ => b'A',
        };
        format!("v1:{}", String::from_utf8(bytes).unwrap())
    }
}
