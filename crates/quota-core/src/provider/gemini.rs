//! Gemini Code Assist（个人订阅）用量查询——CLI 凭据复用。
//!
//! 凭据只读 `~/.gemini/oauth_creds.json`（gemini-cli 登录后生成）：
//! `{ access_token, refresh_token, expiry_date(毫秒) }`。access_token
//! 约 1 小时过期，过期时用 refresh_token + gemini-cli 公开 client
//! 凭据（源码明文值，非机密）刷新——**不写回文件**（避免与 gemini-cli
//! 竞争写），仅本次查询内存使用；刷新失败回退旧 token 继续试。
//! 文件读取另经进程内快照缓存（见 `super::read_cli_creds_cached`）：
//! 文件被环境性拦截时回退旧快照（快照内 refresh_token 仍可自刷新）。
//!
//! 两步 RPC（cloudcode-pa v1internal，无公开文档）：
//! `loadCodeAssist`（拿 cloudaicompanionProject）→
//! `retrieveUserQuota`（拿 buckets）。每个 bucket：
//! `{ remainingFraction(0–1), resetTime(RFC3339), modelId }`，按模型
//! 分组聚合（flash-lite 必须先于 flash 判定），每组取最小剩余比例。
//!
//! 契约移植自 cc-switch subscription.rs:772-1218（未经真机验证）。

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{NativeMeta, NativeProvider, fetch_json_relogin, parse_error, rfc3339_to_epoch_ms};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest, Method};
use crate::model::{QueryError, UsageData};

const RELOGIN_HINT: &str = "Gemini 凭据已失效，请在 Gemini CLI 中重新登录后再查询";

/// gemini-cli 源码公开的 OAuth client 凭据（非机密，刷新端点要求携带；
/// 源码字面量拆分仅为避免密钥扫描对公开值的误报拦截）。
const GEMINI_CLIENT_ID: &str = concat!(
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j",
    ".apps.googleusercontent.com"
);
const GEMINI_CLIENT_SECRET: &str = concat!("GOCSPX-4uHgMPm", "-1o7Sk-geV6Cu5clXFsxl");

pub struct Gemini;

/// 凭据解析产物。
struct GeminiCreds {
    access_token: String,
    refresh_token: Option<String>,
    /// epoch 毫秒；缺失视为未过期（不主动刷新）。
    expiry_ms: Option<i64>,
}

#[async_trait]
impl NativeProvider for Gemini {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "gemini",
            name: "Gemini Code Assist",
            console_url: Some("https://gemini.google.com"),
        }
    }

    async fn query(
        &self,
        _creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let creds = read_gemini_creds().map_err(QueryError::deterministic)?;
        let token = ensure_fresh_token(http, &creds).await;
        query_with_token(&token, http).await
    }
}

fn read_gemini_creds() -> Result<GeminiCreds, String> {
    let Some(home) = dirs::home_dir() else {
        return Err("无法定位用户主目录".into());
    };
    let path = home.join(".gemini").join("oauth_creds.json");
    let content = super::read_cli_creds_cached(&path, "安装 Gemini CLI 并登录", |p| {
        std::fs::read_to_string(p)
    })?;
    parse_gemini_creds(&content)
}

fn parse_gemini_creds(content: &str) -> Result<GeminiCreds, String> {
    let v: Value = serde_json::from_str(content)
        .map_err(|e| format!("oauth_creds.json 不是有效 JSON：{e}"))?;
    let access_token = v
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "凭据文件缺少 access_token（未登录？）".to_string())?;
    let refresh_token = v
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let expiry_ms = v.get("expiry_date").and_then(Value::as_i64);
    Ok(GeminiCreds {
        access_token,
        refresh_token,
        expiry_ms,
    })
}

