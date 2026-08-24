//! quota-core：QuotaTray 业务核心库。
//!
//! 模块规划见 `docs/项目方案预研.md` §3.1：
//! - `model`：统一数据契约（M0）
//! - `vault` / `config` / `http` / `provider` / `query`：核心业务（M1）
//! - `template`：声明式模板 DSL（M2a，core 的 M2 API 面就此冻结）
//! - `update`：GitHub release 检测更新与安装包下载（M4-b）
//! - `pricing`：峰谷定价（时段判定、预置平台定价、自定义合并）
//! - `script`：QuickJS 沙箱脚本查询（M4，`{request, extractor}` 协议）

pub mod config;
pub mod http;
pub mod model;
pub mod pricing;
pub mod provider;
pub mod query;
pub mod script;
pub mod template;
pub mod update;
pub mod vault;

pub use config::{
    AppConfig, CONFIG_EXPORT_EXTENSION, ConfigTransferError, Credentials, PlanVariant,
    ProviderEntry, ProviderKind, export_config, export_config_to_path, import_config,
    import_config_from_path, import_config_to_path,
};
pub use model::{QueryError, UsageData};
pub use pricing::{
    CustomModelDef, PeakKind, PeakWindow, PlanKind, PriceTier, PricingConfig, PricingError,
    PricingSource, ResolvedPricing, default_currency, format_price, next_change, preset,
    preset_with_currency, resolve, resolve_in_currency, resolve_with, validate,
    validate_custom_model,
};
pub use query::{DEFAULT_TIMEOUT, QueryEngine};
pub use script::{ScriptConfig, ScriptError};
pub use template::{TemplateConfig, TemplateError};
pub use update::{
    AssetDownloader, DownloadProgress, DownloadProgressReporter, ReqwestAssetDownloader,
    UpdateError, UpdateStatus, VERSION,
};
pub use vault::{InMemoryStore, KeyringStore, SecretStore, Vault};
