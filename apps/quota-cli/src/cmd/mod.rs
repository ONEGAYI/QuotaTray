//! 子命令实现：每命令一个模块，handler 接收 [`crate::ctx::Ctx`]。

pub mod add;
#[cfg(debug_assertions)]
pub mod devsmoke;
pub mod edit;
pub mod list;
pub mod natives;
pub mod pricing;
pub mod pricing_models;
pub mod query;
pub mod remove;
pub mod setkey;
pub mod template;
pub mod update;
pub mod vault;
