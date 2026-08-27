//! AES-256-GCM 加密原语与 `v1:` 密文格式。
//!
//! 格式：`v1:<base64(nonce[12] || ciphertext_and_tag)>`，
//! AAD 由调用方（[`super::Vault`](super::Vault)）传入。

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, AeadCore, KeyInit, OsRng},
};
use base64::{Engine, engine::general_purpose::STANDARD as B64};

/// 密文版本前缀，随算法升级递增（v2 = 换算法/参数时新增分支）。
const VERSION: &str = "v1";
/// 主密钥长度（AES-256）。vault 后端（keyring/File）做同值校验用。
pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum CipherError {
    #[error("密文缺少版本前缀")]
    MissingVersion,
    #[error("不支持的密文版本：{version}")]
    UnsupportedVersion { version: String },
    #[error("密文不是合法的 base64：{0}")]
    InvalidBase64(#[source] base64::DecodeError),
    #[error("密文长度异常（nonce 或认证标签缺失）")]
    MalformedCiphertext,
    #[error("主密钥长度非法（期望 32 字节）")]
    InvalidKeyLength,
    #[error("解密失败：密文被篡改、密钥不符或 AAD 不匹配")]
    AuthFailed,
}

/// 生成 32 字节密码学随机主密钥。
pub fn generate_master_key() -> Vec<u8> {
    use aes_gcm::aead::rand_core::RngCore;
    let mut key = vec![0u8; KEY_LEN];
    OsRng.fill_bytes(&mut key);
    key
}

#[derive(Clone)]
pub struct AesGcmCipher {
    cipher: Aes256Gcm,
}

// Aes256Gcm 未实现 Debug；且内部是密钥派生状态，Debug 输出不应携带任何密钥材料。
impl std::fmt::Debug for AesGcmCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AesGcmCipher")
    }
}

impl AesGcmCipher {
    pub fn new(master_key: &[u8]) -> Result<Self, CipherError> {
        if master_key.len() != KEY_LEN {
            // 主密钥来自 generate_master_key 或系统凭据库，长度不符意味着存储被破坏
            return Err(CipherError::InvalidKeyLength);
        }
        let cipher =
            Aes256Gcm::new_from_slice(master_key).map_err(|_| CipherError::InvalidKeyLength)?;
        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<String, CipherError> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let sealed = self
            .cipher
            .encrypt(
                &nonce,
                aes_gcm::aead::Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| CipherError::AuthFailed)?;
        Ok(format!(
            "{VERSION}:{}",
            B64.encode([nonce.as_slice(), sealed.as_slice()].concat())
        ))
    }

    pub fn decrypt(&self, ciphertext: &str, aad: &[u8]) -> Result<Vec<u8>, CipherError> {
        let (version, payload) = ciphertext
            .split_once(':')
            .ok_or(CipherError::MissingVersion)?;
        if version != VERSION {
            return Err(CipherError::UnsupportedVersion {
                version: version.to_string(),
            });
        }
        let raw = B64.decode(payload).map_err(CipherError::InvalidBase64)?;
        if raw.len() <= NONCE_LEN {
            return Err(CipherError::MalformedCiphertext);
        }
        let (nonce, sealed) = raw.split_at(NONCE_LEN);
        self.cipher
            .decrypt(
                Nonce::from_slice(nonce),
                aes_gcm::aead::Payload { msg: sealed, aad },
            )
            .map_err(|_| CipherError::AuthFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let cipher = AesGcmCipher::new(&generate_master_key()).unwrap();
        let ct = cipher.encrypt(b"api-key-123", b"aad").unwrap();
        let pt = cipher.decrypt(&ct, b"aad").unwrap();
        assert_eq!(pt, b"api-key-123");
    }

    #[test]
    fn reject_short_key() {
        assert!(AesGcmCipher::new(b"short").is_err());
    }
}
