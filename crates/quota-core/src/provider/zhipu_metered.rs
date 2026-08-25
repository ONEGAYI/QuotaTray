//! 智谱开放平台 / Z.ai 通用 API 的按量计费余额查询。
//!
//! 通用 API 与 GLM Coding Plan 是独立计费域：本模块使用 Bearer API Key，
//! 优先查询 OpenAI 风格的 `user/credit_grants`；仅当该端点明确返回
//! 404/405 时回退同域的 `balance` 端点。Coding Plan 的裸 key 与百分比
//! 窗口仍由 `zhipu.rs` 独立处理。

use async_trait::async_trait;
use serde_json::Value;

use super::{
    NativeMeta, NativeProvider, parse_error, parse_num, parse_success_json, status_error_with_body,
};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpError, HttpRequest, HttpResponse};
use crate::model::{QueryError, UsageData};

pub struct ZhipuMetered {
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    currency: &'static str,
}

pub const ZHIPU_API: ZhipuMetered = ZhipuMetered {
    id: "zhipu_api",
    name: "智谱开放平台（按量计费）",
    base_url: "https://open.bigmodel.cn",
    currency: "CNY",
};

pub const ZAI_API: ZhipuMetered = ZhipuMetered {
    id: "zai_api",
    name: "Z.ai 开放平台（按量计费）",
    base_url: "https://api.z.ai",
    currency: "USD",
};

fn candidate_objects(body: &Value) -> Vec<&Value> {
    let mut objects = vec![body];
    if let Some(data) = body.get("data") {
        objects.push(data);
        if let Some(grants) = data.get("credit_grants") {
            objects.push(grants);
        }
    }
    objects
}

fn first_num(objects: &[&Value], keys: &[&str]) -> Option<f64> {
    objects
        .iter()
        .find_map(|object| keys.iter().find_map(|key| parse_num(object.get(*key))))
}