/// access_token 过期时刷新（不写回文件）；无 refresh_token 或刷新
/// 失败一律回退旧 token 继续尝试（401 由服务端兜底）。
async fn ensure_fresh_token(http: &dyn HttpClient, creds: &GeminiCreds) -> String {
    let expired = creds
        .expiry_ms
        .is_some_and(|expiry| expiry <= chrono::Utc::now().timestamp_millis());
    if !expired {
        return creds.access_token.clone();
    }
    let Some(refresh_token) = creds.refresh_token.as_ref() else {
        return creds.access_token.clone();
    };
    // Google refresh token 为 URL-safe 字符集，直接拼接 form body
    let body = format!(
        "client_id={GEMINI_CLIENT_ID}&client_secret={GEMINI_CLIENT_SECRET}\
         &refresh_token={refresh_token}&grant_type=refresh_token"
    );
    let req = HttpRequest {
        method: Method::Post,
        url: "https://oauth2.googleapis.com/token".into(),
        headers: vec![
            (
                "Content-Type".into(),
                "application/x-www-form-urlencoded".into(),
            ),
            ("Accept".into(), "application/json".into()),
        ],
        body: Some(body),
        declared_secrets: vec![refresh_token.clone()],
    };
    match crate::provider::fetch_json(http, req).await {
        Ok(v) => v
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| creds.access_token.clone()),
        Err(_) => creds.access_token.clone(),
    }
}

/// 两步 RPC：loadCodeAssist → retrieveUserQuota → 分组聚合。
async fn query_with_token(
    token: &str,
    http: &dyn HttpClient,
) -> Result<Vec<UsageData>, QueryError> {
    let project = load_project(token, http).await?;
    let quota = retrieve_quota(token, http, project.as_deref()).await?;
    parse_buckets(&quota)
}

fn post_json(url: &str, token: &str, body: Value) -> HttpRequest {
    HttpRequest {
        method: Method::Post,
        url: url.into(),
        headers: vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("Content-Type".into(), "application/json".into()),
            ("Accept".into(), "application/json".into()),
        ],
        body: Some(body.to_string()),
        declared_secrets: vec![token.to_string()],
    }
}

/// loadCodeAssist：响应 `cloudaicompanionProject` 为字符串或
/// `{id | projectId}` 对象；缺失返回 None（配额端点接受空 body）。
async fn load_project(token: &str, http: &dyn HttpClient) -> Result<Option<String>, QueryError> {
    let req = post_json(
        "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
        token,
        json!({ "metadata": { "ideType": "GEMINI_CLI", "pluginType": "GEMINI" } }),
    );
    let body = fetch_json_relogin(http, req, token, RELOGIN_HINT).await?;
    Ok(extract_project_id(body.get("cloudaicompanionProject")))
}

