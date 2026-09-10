//! quota-core：QuotaTray 业务核心库。
//!
//! 模块规划见 `docs/项目方案预研.md` §3.1：
//! - `model`：统一数据契约（M0）
//! - `vault` / `config` / `http` / `provider` / `query`：核心业务（M1）
//! - `template`：声明式模板 DSL（M2a，core 的 M2 API 面就此冻结）
//! - `update`：GitHub release 检测更新与安装包下载（M4-b）
//! - `pricing`：峰谷定价（时段判定、预置平台定价、自定义合并）
//! - `pricing_catalog`：定价目录（JSON 类型、校验与内置种子；预置单一数据源）
//! - `script`：QuickJS 沙箱脚本查询（M4，`{request, extractor}` 协议）
//! - `history`：查询结果的历史存储（M5，SQLite + 版本化迁移）
//! - `runtime`：安装态/便携态运行模式解析（Portable 方案 A，纯函数）
//! - `logging`：结构化事件打点约定（log facade 薄层，两端装配 flexi_logger）

pub mod config;
pub mod history;
pub mod http;
pub mod logging;
pub mod model;
pub mod pricing;
pub mod pricing_catalog;
pub mod provider;
pub mod query;
pub mod runtime;
pub mod script;
pub mod template;
pub mod update;
pub mod vault;

pub use config::{
    AppConfig, CONFIG_EXPORT_EXTENSION, ConfigTransferError, Credentials,
    MAX_USAGE_COMPARISON_SERIES, PlanVariant, ProviderEntry, ProviderKind, TransferBundle,
    UsageComparisonSeries, export_config, export_config_to_path, export_config_to_path_with_usage,
    export_config_with_usage, import_config, import_config_from_path, import_config_to_path,
    sanitize_usage_comparison_series,
};
pub use history::{
    DEFAULT_RETENTION_DAYS, HistoryError, HistoryExportRow, HistoryPoint, HistoryStore, WindowKind,
    window_key, window_kind,
};
pub use logging::EVENT_TARGET;
pub use model::{QueryError, UsageData, used_percent};
pub use pricing::{
    CustomModelDef, PeakKind, PeakWindow, PlanKind, PriceTier, PricingConfig, PricingError,
    PricingSource, ResolvedModelStatus, ResolvedPricing, default_currency, format_price,
    next_change, preset, preset_with_currency, resolve, resolve_in_catalog, resolve_in_currency,
    resolve_with, validate, validate_custom_model,
};
pub use pricing_catalog::sync::{
    CATALOG_CACHE_FILE, CATALOG_LOCK_FILE, CATALOG_MAX_BYTES, CATALOG_URL, CatalogCacheEnvelope,
    CatalogDecision, CatalogOrigin, CatalogStatusView, CatalogSync, CatalogSyncError,
    CatalogUpdateOutcome, EffectiveCatalog, FallbackReason, decide_between,
    effective_from_envelope_json, evaluate_incoming, load_effective,
};
pub use pricing_catalog::{
    CATALOG_SCHEMA_VERSION, Catalog, CatalogError, CatalogModel, CatalogProvider, CatalogSuite,
    MAX_REVISION, ModelStatus, bundled_catalog, find_suite, parse_catalog, validate_catalog,
    validate_no_removal,
};
pub use query::{DEFAULT_TIMEOUT, QueryEngine};
pub use runtime::{
    PORTABLE_DATA_DIR, PORTABLE_KEY, PORTABLE_MARKER, RuntimeMode, has_portable_marker,
    portable_data_root, portable_key_path, resolve_mode,
};
pub use script::{ScriptConfig, ScriptError};
pub use template::{TemplateConfig, TemplateError};
pub use update::{
    AssetDownloader, AssetSelector, DownloadProgress, DownloadProgressReporter, DualHttpClients,
    Flavor, ReqwestAssetDownloader, UpdateChannel, UpdateError, UpdateStatus, VERSION, arch_label,
    build_dual_http_clients, check_update_with_fallback, expected_asset_name, is_stale_installer,
    parse_asset_filename,
};
pub use vault::{FileStore, InMemoryStore, KeyringStore, SecretStore, Vault};
