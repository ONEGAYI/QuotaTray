//! quota-core：QuotaTray 业务核心库。
//!
//! 模块规划见 `docs/项目方案预研.md` §3.1：
//! - `model`：统一数据契约（M0）
//! - `vault` / `config` / `http` / `provider` / `query`：核心业务（M1）
//! - `template`（M2）/ `script`（M4）随里程碑建立，不留空壳模块。

pub mod config;
pub mod http;
pub mod model;
pub mod provider;
pub mod query;
pub mod vault;

pub use config::{AppConfig, Credentials, ProviderEntry, ProviderKind};
pub use model::{QueryError, UsageData};
pub use query::{DEFAULT_TIMEOUT, QueryEngine};
pub use vault::{InMemoryStore, KeyringStore, SecretStore, Vault};
