//! 完整配置的跨机器迁移格式。
//!
//! 普通 `config.json` 的机器主密钥仍只存在于系统凭据库。显式导出时，先把每条
//! 凭据转写到本次随机生成的一次性迁移密钥，再用同一迁移密钥整体加密配置。
//! 迁移密钥随私有二进制容器携带，因此导出包的敏感级别等同明文凭据。

use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use super::AppConfig;
use crate::vault::{Vault, VaultError};

/// QuotaTray 配置迁移文件的推荐扩展名（不含点）。
pub const CONFIG_EXPORT_EXTENSION: &str = "qtray-export";

const MAGIC: &[u8; 8] = b"QTRAYCFG";
const FORMAT_VERSION: u16 = 1;
const TRANSFER_KEY_LEN: usize = 32;
const VERSION_OFFSET: usize = MAGIC.len();
const KEY_OFFSET: usize = VERSION_OFFSET + 2;
const LENGTH_OFFSET: usize = KEY_OFFSET + TRANSFER_KEY_LEN;
const HEADER_LEN: usize = LENGTH_OFFSET + 4;
const MAX_EXPORT_SIZE: usize = 16 * 1024 * 1024;
const ENVELOPE_AAD: &str = "quotatray-config-export:v1";

/// 配置迁移编码、认证、凭据转写或文件读写错误。
#[derive(Debug, thiserror::Error)]
pub enum ConfigTransferError {
    #[error("配置迁移包格式无效：{reason}")]
    InvalidFormat { reason: &'static str },
    #[error("不支持的配置迁移包版本：{version}")]
    UnsupportedVersion { version: u16 },
    #[error("配置迁移包超过 16 MiB 上限")]
    TooLarge,
    #[error("配置迁移包读取失败：{0}")]
    Read(#[source] std::io::Error),
    #[error("配置迁移包写入失败：{0}")]
    Write(#[source] std::io::Error),
    #[error("配置迁移内容序列化失败：{0}")]
    Serialize(#[source] serde_json::Error),
    #[error("配置迁移内容解析失败：{0}")]
    Parse(#[source] serde_json::Error),
    #[error("配置迁移凭据处理失败：{0}")]
    Vault(#[from] VaultError),
    #[error("导入配置保存失败：{0}")]
    Save(#[from] super::ConfigError),
}

/// 将完整配置编码为携带一次性迁移密钥的私有认证容器。
///
/// 所有已配置凭据必须能被 `source_vault` 解密，否则整次导出失败。
pub fn export_config(
    config: &AppConfig,
    source_vault: &Vault,
) -> Result<Vec<u8>, ConfigTransferError> {
    let (transfer_vault, transfer_key) = Vault::transient()?;
    let mut transferable = config.clone();
    rewrap_credentials(&mut transferable, source_vault, &transfer_vault)?;

    let serialized = Zeroizing::new(
        serde_json::to_string(&transferable).map_err(ConfigTransferError::Serialize)?,
    );
    let sealed = transfer_vault.encrypt(&serialized, ENVELOPE_AAD)?;
    let payload = sealed.as_bytes();
    let total_len = HEADER_LEN
        .checked_add(payload.len())
        .ok_or(ConfigTransferError::TooLarge)?;
    if total_len > MAX_EXPORT_SIZE {
        return Err(ConfigTransferError::TooLarge);
    }
    let payload_len = u32::try_from(payload.len()).map_err(|_| ConfigTransferError::TooLarge)?;

    let mut bytes = Vec::with_capacity(total_len);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
    bytes.extend_from_slice(&transfer_key);
    bytes.extend_from_slice(&payload_len.to_be_bytes());
    bytes.extend_from_slice(payload);
    Ok(bytes)
}

/// 解码迁移容器，并将其中所有凭据转写到 `target_vault`。
///
/// 函数只在完整认证、解析和转写成功后返回配置，不产生部分导入结果。
pub fn import_config(bytes: &[u8], target_vault: &Vault) -> Result<AppConfig, ConfigTransferError> {
    if bytes.len() > MAX_EXPORT_SIZE {
        return Err(ConfigTransferError::TooLarge);
    }
    if bytes.len() < HEADER_LEN {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "文件头或载荷不完整",
        });
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "魔数不匹配",
        });
    }

    let version = u16::from_be_bytes(
        bytes[VERSION_OFFSET..KEY_OFFSET]
            .try_into()
            .expect("fixed version field"),
    );
    if version != FORMAT_VERSION {
        return Err(ConfigTransferError::UnsupportedVersion { version });
    }

    let transfer_key = Zeroizing::new(bytes[KEY_OFFSET..LENGTH_OFFSET].to_vec());
    let payload_len = u32::from_be_bytes(
        bytes[LENGTH_OFFSET..HEADER_LEN]
            .try_into()
            .expect("fixed length field"),
    ) as usize;
    let expected_len = HEADER_LEN
        .checked_add(payload_len)
        .ok_or(ConfigTransferError::TooLarge)?;
    if expected_len > MAX_EXPORT_SIZE {
        return Err(ConfigTransferError::TooLarge);
    }
    if bytes.len() != expected_len {
        return Err(ConfigTransferError::InvalidFormat {
            reason: "载荷长度不符或存在尾随数据",
        });
    }

    let payload = std::str::from_utf8(&bytes[HEADER_LEN..]).map_err(|_| {
        ConfigTransferError::InvalidFormat {
            reason: "加密载荷不是合法 UTF-8",
        }
    })?;
    let transfer_vault = Vault::from_master_key(&transfer_key)?;
    let serialized = Zeroizing::new(transfer_vault.decrypt(payload, ENVELOPE_AAD)?);
    let mut config: AppConfig =
        serde_json::from_str(&serialized).map_err(ConfigTransferError::Parse)?;
    rewrap_credentials(&mut config, &transfer_vault, target_vault)?;
    Ok(config)
}

/// 原子写出配置迁移包；失败时清理同目录临时文件。
pub fn export_config_to_path(
    config: &AppConfig,
    source_vault: &Vault,
    export_path: &Path,
) -> Result<(), ConfigTransferError> {
    let bytes = export_config(config, source_vault)?;
    atomic_write(export_path, &bytes).map_err(ConfigTransferError::Write)
}

/// 从文件读取迁移包并返回已转写到目标保险库的完整配置。
pub fn import_config_from_path(
    export_path: &Path,
    target_vault: &Vault,
) -> Result<AppConfig, ConfigTransferError> {
    let bytes = std::fs::read(export_path).map_err(ConfigTransferError::Read)?;
    import_config(&bytes, target_vault)
}

/// 完整导入迁移包后，原子替换目标配置文件并返回保存的配置。
pub fn import_config_to_path(
    export_path: &Path,
    target_vault: &Vault,
    config_path: &Path,
) -> Result<AppConfig, ConfigTransferError> {
    let config = import_config_from_path(export_path, target_vault)?;
    config.save(config_path)?;
    Ok(config)
}

fn rewrap_credentials(
    config: &mut AppConfig,
    source_vault: &Vault,
    target_vault: &Vault,
) -> Result<(), ConfigTransferError> {
    for provider in &mut config.providers {
        let Some(ciphertext) = provider.api_key_enc.as_deref() else {
            continue;
        };
        let plaintext = Zeroizing::new(source_vault.decrypt(ciphertext, &provider.id)?);
        provider.api_key_enc = Some(target_vault.encrypt(&plaintext, &provider.id)?);
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = temporary_path(path);
    let result = (|| {
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("export");
    path.with_extension(format!("{extension}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    use crate::config::{PlanVariant, ProviderEntry, ProviderKind};
    use crate::pricing::{CustomModelDef, PriceTier, PricingConfig};
    use crate::vault::{InMemoryStore, SecretStore};

    use super::*;

    const SECRET_A: &str = "sk-source-secret-a";
    const SECRET_B: &str = "sk-source-secret-b";

    fn temp_path(tag: &str, extension: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "quotatray-transfer-{tag}-{}-{extension}",
            std::process::id()
        ))
    }

    fn sample_config(vault: &Vault) -> AppConfig {
        let mut native = ProviderEntry {
            id: "native-a".into(),
            name: "敏感显示名称".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: Some("https://sensitive.example.test".into()),
            pricing: Some(PricingConfig {
                model: Some("private-model".into()),
                peak: Some(PriceTier::full(0.1, 1.0, 2.0)),
                ..Default::default()
            }),
            plan_variant: PlanVariant::Weekly,
            use_proxy: false,
        };
        native.set_api_key(vault, SECRET_A).unwrap();

        let template = serde_json::from_value(serde_json::json!({
            "request": {
                "url": "{{baseUrl}}/balance",
                "headers": { "Authorization": "Bearer {{apiKey}}" }
            },
            "extract": {
                "remaining": "$.balance",
                "unit": { "const": "USD" }
            }
        }))
        .unwrap();
        let mut custom = ProviderEntry {
            id: "template-b".into(),
            name: "Template B".into(),
            kind: ProviderKind::Template(Box::new(template)),
            enabled: false,
            api_key_enc: None,
            base_url: Some("https://template.example.test".into()),
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        };
        custom.set_api_key(vault, SECRET_B).unwrap();

        let mut custom_models = BTreeMap::new();
        custom_models.insert(
            "deepseek".into(),
            vec![CustomModelDef {
                id: "private-model".into(),
                display: "私有计价模型".into(),
                peak: Some(PriceTier::full(0.2, 2.0, 4.0)),
                ..Default::default()
            }],
        );
        AppConfig {
            providers: vec![native, custom],
            custom_models,
        }
    }

    #[test]
    fn cross_machine_roundtrip_rewraps_every_credential() {
        let source_store = InMemoryStore::new();
        let target_store = InMemoryStore::new();
        let source_vault = Vault::open(&source_store).unwrap();
        let target_vault = Vault::open(&target_store).unwrap();
        let source_key_before = source_store.get().unwrap().unwrap();
        let target_key_before = target_store.get().unwrap().unwrap();
        let config = sample_config(&source_vault);

        let bytes = export_config(&config, &source_vault).unwrap();
        let imported = import_config(&bytes, &target_vault).unwrap();

        assert_eq!(imported.providers.len(), 2);
        assert_eq!(imported.custom_models, config.custom_models);
        assert_eq!(
            imported.providers[0]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_A
        );
        assert_eq!(
            imported.providers[1]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_B
        );
        assert_ne!(
            imported.providers[0].api_key_enc,
            config.providers[0].api_key_enc
        );
        assert_eq!(source_store.get().unwrap().unwrap(), source_key_before);
        assert_eq!(target_store.get().unwrap().unwrap(), target_key_before);
        assert!(!bytes.windows(32).any(|window| window == source_key_before));
        assert!(!bytes.windows(32).any(|window| window == target_key_before));
    }

    #[test]
    fn export_is_opaque_and_uses_a_fresh_transfer_key() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let config = sample_config(&source_vault);
        let first = export_config(&config, &source_vault).unwrap();
        let second = export_config(&config, &source_vault).unwrap();

        assert_ne!(first, second);
        assert_ne!(
            &first[KEY_OFFSET..LENGTH_OFFSET],
            &second[KEY_OFFSET..LENGTH_OFFSET],
            "每次导出必须换迁移密钥"
        );
        for plain in [
            SECRET_A,
            SECRET_B,
            "敏感显示名称",
            "sensitive.example.test",
            "私有计价模型",
        ] {
            assert!(
                !first
                    .windows(plain.len())
                    .any(|window| window == plain.as_bytes()),
                "导出包泄漏明文：{plain}"
            );
        }
        assert!(serde_json::from_slice::<serde_json::Value>(&first).is_err());
    }

    #[test]
    fn empty_missing_and_empty_string_credentials_roundtrip() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let empty = export_config(&AppConfig::default(), &source_vault).unwrap();
        assert_eq!(
            import_config(&empty, &target_vault).unwrap(),
            AppConfig::default()
        );

        let mut config = sample_config(&source_vault);
        config.providers[0].api_key_enc = None;
        config.providers[1].set_api_key(&source_vault, "").unwrap();
        let imported = import_config(
            &export_config(&config, &source_vault).unwrap(),
            &target_vault,
        )
        .unwrap();
        assert!(imported.providers[0].api_key_enc.is_none());
        assert_eq!(
            imported.providers[1]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            ""
        );
    }

    #[test]
    fn malformed_or_tampered_packages_are_rejected() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let valid = export_config(&sample_config(&source_vault), &source_vault).unwrap();

        let mut bad_magic = valid.clone();
        bad_magic[0] ^= 1;
        assert!(import_config(&bad_magic, &target_vault).is_err());

        let mut bad_version = valid.clone();
        bad_version[VERSION_OFFSET..KEY_OFFSET].copy_from_slice(&2_u16.to_be_bytes());
        assert!(import_config(&bad_version, &target_vault).is_err());

        assert!(import_config(&valid[..20], &target_vault).is_err());

        let mut bad_length = valid.clone();
        bad_length[LENGTH_OFFSET..HEADER_LEN].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(import_config(&bad_length, &target_vault).is_err());

        let mut trailing = valid.clone();
        trailing.push(0);
        assert!(import_config(&trailing, &target_vault).is_err());

        let mut tampered = valid;
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(import_config(&tampered, &target_vault).is_err());

        let oversized = vec![0_u8; MAX_EXPORT_SIZE + 1];
        assert!(matches!(
            import_config(&oversized, &target_vault),
            Err(ConfigTransferError::TooLarge)
        ));
    }

    #[test]
    fn authenticated_but_invalid_content_is_rejected() {
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let (transfer_vault, transfer_key) = Vault::transient().unwrap();

        let invalid_json = package_with_payload(
            &transfer_key,
            &transfer_vault
                .encrypt("{not valid json", ENVELOPE_AAD)
                .unwrap(),
        );
        assert!(matches!(
            import_config(&invalid_json, &target_vault),
            Err(ConfigTransferError::Parse(_))
        ));

        let wrong_envelope_aad = package_with_payload(
            &transfer_key,
            &transfer_vault.encrypt("{}", "wrong-envelope-aad").unwrap(),
        );
        assert!(matches!(
            import_config(&wrong_envelope_aad, &target_vault),
            Err(ConfigTransferError::Vault(_))
        ));

        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut config = sample_config(&source_vault);
        rewrap_credentials(&mut config, &source_vault, &transfer_vault).unwrap();
        config.providers[0].id = "changed-after-encryption".into();
        let serialized = serde_json::to_string(&config).unwrap();
        let bad_credential_aad = package_with_payload(
            &transfer_key,
            &transfer_vault.encrypt(&serialized, ENVELOPE_AAD).unwrap(),
        );
        assert!(matches!(
            import_config(&bad_credential_aad, &target_vault),
            Err(ConfigTransferError::Vault(_))
        ));
    }

    #[test]
    fn corrupt_source_credential_aborts_export() {
        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut config = sample_config(&source_vault);
        config.providers[0].api_key_enc = Some("v1:AAAA".into());
        assert!(export_config(&config, &source_vault).is_err());
    }

    #[test]
    fn path_helpers_replace_only_after_successful_import() {
        let export_path = temp_path("bundle", CONFIG_EXPORT_EXTENSION);
        let config_path = temp_path("target", "json");
        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);

        let source_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let target_vault = Vault::open(&InMemoryStore::new()).unwrap();
        let source = sample_config(&source_vault);
        let original = AppConfig::default();
        original.save(&config_path).unwrap();

        fs::write(&export_path, b"not a QuotaTray package").unwrap();
        assert!(import_config_to_path(&export_path, &target_vault, &config_path).is_err());
        assert_eq!(AppConfig::load(&config_path).unwrap(), original);

        export_config_to_path(&source, &source_vault, &export_path).unwrap();
        let decoded = import_config_from_path(&export_path, &target_vault).unwrap();
        let saved = import_config_to_path(&export_path, &target_vault, &config_path).unwrap();
        assert_eq!(decoded.custom_models, saved.custom_models);
        assert_eq!(decoded.providers.len(), saved.providers.len());
        for (decoded_entry, saved_entry) in decoded.providers.iter().zip(&saved.providers) {
            let mut decoded_public = decoded_entry.clone();
            let mut saved_public = saved_entry.clone();
            decoded_public.api_key_enc = None;
            saved_public.api_key_enc = None;
            assert_eq!(decoded_public, saved_public);
            assert_eq!(
                decoded_entry
                    .credentials(&target_vault)
                    .unwrap()
                    .api_key
                    .as_str(),
                saved_entry
                    .credentials(&target_vault)
                    .unwrap()
                    .api_key
                    .as_str()
            );
        }
        assert_eq!(AppConfig::load(&config_path).unwrap(), saved);
        assert_eq!(
            saved.providers[0]
                .credentials(&target_vault)
                .unwrap()
                .api_key
                .as_str(),
            SECRET_A
        );

        let _ = fs::remove_file(&export_path);
        let _ = fs::remove_file(&config_path);
    }

    fn package_with_payload(transfer_key: &[u8], payload: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
        bytes.extend_from_slice(transfer_key);
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(payload.as_bytes());
        bytes
    }
}
