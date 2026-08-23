//! 智谱 BigModel / Z.ai 的 GLM Coding Plan 套餐用量查询（国内/国际双站）。
//!
//! `GET {base}/api/monitor/usage/quota/limit`（**裸 api key，无 Bearer 前缀**）
//! 响应：`{"data": {"limits": [{"percentage": 42.5, ...}]}}`，
//! `percentage` 已是已用百分比（0-100），每项限制窗口一条 `UsageData`。
//!
//! 端点为社区逆向所得、无官方文档（cc-switch 同款，本项目调研报告 §4.2
//! 收录），官方可能变动。仅支持个人版 Coding Plan key；团队版需额外
//! `bigmodel-organization`/`bigmodel-project` 头（已知限制，暂不支持）。

use async_trait::async_trait;

use super::{NativeMeta, NativeProvider, fetch_json, parse_error, parse_num};
use crate::config::Credentials;
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 双站共享的实现，`id`/`base_url` 随站点实例化。
pub struct ZhipuApi {
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
}

/// 智谱国内站（open.bigmodel.cn）。
pub const ZHIPU: ZhipuApi = ZhipuApi {
    id: "zhipu",
    name: "智谱 GLM Coding Plan",
    base_url: "https://open.bigmodel.cn",
};

/// Z.ai 国际站。
pub const ZAI: ZhipuApi = ZhipuApi {
    id: "zai",
    name: "Z.ai GLM Coding Plan",
    base_url: "https://api.z.ai",
};

#[async_trait]
impl NativeProvider for ZhipuApi {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: self.id,
            name: self.name,
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = HttpRequest::get(format!("{}/api/monitor/usage/quota/limit", self.base_url))
            // 智谱侧约定：Authorization 直接放裸 key，无 Bearer 前缀
            .header("Authorization", creds.api_key.as_str());
        let body = fetch_json(http, req).await?;

        let limits = body
            .get("data")
            .and_then(|d| d.get("limits"))
            .and_then(|l| l.as_array())
            .ok_or_else(|| parse_error(self.name, "data.limits 数组"))?;

        // percentage 已是已用百分比：空数组/无一项含数值都视为结构异常
        let mut rows = Vec::new();
        for item in limits {
            let Some(used) = parse_num(item.get("percentage")) else {
                continue;
            };
            rows.push(UsageData {
                plan_name: Some("GLM Coding Plan".into()),
                total: Some(100.0),
                used: Some(used),
                remaining: Some((100.0 - used).max(0.0)),
                unit: Some("%".into()),
                is_valid: None,
                invalid_message: None,
                extra: Some(item.clone()),
            });
        }
        if rows.is_empty() {
            return Err(parse_error(self.name, "limits[].percentage 数值"));
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    fn creds() -> Credentials {
        Credentials::new("raw-key-no-bearer")
    }

    fn auth_of(req: &crate::http::HttpRequest) -> &str {
        req.headers
            .iter()
            .find(|(k, _)| k == "Authorization")
            .map(|(_, v)| v.as_str())
            .unwrap()
    }

    /// 正常响应：每个 limits 项 → 一条已用百分比 UsageData。
    #[tokio::test]
    async fn parses_percentage_windows() {
        let body = r#"{"data":{"limits":[{"percentage":42.5,"name":"5h"},{"percentage":7.0,"name":"week"}]}}"#;
        let data = ZHIPU.query(&creds(), &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].used, Some(42.5));
        assert_eq!(data[0].unit.as_deref(), Some("%"));
        assert_eq!(data[1].remaining, Some(93.0));
    }

    /// 请求头契约：Authorization 为裸 key（无 Bearer 前缀），域名按站点。
    #[tokio::test]
    async fn raw_key_header_and_site_domains() {
        let mock = MockHttp::ok(r#"{"data":{"limits":[{"percentage":1.0}]}}"#);
        ZHIPU.query(&creds(), &mock).await.unwrap();
        let req = &mock.captured_requests()[0];
        assert_eq!(
            req.url,
            "https://open.bigmodel.cn/api/monitor/usage/quota/limit"
        );
        assert_eq!(auth_of(req), "raw-key-no-bearer", "智谱约定为裸 key");

        let mock = MockHttp::ok(r#"{"data":{"limits":[{"percentage":1.0}]}}"#);
        ZAI.query(&creds(), &mock).await.unwrap();
        assert_eq!(
            mock.captured_requests()[0].url,
            "https://api.z.ai/api/monitor/usage/quota/limit"
        );
    }

    /// limits 数组存在但无一项含数值 percentage → 确定性失败。
    #[tokio::test]
    async fn empty_or_numericless_limits_is_deterministic() {
        for body in [
            r#"{"data":{"limits":[]}}"#,
            r#"{"data":{"limits":[{"name":"5h"}]}}"#,
        ] {
            let err = ZHIPU
                .query(&creds(), &MockHttp::ok(body))
                .await
                .unwrap_err();
            assert!(!err.is_transient(), "body: {body}");
        }
    }

    /// 结构缺失（data/limits 不存在）→ 确定性失败。
    #[tokio::test]
    async fn missing_structure_is_deterministic() {
        let err = ZHIPU
            .query(&creds(), &MockHttp::ok(r#"{"data":{}}"#))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }
}