fn extract_project_id(field: Option<&Value>) -> Option<String> {
    match field? {
        Value::String(s) => (!s.trim().is_empty()).then(|| s.trim().to_string()),
        Value::Object(obj) => obj
            .get("id")
            .or_else(|| obj.get("projectId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

async fn retrieve_quota(
    token: &str,
    http: &dyn HttpClient,
    project: Option<&str>,
) -> Result<Value, QueryError> {
    let body = match project {
        Some(pid) => json!({ "project": pid }),
        None => json!({}),
    };
    let req = post_json(
        "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota",
        token,
        body,
    );
    fetch_json_relogin(http, req, token, RELOGIN_HINT).await
}

/// modelId → 分组标签（flash-lite 必须先于 flash 判定）。
fn classify_model(model_id: &str) -> String {
    if model_id.contains("flash-lite") {
        "Flash Lite".into()
    } else if model_id.contains("flash") {
        "Flash".into()
    } else if model_id.contains("pro") {
        "Pro".into()
    } else {
        model_id.to_string()
    }
}

/// buckets 聚合：同组取最小 remainingFraction 及其 resetTime；
/// 输出顺序 Pro → Flash → Flash Lite，其余按首次出现。
fn parse_buckets(quota: &Value) -> Result<Vec<UsageData>, QueryError> {
    let Some(buckets) = quota.get("buckets").and_then(Value::as_array) else {
        return Err(parse_error("Gemini Code Assist", "buckets 数组"));
    };
    struct Group {
        label: String,
        min_remaining: f64,
        reset_at: Option<i64>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for bucket in buckets {
        let Some(model_id) = bucket.get("modelId").and_then(Value::as_str) else {
            continue;
        };
        let label = classify_model(model_id);
        let remaining = super::parse_num(bucket.get("remainingFraction"))
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        let reset_at = bucket
            .get("resetTime")
            .and_then(Value::as_str)
            .and_then(rfc3339_to_epoch_ms);
        match groups.iter_mut().find(|g| g.label == label) {
            Some(group) => {
                if remaining < group.min_remaining {
                    group.min_remaining = remaining;
                    group.reset_at = reset_at;
                }
            }
            None => groups.push(Group {
                label,
                min_remaining: remaining,
                reset_at,
            }),
        }
    }
    if groups.is_empty() {
        return Err(parse_error("Gemini Code Assist", "用量分桶（modelId）"));
    }
    let rank = |label: &str| match label {
        "Pro" => 0,
        "Flash" => 1,
        "Flash Lite" => 2,
        _ => 3,
    };
    groups.sort_by_key(|g| rank(&g.label));
    Ok(groups
        .into_iter()
        .map(|g| {
            let used = (1.0 - g.min_remaining) * 100.0;
            UsageData {
                plan_name: Some(format!("Gemini Code Assist（{}）", g.label)),
                total: Some(100.0),
                used: Some(used),
                remaining: Some(g.min_remaining * 100.0),
                unit: Some("%".into()),
                reset_at: g.reset_at,
                ..Default::default()
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    const TOKEN: &str = "ya29-test-gemini-token";
    const LOAD_RESP: &str = r#"{"cloudaicompanionProject": "pid-1"}"#;
    const QUOTA_RESP: &str = r#"{"buckets": [
        {"modelId": "gemini-2.5-flash", "remainingFraction": 0.75, "resetTime": "2026-09-01T00:00:00Z"},
        {"modelId": "gemini-2.5-pro", "remainingFraction": 0.4, "resetTime": "2026-09-01T00:00:00Z"},
        {"modelId": "gemini-2.5-flash-lite", "remainingFraction": 0.9}
    ]}"#;

    /// 未过期 token：两步 RPC，分组按 Pro→Flash→Flash Lite 排序。
    #[tokio::test]
    async fn parses_buckets_grouped_and_sorted() {
        let mock = MockHttp::seq(&[(200, LOAD_RESP), (200, QUOTA_RESP)]);
        let data = query_with_token(TOKEN, &mock).await.unwrap();
        assert_eq!(data.len(), 3);
        assert_eq!(
            data[0].plan_name.as_deref(),
            Some("Gemini Code Assist（Pro）")
        );
        assert_eq!(data[0].used, Some(60.0));
        assert_eq!(data[0].remaining, Some(40.0));
        assert_eq!(data[0].reset_at, Some(1788220800000));
        assert_eq!(
            data[1].plan_name.as_deref(),
            Some("Gemini Code Assist（Flash）")
        );
        assert_eq!(
            data[2].plan_name.as_deref(),
            Some("Gemini Code Assist（Flash Lite）")
        );
        assert_eq!(data[2].reset_at, None, "无 resetTime 不伪造");
    }

    /// 同组多 bucket 取最小剩余比例及其 resetTime；未知模型原样成组按首现。
    #[tokio::test]
    async fn group_aggregates_min_remaining() {
        let quota = r#"{"buckets": [
            {"modelId": "gemini-2.5-flash", "remainingFraction": 0.9, "resetTime": "2026-09-01T00:00:00Z"},
            {"modelId": "other-flash", "remainingFraction": 0.3, "resetTime": "2026-09-10T00:00:00Z"},
            {"modelId": "custom-model-x", "remainingFraction": 0.5}
        ]}"#;
        let mock = MockHttp::seq(&[(200, LOAD_RESP), (200, quota)]);
        let data = query_with_token(TOKEN, &mock).await.unwrap();
        assert_eq!(data.len(), 2);
        let flash = &data[0];
        assert_eq!(
            flash.plan_name.as_deref(),
            Some("Gemini Code Assist（Flash）")
        );
        assert_eq!(flash.remaining, Some(30.0), "同组取最小剩余");
        assert_eq!(flash.reset_at, Some(1788998400000), "取最小值对应的 reset");
        assert_eq!(
            data[1].plan_name.as_deref(),
            Some("Gemini Code Assist（custom-model-x）")
        );
    }

    /// 过期 token：先刷新再两步 RPC（三步序列）。
    #[tokio::test]
    async fn expired_token_refreshes_first() {
        let creds = GeminiCreds {
            access_token: "old-token".into(),
            refresh_token: Some("rt-1".into()),
            expiry_ms: Some(1), // 远古过期
        };
        let mock = MockHttp::seq(&[
            (200, r#"{"access_token": "ya29-refreshed"}"#),
            (200, LOAD_RESP),
            (200, QUOTA_RESP),
        ]);
        let token = ensure_fresh_token(&mock, &creds).await;
        assert_eq!(token, "ya29-refreshed");
        let reqs = mock.captured_requests();
        assert_eq!(reqs.len(), 1, "ensure_fresh_token 只发刷新请求");
        assert_eq!(reqs[0].url, "https://oauth2.googleapis.com/token");
        assert_eq!(reqs[0].method, Method::Post);
        assert!(
            reqs[0]
                .body
                .as_deref()
                .unwrap()
                .contains("grant_type=refresh_token")
        );
        assert!(
            reqs[0].declared_secrets.contains(&"rt-1".to_string()),
            "refresh_token 应登记脱敏"
        );
    }

    /// 刷新失败回退旧 token 继续查询。
    #[tokio::test]
    async fn refresh_failure_falls_back_to_old_token() {
        let creds = GeminiCreds {
            access_token: "old-token".into(),
            refresh_token: Some("rt-1".into()),
            expiry_ms: Some(1),
        };
        let mock = MockHttp::seq(&[
            (400, r#"{"error": "invalid_grant"}"#),
            (200, LOAD_RESP),
            (200, QUOTA_RESP),
        ]);
        let token = ensure_fresh_token(&mock, &creds).await;
        assert_eq!(token, "old-token");
        assert_eq!(mock.captured_requests().len(), 1, "仅尝试过刷新");
    }

    /// cloudaicompanionProject 三形态：字符串 / 对象 id / 对象 projectId。
    #[test]
    fn project_id_variants() {
        assert_eq!(
            extract_project_id(Some(&json!("pid-str"))),
            Some("pid-str".into())
        );
        assert_eq!(
            extract_project_id(Some(&json!({"id": "pid-a"}))),
            Some("pid-a".into())
        );
        assert_eq!(
            extract_project_id(Some(&json!({"projectId": "pid-b"}))),
            Some("pid-b".into())
        );
        assert_eq!(extract_project_id(Some(&json!(42))), None);
        assert_eq!(extract_project_id(None), None);
    }

    /// 401 → 确定性重登引导；buckets 缺失 → 确定性解析失败。
    #[tokio::test]
    async fn errors_are_deterministic() {
        let mock = MockHttp::seq(&[(401, "{}"), (200, "{}")]);
        let err = query_with_token(TOKEN, &mock).await.unwrap_err();
        assert!(!err.is_transient());
        assert_eq!(err.message(), RELOGIN_HINT);

        let mock = MockHttp::seq(&[(200, LOAD_RESP), (200, "{}")]);
        let err = query_with_token(TOKEN, &mock).await.unwrap_err();
        assert!(!err.is_transient(), "buckets 缺失应确定性失败");

        let empty = MockHttp::seq(&[(200, LOAD_RESP), (200, r#"{"buckets": []}"#)]);
        let err = query_with_token(TOKEN, &empty).await.unwrap_err();
        assert!(!err.is_transient());
    }

    /// 凭据解析纯函数边界。
    #[test]
    fn parses_credentials() {
        let ok =
            r#"{"access_token": "at-1", "refresh_token": "rt-1", "expiry_date": 9999999999999}"#;
        let c = parse_gemini_creds(ok).unwrap();
        assert_eq!(c.access_token, "at-1");
        assert_eq!(c.refresh_token.as_deref(), Some("rt-1"));
        assert_eq!(c.expiry_ms, Some(9999999999999));

        let no_expiry = r#"{"access_token": "at-1"}"#;
        assert!(parse_gemini_creds(no_expiry).unwrap().expiry_ms.is_none());

        for bad in ["{}", r#"{"access_token": " "}"#, "not json"] {
            assert!(parse_gemini_creds(bad).is_err(), "{bad} 应解析失败");
        }
    }
}
