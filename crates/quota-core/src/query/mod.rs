//! 查询引擎：解密凭据 → 分派 Provider → 统一超时。
//!
//! 引擎不持有 Vault（解密是 config 层与调用方的职责组合点），
//! 不感知前端形态（CLI / GUI 同一入口）。

use std::sync::Arc;
use std::time::Duration;

use crate::config::{Credentials, ProviderEntry, ProviderKind};
use crate::http::{HttpClient, ReqwestHttpClient};
use crate::model::QueryError;
use crate::model::UsageData;
use crate::provider;
use crate::vault::Vault;

/// 业务级默认超时（取 cc-switch clamp(2,30) 区间内的 15 秒）。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

pub struct QueryEngine {
    http: Arc<dyn HttpClient>,
    timeout: Duration,
}

impl QueryEngine {
    pub fn new(http: Arc<dyn HttpClient>, timeout: Duration) -> Self {
        Self { http, timeout }
    }

    /// 生产默认构造：reqwest 客户端（rustls）+ 15 秒业务超时。
    ///
    /// 引擎应全局复用单个实例——reqwest Client 内部持有连接池与 DNS 缓存。
    pub fn with_default_client() -> Result<Self, crate::http::HttpError> {
        let http = ReqwestHttpClient::new(DEFAULT_TIMEOUT)?;
        Ok(Self::new(Arc::new(http), DEFAULT_TIMEOUT))
    }

    /// 查询单个供应商条目：解密凭据 → 按 kind 分派 → 超时包裹。
    pub async fn query(
        &self,
        vault: &Vault,
        entry: &ProviderEntry,
    ) -> Result<Vec<UsageData>, QueryError> {
        match &entry.kind {
            ProviderKind::Native { provider: id } => {
                let native = provider::find(id)
                    .ok_or_else(|| QueryError::deterministic(format!("未知的预置平台 id：{id}")))?;
                // CLI 凭据型平台（订阅四家）：凭据由 provider 查询时从本机
                // 官方 CLI 的登录文件只读获取，跳过 api_key 解密前置——
                // api_key_enc 为 None 不再是错误
                let creds = if provider::uses_cli_credentials(id) {
                    Credentials::new("")
                } else {
                    entry.credentials(vault)?
                };
                let fut = native.query(&creds, self.http.as_ref(), entry.plan_variant);
                self.with_timeout(fut).await
            }
            ProviderKind::Template(config) => {
                let creds = entry.credentials(vault)?;
                let fut = crate::template::execute(
                    self.http.as_ref(),
                    config,
                    creds.api_key.as_str(),
                    entry.base_url.as_deref(),
                );
                self.with_timeout(fut).await
            }
            ProviderKind::Script(config) => {
                let creds = entry.credentials(vault)?;
                let fut = crate::script::execute(
                    self.http.as_ref(),
                    config,
                    creds.api_key.as_str(),
                    entry.base_url.as_deref(),
                );
                self.with_timeout(fut).await
            }
        }
    }

