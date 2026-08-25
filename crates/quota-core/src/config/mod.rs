//! 配置层：供应商条目与 `~/.quotatray/config.json` 读写。
//!
//! 凭据字段以 vault 密文（`v1:` 前缀）落盘，AAD 绑定条目 id；
//! 配置文件明文 JSON（除凭据密文外均为普通字段），人工可读可备份。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::QueryError;
use crate::pricing::{CustomModelDef, PricingConfig};
use crate::vault::Vault;

mod provider;
mod transfer;

pub use provider::{Credentials, PlanVariant, ProviderKind};
pub use transfer::{
    CONFIG_EXPORT_EXTENSION, ConfigTransferError, TransferBundle, export_config,
    export_config_to_path, import_config, import_config_from_path, import_config_to_path,
};

/// 单个供应商条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderEntry {
    /// 稳定唯一 id（同时作为凭据密文的 AAD）。
    pub id: String,
    /// 显示名。
    pub name: String,
    pub kind: ProviderKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// API key 密文（`v1:...`）。None = 尚未配置凭据。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_enc: Option<String>,
    /// 模板供应商的 {{baseUrl}} 变量来源（明文，非敏感，如 `https://api.xxx.com`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// 峰谷定价自定义（None/字段缺省 = 回退预置；见 `pricing::resolve`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<PricingConfig>,
    /// 订阅套餐变体（默认 Auto 不落盘，旧配置天然兼容；语义见定义处）。
    #[serde(default, skip_serializing_if = "PlanVariant::is_auto")]
    pub plan_variant: PlanVariant,
    /// 查询是否走代理（条目级开关，默认 false 直连）。代理端口来自
    /// settings.json 的全局网络代理端口——开启但未配端口时查询报
    /// 确定性引导错误。旧配置无此字段天然兼容。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub use_proxy: bool,
}

fn default_true() -> bool {
    true
}

impl ProviderEntry {
    /// 写入/更新 API key：以条目 id 为 AAD 加密后保存。
    pub fn set_api_key(
        &mut self,
        vault: &Vault,
        key: &str,
    ) -> Result<(), crate::vault::VaultError> {
        self.api_key_enc = Some(vault.encrypt(key, &self.id)?);
        Ok(())
    }

    /// 解密凭据。未配置凭据视为确定性失败（引导用户补配，而非重试）。
    pub fn credentials(&self, vault: &Vault) -> Result<Credentials, QueryError> {
        let enc = self.api_key_enc.as_ref().ok_or_else(|| {
            QueryError::deterministic(format!("供应商 {}（{}）未配置 API key", self.name, self.id))
        })?;
        let api_key = vault
            .decrypt(enc, &self.id)
            .map_err(|e| QueryError::deterministic(format!("凭据解密失败：{e}")))?;
        Ok(Credentials::new(api_key))
    }
}

