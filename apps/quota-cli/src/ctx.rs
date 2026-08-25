//! 命令执行上下文：配置路径 + 语言 + 凭据库后端。
//!
//! [`Ctx`] 刻意保持轻薄：vault 与引擎不缓存——`open_vault` 幂等
//! （同一 store 读出同一主密钥，每次调用等价），引擎每进程只建一次
//! （调用方持有 owned 后传引用）。`natives`/`list` 等纯本地命令
//! 不触碰任何后端。
//!
//! `lang` 恒为**已 resolve** 的具体语言（构造方负责消解 System；
//! 文案表 [`crate::texts::t`] 对 System 有中文兜底，但调用不应依赖）。

use std::path::PathBuf;
use std::sync::Arc;

use quota_core::{KeyringStore, QueryEngine, SecretStore, Vault};

use crate::lang::Lang;
use crate::texts::{T, t};

pub struct Ctx {
    pub config_path: PathBuf,
    /// 展示语言（已 resolve，非 System）。
    pub lang: Lang,
    store: Arc<dyn SecretStore>,
}

impl Ctx {
    /// 生产上下文：主密钥后端为系统凭据库（keyring）。
    /// `lang` 须为 resolve 后值（`Lang::System.resolve()`）。
    pub fn production(config_path: PathBuf, lang: Lang) -> Self {
        Self {
            config_path,
            lang,
            store: Arc::new(KeyringStore::new()),
        }
    }

    /// 测试上下文：注入内存后端（语言默认中文，测试可改 `lang` 字段）。
    #[cfg(test)]
    pub fn with_store(config_path: PathBuf, store: Arc<dyn SecretStore>) -> Self {
        Self {
            config_path,
            lang: Lang::Zh,
            store,
        }
    }

    /// 测试链式设置语言（store 私有，跨模块无法用结构体更新语法）。
    #[cfg(test)]
    pub fn with_lang(mut self, lang: Lang) -> Self {
        self.lang = lang;
        self
    }

    /// 主密钥后端（vault status 健康检查用；测试注入内存后端即可测）。
    pub fn store(&self) -> &dyn SecretStore {
        self.store.as_ref()
    }

    /// 打开凭据保险库（幂等：同一 store 恒得同一主密钥，可重复调用）。
    pub fn open_vault(&self) -> Result<Vault, String> {
        Vault::open(self.store.as_ref())
            .map_err(|e| format!("{}{e}", t(self.lang, T::VaultOpenFailCtx)))
    }

    /// 构造生产查询引擎（reqwest + 15 秒超时）。每进程调用一次即可。
    pub fn new_engine(&self) -> Result<QueryEngine, String> {
        build_engine_from_settings(&self.config_path, self.lang)
    }

    /// 历史库路径：与配置文件同目录（`--config` 覆盖时自然跟随）。
    pub fn history_path(&self) -> PathBuf {
        self.config_path
            .parent()
            .map(|dir| dir.join("history.db"))
            .unwrap_or_else(|| PathBuf::from("history.db"))
    }
}

/// 按 settings.json 的网络代理端口构造查询引擎（None = 直连）。
/// 代理与更新检测共用同一端口（chatgpt.com 等被墙站点的订阅查询必需）。
pub fn build_engine_from_settings(
    config_path: &std::path::Path,
    lang: crate::lang::Lang,
) -> Result<QueryEngine, String> {
    let prefs = crate::settings_io::load_prefs(config_path);
    let proxy_url = quota_core::update::proxy_url_of(prefs.update_proxy_port);
    let direct = quota_core::http::ReqwestHttpClient::new(quota_core::DEFAULT_TIMEOUT)
        .map_err(|e| format!("{}{e}", t(lang, T::EngineInitFail)))?;
    let proxied = match proxy_url.as_deref() {
        Some(url) => Some(
            quota_core::http::ReqwestHttpClient::new_with_proxy(
                quota_core::DEFAULT_TIMEOUT,
                Some(url),
            )
            .map_err(|e| format!("代理配置无效：{e}"))?,
        ),
        None => None,
    };
    Ok(QueryEngine::with_proxied(
        std::sync::Arc::new(direct),
        proxied.map(|c| std::sync::Arc::new(c) as std::sync::Arc<dyn quota_core::http::HttpClient>),
        quota_core::DEFAULT_TIMEOUT,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：open_vault 幂等——两次打开的保险库互通（密文可互解）。
    #[test]
    fn open_vault_is_idempotent() {
        let ctx = Ctx::with_store(
            PathBuf::from("unused.json"),
            Arc::new(quota_core::InMemoryStore::new()),
        );
        let v1 = ctx.open_vault().unwrap();
        let v2 = ctx.open_vault().unwrap();
        let ct = v1.encrypt("secret", "p").unwrap();
        assert_eq!(v2.decrypt(&ct, "p").unwrap(), "secret");
    }

    /// 契约：ctx 错误文案双语（语言字段直接驱动）——注入恒失败 store
    /// 驱动 open_vault 真实失败路径，断言消息以前缀开头且透出底层原因。
    #[test]
    fn open_vault_error_prefix_follows_lang() {
        struct FailingStore;
        impl quota_core::SecretStore for FailingStore {
            fn get(&self) -> Result<Option<Vec<u8>>, quota_core::vault::VaultError> {
                Err(quota_core::vault::VaultError::Store("backend down".into()))
            }
            fn set(&self, _key: &[u8]) -> Result<(), quota_core::vault::VaultError> {
                Err(quota_core::vault::VaultError::Store("backend down".into()))
            }
        }

        for lang in [Lang::Zh, Lang::En] {
            let ctx = Ctx::with_store(PathBuf::from("unused.json"), Arc::new(FailingStore));
            let ctx = Ctx { lang, ..ctx };
            let err = ctx.open_vault().unwrap_err();
            assert!(
                err.starts_with(t(lang, T::VaultOpenFailCtx)),
                "{lang:?}: {err}"
            );
            assert!(err.contains("backend down"), "{lang:?}: {err}");
        }
    }
}