    async fn with_timeout<F>(&self, fut: F) -> Result<Vec<UsageData>, QueryError>
    where
        F: std::future::Future<Output = Result<Vec<UsageData>, QueryError>>,
    {
        match tokio::time::timeout(self.timeout, fut).await {
            Ok(result) => result,
            Err(_elapsed) => Err(QueryError::transient(format!(
                "查询超时（{} 秒）",
                self.timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlanVariant;
    use crate::config::ProviderEntry;
    use crate::provider::testing::MockHttp;
    use crate::vault::InMemoryStore;

    fn entry(provider: &str) -> ProviderEntry {
        ProviderEntry {
            id: "e1".into(),
            name: "测试条目".into(),
            kind: ProviderKind::Native {
                provider: provider.into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
        }
    }

    /// 全链契约：密文凭据 → 解密 → mock HTTP → UsageData。
    #[tokio::test]
    async fn full_pipeline_with_encrypted_credentials() {
        let store = InMemoryStore::new();
        let vault = Vault::open(&store).unwrap();
        let mut e = entry("deepseek");
        e.set_api_key(&vault, "sk-real-key").unwrap();

        let http = MockHttp::ok(
            r#"{"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"88.00"}]}"#,
        );
        let engine = QueryEngine::new(Arc::new(http), DEFAULT_TIMEOUT);
        let data = engine.query(&vault, &e).await.unwrap();
        assert_eq!(data[0].remaining, Some(88.0));
    }

    /// 契约：超时 → 瞬时失败。
    #[tokio::test]
    async fn timeout_is_transient() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut e = entry("deepseek");
        e.set_api_key(&vault, "k").unwrap();

        let engine = QueryEngine::new(
            Arc::new(MockHttp::delayed(Duration::from_millis(200))),
            Duration::from_millis(20),
        );
        let err = engine.query(&vault, &e).await.unwrap_err();
        assert!(err.is_transient());
    }

    /// 契约：未注册的平台 id → 确定性失败。
    #[tokio::test]
    async fn unknown_native_id_is_deterministic() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut e = entry("no-such-platform");
        e.set_api_key(&vault, "k").unwrap();
        let engine = QueryEngine::new(Arc::new(MockHttp::ok("{}")), DEFAULT_TIMEOUT);
        assert!(!engine.query(&vault, &e).await.unwrap_err().is_transient());
    }

    /// 契约：未配置凭据 → 确定性失败（不发起网络请求）。
    #[tokio::test]
    async fn missing_credentials_is_deterministic() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let engine = QueryEngine::new(Arc::new(MockHttp::ok("{}")), DEFAULT_TIMEOUT);
        assert!(
            !engine
                .query(&vault, &entry("deepseek"))
                .await
                .unwrap_err()
                .is_transient()
        );
    }

    /// 契约：CLI 凭据型平台（订阅四家）api_key_enc 为 None 也不在
    /// 解密前置报「未配置 API key」——凭据由 provider 从本机 CLI 登录
    /// 文件获取（本机文件缺失时是 provider 层的确定性引导错误）。
    #[tokio::test]
    async fn cli_credential_platform_runs_without_api_key() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let engine = QueryEngine::new(Arc::new(MockHttp::ok("{}")), DEFAULT_TIMEOUT);
        let err = engine.query(&vault, &entry("claude")).await.unwrap_err();
        let message = err.message();
        assert!(
            !message.contains("未配置 API key"),
            "CLI 凭据型平台不应报缺 key：{message}"
        );
    }

    /// 契约：引擎层透传 provider 的 401 → 确定性失败。
    #[tokio::test]
    async fn engine_passes_through_401_as_deterministic() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let mut e = entry("openrouter");
        e.set_api_key(&vault, "sk-bad").unwrap();
        let engine = QueryEngine::new(Arc::new(MockHttp::status(401)), DEFAULT_TIMEOUT);
        let err = engine.query(&vault, &e).await.unwrap_err();
        assert!(!err.is_transient());
        assert!(err.message().contains("401"), "实际：{err}");
    }

    /// 契约：Template 条目经引擎全链执行（解密→模板→mock HTTP→UsageData）。
    #[tokio::test]
    async fn engine_executes_template_entry() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let template: crate::TemplateConfig = serde_json::from_value(serde_json::json!({
            "request": { "url": "{{baseUrl}}/user/balance" },
            "extract": { "remaining": "$.balance" }
        }))
        .unwrap();
        let mut e = ProviderEntry {
            id: "tpl1".into(),
            name: "模板测试".into(),
            kind: ProviderKind::Template(Box::new(template)),
            enabled: true,
            api_key_enc: None,
            base_url: Some("https://api.demo.com".into()),
            pricing: None,
            plan_variant: PlanVariant::Auto,
        };
        e.set_api_key(&vault, "sk-tpl").unwrap();

        let engine = QueryEngine::new(
            Arc::new(MockHttp::ok(r#"{"balance":"7.5"}"#)),
            DEFAULT_TIMEOUT,
        );
        let data = engine.query(&vault, &e).await.unwrap();
        assert_eq!(data[0].remaining, Some(7.5));
    }

    /// 契约：Script 条目经引擎全链执行（解密→沙箱→mock HTTP→UsageData）。
    #[tokio::test]
    async fn engine_executes_script_entry() {
        let vault = Vault::open(&InMemoryStore::new()).unwrap();
        let script = crate::ScriptConfig {
            code: r#"
                function request() {
                    return { url: "{{baseUrl}}/v1/balance", headers: { "X-Key": "{{apiKey}}" } };
                }
                function extract(resp) { return { remaining: resp.balance }; }
            "#
            .into(),
            allow_insecure: false,
        };
        let mut e = ProviderEntry {
            id: "scr1".into(),
            name: "脚本测试".into(),
            kind: ProviderKind::Script(Box::new(script)),
            enabled: true,
            api_key_enc: None,
            base_url: Some("https://api.demo.com".into()),
            pricing: None,
            plan_variant: PlanVariant::Auto,
        };
        e.set_api_key(&vault, "sk-scr").unwrap();

        let http = Arc::new(MockHttp::ok(r#"{"balance":"3.25"}"#));
        let engine = QueryEngine::new(http.clone(), DEFAULT_TIMEOUT);
        let data = engine.query(&vault, &e).await.unwrap();
        assert_eq!(data[0].remaining, Some(3.25));
        let reqs = http.captured_requests();
        assert_eq!(reqs[0].url, "https://api.demo.com/v1/balance");
        assert!(
            reqs[0]
                .headers
                .iter()
                .any(|(k, v)| k == "X-Key" && v == "sk-scr")
        );
    }
}