/// 应用配置（整个配置文件）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub providers: Vec<ProviderEntry>,
    /// 用户自定义模型定价，按 native id 聚类（见 `pricing::resolve_with`）。
    /// 一次定义、同平台多个条目复用；template 条目不参与（无 native id）。
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub custom_models: std::collections::BTreeMap<String, Vec<CustomModelDef>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("配置读取失败：{0}")]
    Read(#[source] std::io::Error),
    #[error("配置解析失败：{0}")]
    Parse(#[source] serde_json::Error),
    #[error("配置写入失败：{0}")]
    Write(#[source] std::io::Error),
    #[error("无法定位用户主目录")]
    NoHomeDir,
}

impl AppConfig {
    /// 默认配置路径：`~/.quotatray/config.json`。
    pub fn default_path() -> Result<PathBuf, ConfigError> {
        let home = dirs::home_dir().ok_or(ConfigError::NoHomeDir)?;
        Ok(home.join(".quotatray").join("config.json"))
    }

    /// 加载配置；文件不存在返回空配置（首次运行）。
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(ConfigError::Parse),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ConfigError::Read(e)),
        }
    }

    /// 原子保存：先写临时文件再重命名，避免写一半损坏配置。
    ///
    /// tmp 名含进程 id（多进程并发保存不互踩）；rename 失败时清理 tmp 残留。
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(ConfigError::Write)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(ConfigError::Parse)?;
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, text).map_err(ConfigError::Write)?;
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            ConfigError::Write(e)
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::InMemoryStore;

    fn temp_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("quotatray-test-{tag}-{}.json", std::process::id()));
        p
    }

    /// 契约：保存后加载 roundtrip 无损。
    #[test]
    fn save_load_roundtrip() {
        let path = temp_path("roundtrip");
        let cfg = AppConfig::default();
        cfg.save(&path).unwrap();
        let back = AppConfig::load(&path).unwrap();
        assert_eq!(cfg, back);
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：自定义模型库随配置持久化（按 native id 聚类），旧文件无该字段
    /// 加载为空库；空库不落盘字段（serde skip）。
    #[test]
    fn custom_models_roundtrip_and_legacy_compat() {
        use crate::pricing::{CustomModelDef, PriceTier};
        let path = temp_path("custom-models");
        let mut cfg = AppConfig::default();
        cfg.custom_models.insert(
            "deepseek".into(),
            vec![CustomModelDef {
                id: "flash".into(),
                display: "V4 Flash（自算）".into(),
                peak: Some(PriceTier::full(0.11, 3.1, 9.1)),
                ..Default::default()
            }],
        );
        cfg.save(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"custom_models\""), "库字段应落盘");
        assert!(raw.contains("V4 Flash（自算）"));
        let back = AppConfig::load(&path).unwrap();
        assert_eq!(back.custom_models, cfg.custom_models);
        let _ = std::fs::remove_file(&path);

        // 旧格式（无 custom_models 字段）加载 → 空库；空库保存不写字段
        let path = temp_path("legacy");
        std::fs::write(&path, r#"{"providers":[]}"#).unwrap();
        assert!(AppConfig::load(&path).unwrap().custom_models.is_empty());
        AppConfig::default().save(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("custom_models"), "空库省略字段");
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：加载不存在的文件返回空配置（首次运行不报错）。
    #[test]
    fn load_missing_file_yields_empty() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert_eq!(AppConfig::load(&path).unwrap(), AppConfig::default());
    }

    /// 契约：损坏的 JSON 文件报 Parse 错误（而非静默当空配置）。
    #[test]
    fn corrupted_json_fails_with_parse_error() {
        let path = temp_path("corrupted");
        std::fs::write(&path, "{ not valid json").unwrap();
        assert!(matches!(AppConfig::load(&path), Err(ConfigError::Parse(_))));
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：凭据密文落盘——配置文件中不得出现明文 API key。
    #[test]
    fn api_key_never_stored_in_plaintext() {
        let path = temp_path("plaintext");
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut entry = ProviderEntry {
            id: "p1".into(),
            name: "DeepSeek".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        };
        entry.set_api_key(&vault, "sk-plaintext-secret").unwrap();

        let cfg = AppConfig {
            custom_models: Default::default(),
            providers: vec![entry],
        };
        cfg.save(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            !raw.contains("sk-plaintext-secret"),
            "明文凭据泄漏进配置文件"
        );
        assert!(raw.contains("v1:"), "凭据应保存为 v1 密文");
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：写入凭据后可解密取回（AAD 正确）。
    #[test]
    fn credentials_roundtrip_via_vault() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut entry = ProviderEntry {
            id: "p1".into(),
            name: "DeepSeek".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        };
        entry.set_api_key(&vault, "sk-abc").unwrap();
        assert_eq!(
            entry.credentials(&vault).unwrap().api_key.as_str(),
            "sk-abc"
        );
    }

    /// 契约：未配置凭据 → 确定性失败（不触发重试）。
    #[test]
    fn missing_credentials_is_deterministic() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let entry = ProviderEntry {
            id: "p1".into(),
            name: "X".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        };
        let err = entry.credentials(&vault).unwrap_err();
        assert!(!err.is_transient());
    }

    /// 契约：密文挪到其他条目（id 不同 → AAD 不匹配）解密失败。
    #[test]
    fn ciphertext_is_bound_to_entry_id() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut a = ProviderEntry {
            id: "a".into(),
            name: "A".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        };
        a.set_api_key(&vault, "sk-abc").unwrap();
        let mut b = a.clone();
        b.id = "b".into();
        let err = b.credentials(&vault).unwrap_err();
        assert!(!err.is_transient());
    }
}
