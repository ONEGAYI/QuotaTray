//! 凭据保险库：主密钥管理 + AES-256-GCM 加解密。
//!
//! 密钥层级（见 `docs/项目方案预研.md` §4.1）：
//!
//! ```text
//! 系统凭据库（SecretStore，经 keyring crate）
//!   └─ 主密钥：32 字节随机，首次运行生成，永不落盘明文
//!         │ AES-256-GCM（AAD = 所属条目 id）
//!         ▼
//! config.json 中的凭据字段（v1:<base64(nonce||ciphertext||tag)>）
//! ```

mod cipher;
mod store;

use cipher::AesGcmCipher;
pub use cipher::CipherError;
pub use store::{InMemoryStore, KeyringStore, SecretStore, VaultError};

/// 凭据保险库。持有主密钥（仅内存），提供加解密入口。
///
/// `aad` 参数绑定密文与其所属条目（如 provider id），
/// 密文被挪到其他条目时解密失败——防配置内密文字段错位/互换。
#[derive(Debug, Clone)]
pub struct Vault {
    cipher: AesGcmCipher,
}

impl Vault {
    /// 打开（或首次创建）保险库：从 `store` 读取主密钥，不存在则随机生成并写入。
    pub fn open(store: &dyn SecretStore) -> Result<Self, VaultError> {
        let key = match store.get()? {
            Some(key) => key,
            None => {
                let key = cipher::generate_master_key();
                store.set(&key)?;
                key
            }
        };
        let cipher = AesGcmCipher::new(&key)?;
        Ok(Self { cipher })
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