fn first_string<'a>(objects: &[&'a Value], keys: &[&str]) -> Option<&'a str> {
    objects.iter().find_map(|object| {
        keys.iter().find_map(|key| {
            object
                .get(*key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
    })
}

fn row(
    plan_name: &str,
    total: Option<f64>,
    used: Option<f64>,
    remaining: Option<f64>,
    currency: &str,
) -> Option<UsageData> {
    if total.is_none() && used.is_none() && remaining.is_none() {
        return None;
    }
    Some(UsageData {
        plan_name: Some(plan_name.into()),
        total,
        used,
        remaining,
        unit: Some(currency.into()),
        reset_at: None,
        is_valid: None,
        invalid_message: None,
        // 余额响应可能包含 grant 标识或账户信息，不向 dev-smoke 透传。
        extra: None,
    })
}

/// OpenAI 风格 credit grants：total_granted / total_used / total_available。
fn parse_credit_grants(body: &Value, plan_name: &str, default_currency: &str) -> Option<UsageData> {
    let objects = candidate_objects(body);
    let mut total = first_num(&objects, &["total_granted", "totalGranted"]);
    let mut used = first_num(&objects, &["total_used", "totalUsed"]);
    let mut remaining = first_num(&objects, &["total_available", "totalAvailable"]);
    if remaining.is_none() {
        remaining = total.zip(used).map(|(limit, spent)| limit - spent);
    }
    if used.is_none() {
        used = total.zip(remaining).map(|(limit, left)| limit - left);
    }
    if total.is_none() {
        total = used.zip(remaining).map(|(spent, left)| spent + left);
    }
    let currency = first_string(&objects, &["currency", "unit"])
        .unwrap_or(default_currency)
        .to_ascii_uppercase();
    row(plan_name, total, used, remaining, &currency)
}

/// 智谱/Z.ai 余额形态：data.available_balance / used_balance / total_balance。
fn parse_balance(body: &Value, plan_name: &str, default_currency: &str) -> Option<UsageData> {
    let objects = candidate_objects(body);
    let total = first_num(&objects, &["total_balance", "totalBalance"]);
    let used = first_num(&objects, &["used_balance", "usedBalance"]);
    let remaining = first_num(&objects, &["available_balance", "availableBalance"]).or(total);
    let currency = first_string(&objects, &["currency", "unit"])
        .unwrap_or(default_currency)
        .to_ascii_uppercase();
    row(plan_name, total, used, remaining, &currency)
}

fn map_http_error(error: HttpError) -> QueryError {
    match error {
        HttpError::Timeout | HttpError::Network(_) => QueryError::transient(error.to_string()),
        HttpError::InvalidRequest(_) => QueryError::deterministic(error.to_string()),
    }
}

async fn send(
    http: &dyn HttpClient,
    url: String,
    api_key: &str,
) -> Result<(HttpRequest, HttpResponse), QueryError> {
    let req = HttpRequest::get(url)
        .bearer(api_key)
        .header("Accept", "application/json");
    let resp = http.execute(req.clone()).await.map_err(map_http_error)?;
    Ok((req, resp))
}

fn decode(req: &HttpRequest, response: HttpResponse) -> Result<Value, QueryError> {
    if !response.is_success() {
        return Err(status_error_with_body(response.status, &response.body, req));
    }
    parse_success_json(req, &response)
}

#[async_trait]
impl NativeProvider for ZhipuMetered {
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
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let grants_url = format!("{}/api/paas/v4/user/credit_grants", self.base_url);
        let (mut req, mut response) = send(http, grants_url, creds.api_key.as_str()).await?;
        if matches!(response.status, 404 | 405) {
            let (fallback_req, fallback_resp) = send(
                http,
                format!("{}/api/paas/v4/balance", self.base_url),
                creds.api_key.as_str(),
            )
            .await?;
            req = fallback_req;
            response = fallback_resp;
        }
        let body = decode(&req, response)?;
        let row = parse_credit_grants(&body, self.name, self.currency)
            .or_else(|| parse_balance(&body, self.name, self.currency))
            .ok_or_else(|| {
                parse_error(
                    self.name,
                    "total_available/total_used/total_granted 或 data.available_balance 数值",
                )
            })?;
        Ok(vec![row])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;
    use crate::provider::testing::MockHttp;
    use std::sync::{Arc, Mutex};

    fn creds() -> Credentials {
        Credentials::new("metered-test-key")
    }

    fn auth(req: &HttpRequest) -> Option<&str> {
        req.headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.as_str())
    }

    #[tokio::test]
    async fn sends_bearer_requests_to_both_regions() {
        let body = r#"{"total_granted":100,"total_used":25,"total_available":75}"#;
        for (provider, expected) in [
            (
                &ZHIPU_API,
                "https://open.bigmodel.cn/api/paas/v4/user/credit_grants",
            ),
            (&ZAI_API, "https://api.z.ai/api/paas/v4/user/credit_grants"),
        ] {
            let mock = MockHttp::ok(body);
            provider
                .query(&creds(), &mock, PlanVariant::Auto)
                .await
                .unwrap();
            let request = &mock.captured_requests()[0];
            assert_eq!(request.method, Method::Get);
            assert_eq!(request.url, expected);
            assert_eq!(auth(request), Some("Bearer metered-test-key"));
        }
    }

    #[tokio::test]
    async fn parses_credit_grants_and_derives_missing_values() {
        let body = r#"{"data":{"total_granted":"100","total_used":27.5,"currency":"usd"}}"#;
        let rows = ZAI_API
            .query(&creds(), &MockHttp::ok(body), PlanVariant::Auto)
            .await
            .unwrap();
        assert_eq!(rows[0].total, Some(100.0));
        assert_eq!(rows[0].used, Some(27.5));
        assert_eq!(rows[0].remaining, Some(72.5));
        assert_eq!(rows[0].unit.as_deref(), Some("USD"));
        assert_eq!(rows[0].extra, None);
    }

    struct SequenceHttp {
        responses: Mutex<Vec<HttpResponse>>,
        requests: Arc<Mutex<Vec<HttpRequest>>>,
    }

    #[async_trait]
    impl HttpClient for SequenceHttp {
        async fn execute(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
            self.requests.lock().unwrap().push(req);
            Ok(self.responses.lock().unwrap().remove(0))
        }
    }

    #[tokio::test]
    async fn falls_back_to_balance_only_for_missing_grants_endpoint() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mock = SequenceHttp {
            responses: Mutex::new(vec![
                HttpResponse {
                    status: 404,
                    body: "not found".into(),
                    raw: Vec::new(),
                },
                HttpResponse {
                    status: 200,
                    body: r#"{"data":{"available_balance":"8.5","used_balance":1.5,"total_balance":10,"currency":"cny"}}"#.into(),
                    raw: Vec::new(),
                },
            ]),
            requests: Arc::clone(&requests),
        };
        let rows = ZHIPU_API
            .query(&creds(), &mock, PlanVariant::Auto)
            .await
            .unwrap();
        assert_eq!(rows[0].remaining, Some(8.5));
        assert_eq!(rows[0].used, Some(1.5));
        assert_eq!(rows[0].total, Some(10.0));
        assert_eq!(rows[0].unit.as_deref(), Some("CNY"));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].url,
            "https://open.bigmodel.cn/api/paas/v4/balance"
        );
    }

    #[tokio::test]
    async fn invalid_shape_is_deterministic_and_network_is_transient() {
        let invalid = ZHIPU_API
            .query(&creds(), &MockHttp::ok(r#"{"data":{}}"#), PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(!invalid.is_transient());

        let network = ZHIPU_API
            .query(&creds(), &MockHttp::fail(), PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(network.is_transient());
    }

    #[tokio::test]
    async fn classifies_auth_and_rate_limit_without_unsafe_fallback() {
        let auth_http = MockHttp::status(401);
        let auth_error = ZAI_API
            .query(&creds(), &auth_http, PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(!auth_error.is_transient());
        assert_eq!(auth_http.captured_requests().len(), 1, "401 不得换端点重试");

        let rate_http = MockHttp::status(429);
        let rate_error = ZAI_API
            .query(&creds(), &rate_http, PlanVariant::Auto)
            .await
            .unwrap_err();
        assert!(rate_error.is_transient());
        assert_eq!(rate_http.captured_requests().len(), 1, "429 不得换端点重试");
    }
}
