//! quota-core：QuotaTray 业务核心库。
//!
//! 模块规划见 `docs/项目方案预研.md` §3.1。M0 仅落地 `model`（统一数据契约）；
//! `provider` / `query` / `template` / `script` / `vault` / `config`
//! 随 M1 起按里程碑逐步建立，不留空壳模块。

pub mod model;

pub use model::{QueryError, UsageData};
