//! Codex（ChatGPT Plus/Pro 订阅）用量查询——CLI 凭据复用。
//!
//! 凭据只读 `~/.codex/auth.json`（Codex CLI 以 ChatGPT 账号登录后
//! 生成；`auth_mode == "chatgpt"` 才有订阅用量，API key 模式直接
//! 确定性引导）。token 不刷新不落盘（Codex CLI 自刷，`last_refresh`
//! 超 8 天仅视为陈旧提示，不阻断——服务端 401/403 兜底）。
//!
//! `GET https://chatgpt.com/backend-api/wham/usage`：**必须携带
//! `User-Agent: codex-cli`**（否则大概率被 Cloudflare 拦截），存在
//! account_id 时附 `ChatGPT-Account-Id`（多账号防查错）。响应
//! `rate_limit.primary_window / secondary_window`：
//! `{ used_percent（已是百分比）, limit_window_seconds, reset_at（Unix 秒）}`，
//! 窗口标签由秒数推导（18000→5h、604800→week、2592000→30d）。
//!
//! 与 cc-switch 的差异：`limit_window_seconds` 缺失的窗口跳过而非标
//! "unknown"（异常响应宁缺毋错，避免 UI 出现无意义标签）。契约移植
//! 自 cc-switch subscription.rs:462-695（Codex 路径经真机验证）。

use async_trait::async_trait;
use serde_json::Value;

use super::{NativeMeta, NativeProvider, fetch_json_relogin, parse_error, parse_num};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

const RELOGIN_HINT: &str = "Codex 订阅凭据已失效，请在 Codex CLI 中重新登录后再查询";

pub struct Codex;

/// 凭据解析产物：access token + 可选账号 id（多账号头用）。
struct CodexToken {
    access_token: String,
    account_id: Option<String>,
}

// Debug 打码（安全红线：token 不进任何输出）
impl std::fmt::Debug for CodexToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodexToken")
            .field("access_token", &"<redacted>")
            .field("account_id", &self.account_id)
            .finish()
    }
}

#[async_trait]
impl NativeProvider for Codex {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "codex",
            name: "Codex（ChatGPT 订阅）",
            console_url: Some("https://chatgpt.com/#settings/Subscription"),
        }
    }

    async fn query(
        &self,
        _creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let token = read_codex_token().map_err(QueryError::deterministic)?;
        query_with_token(&token, http).await
    }
}

fn read_codex_token() -> Result<CodexToken, String> {
    let Some(home) = dirs::home_dir() else {
        return Err("无法定位用户主目录".into());
    };
    let path = home.join(".codex").join("auth.json");
    let content = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "未找到 {}，请先在本机安装 Codex CLI 并用 ChatGPT 账号登录后再添加本平台",
            path.display()
        )
    })?;
    parse_codex_token(&content)
}

/// 纯函数：auth.json → token。`auth_mode != "chatgpt"`（API key 模式）
/// 视为凭据不可用——无订阅用量可查，给确定性引导而非报错。
fn parse_codex_token(content: &str) -> Result<CodexToken, String> {
    let v: Value =
        serde_json::from_str(content).map_err(|e| format!("auth.json 不是有效 JSON：{e}"))?;
    match v.get("auth_mode").and_then(Value::as_str) {
        None | Some("chatgpt") => {}
        Some(other) => {
            return Err(format!(
                "Codex CLI 当前为 {other} 模式，无订阅用量——请在 Codex CLI 中改用 ChatGPT 账号登录"
            ));
        }
    }
    let tokens = v
        .get("tokens")
        .ok_or_else(|| "auth.json 缺少 tokens 条目（未登录？）".to_string())?;
    let access_token = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "凭据条目缺少 access_token".to_string())?;
    let account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(CodexToken {
        access_token,
        account_id,
    })
}

async fn query_with_token(
    token: &CodexToken,
    http: &dyn HttpClient,
) -> Result<Vec<UsageData>, QueryError> {
    let mut req = HttpRequest::get("https://chatgpt.com/backend-api/wham/usage")
        .bearer(&token.access_token)
        // UA 伪装 Codex CLI：缺此头大概率被 Cloudflare 拦截（cc-switch 实证）
        .header("User-Agent", "codex-cli")
        .header("Accept", "application/json");
    if let Some(account) = token.account_id.as_deref() {
        req = req.header("ChatGPT-Account-Id", account);
    }
    let body = fetch_json_relogin(http, req, &token.access_token, RELOGIN_HINT).await?;
    parse_usage(&body)
}

/// 窗口秒数 → plan_name 括号标注（与 CLI/前端 windowShortLabel 约定对齐）。
fn window_label(seconds: i64) -> String {
    match seconds {
        18_000 => "5h".into(),
        604_800 => "week".into(),
        2_592_000 => "30d".into(),
        _ if seconds >= 86_400 => format!("{}d", seconds / 86_400),
        _ if seconds > 0 => format!("{}h", (seconds / 3_600).max(1)),
        _ => "window".into(),
    }
}

fn parse_usage(body: &Value) -> Result<Vec<UsageData>, QueryError> {
    let rate_limit = body
        .get("rate_limit")
        .ok_or_else(|| parse_error("Codex", "rate_limit"))?;
    let mut rows = Vec::new();
    for key in ["primary_window", "secondary_window"] {
        let Some(window) = rate_limit.get(key) else {
            continue;
        };
        let Some(used) = parse_num(window.get("used_percent")) else {
            continue;
        };
        // 窗口长度缺失属异常响应：跳过该窗口而非伪造标签（宁缺毋错）；
        // parse_int 与 used_percent 的 parse_num 宽度对齐（数字/字符串兼容）
        let Some(seconds) = window
            .get("limit_window_seconds")
            .and_then(super::parse_int)
        else {
            continue;
        };
        let label = window_label(seconds);
        let reset_at = window
            .get("reset_at")
            .and_then(super::parse_int)
            .map(|secs| secs * 1000);
        // 与 zhipu 对齐：钳制防远端异常值把 remaining 顶成负数
        let used = used.clamp(0.0, 100.0);
        rows.push(UsageData {
            plan_name: Some(format!("Codex（{label}）")),
            total: Some(100.0),
            used: Some(used),
            remaining: Some(100.0 - used),
            unit: Some("%".into()),
            reset_at,
            // plan_type（plus/prolite/pro）透传 extra 供排查；
            // additional_rate_limits（按模型的额外限流）首版不解析
            extra: body
                .get("plan_type")
                .filter(|v| !v.is_null())
                .map(|p| serde_json::json!({ "plan_type": p })),
            ..Default::default()
        });
    }
    if rows.is_empty() {
        return Err(parse_error("Codex", "用量窗口（used_percent）"));
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    fn token() -> CodexToken {
        CodexToken {
            access_token: "eyJ-test-codex-token".into(),
            account_id: Some("acct-123".into()),
        }
    }

    /// 正常：primary（5h）+ secondary（week）双窗口，reset_at 秒→毫秒。
    #[tokio::test]
    async fn parses_primary_and_secondary_windows() {
        let body = r#"{"rate_limit": {
            "primary_window": {"used_percent": 42.0, "limit_window_seconds": 18000, "reset_at": 1787659200},
            "secondary_window": {"used_percent": 15.5, "limit_window_seconds": 604800, "reset_at": 1788177600}
        }}"#;
        let data = query_with_token(&token(), &MockHttp::ok(body))
            .await
            .unwrap();
        assert_eq!(data.len(), 2);
        assert!(data[0].extra.is_none(), "无 plan_type 的响应不伪造 extra");
        assert_eq!(data[0].plan_name.as_deref(), Some("Codex（5h）"));
        assert_eq!(data[0].used, Some(42.0));
        assert_eq!(data[0].remaining, Some(58.0));
        assert_eq!(data[0].reset_at, Some(1787659200000));
        assert_eq!(data[1].plan_name.as_deref(), Some("Codex（week）"));
    }

    /// used_percent 越界钳制（>100 不产负 remaining）。
    #[tokio::test]
    async fn clamps_out_of_range_percent() {
        let body = r#"{"rate_limit": {"primary_window": {"used_percent": 250.0, "limit_window_seconds": 18000}}}"#;
        let data = query_with_token(&token(), &MockHttp::ok(body))
            .await
            .unwrap();
        assert_eq!(data[0].used, Some(100.0));
        assert_eq!(data[0].remaining, Some(0.0));
    }

    /// plan_type 透传 extra（真机响应含 plus/prolite/pro 区分订阅档）。
    #[tokio::test]
    async fn plan_type_passed_to_extra() {
        let body = r#"{"plan_type": "prolite", "rate_limit": {"primary_window": {"used_percent": 12, "limit_window_seconds": 604800, "reset_at": 1788152929}, "secondary_window": null}}"#;
        let data = query_with_token(&token(), &MockHttp::ok(body))
            .await
            .unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(
            data[0]
                .extra
                .as_ref()
                .and_then(|v| v.get("plan_type"))
                .and_then(Value::as_str),
            Some("prolite")
        );
    }

    /// 窗口标签映射：30d 与回退形态。
    #[test]
    fn window_labels() {
        assert_eq!(window_label(2_592_000), "30d");
        assert_eq!(window_label(3 * 86_400), "3d");
        assert_eq!(window_label(7_200), "2h");
        assert_eq!(window_label(1_800), "1h", "不足一小时向上取 1h 而非 0h");
        assert_eq!(window_label(0), "window");
    }

    /// 字段缺失跳过单窗口；全部无效 → 确定性失败。
    #[tokio::test]
    async fn missing_fields_skip_or_fail() {
        let one = r#"{"rate_limit": {"primary_window": {"used_percent": 10.0, "limit_window_seconds": 18000}, "secondary_window": {"used_percent": 5.0}}}"#;
        let data = query_with_token(&token(), &MockHttp::ok(one))
            .await
            .unwrap();
        assert_eq!(data.len(), 1, "缺秒数的 secondary 应跳过");

        for body in [
            "{}",
            r#"{"rate_limit": {}}"#,
            r#"{"rate_limit": {"primary_window": {"reset_at": 1}}}"#,
        ] {
            let err = query_with_token(&token(), &MockHttp::ok(body))
                .await
                .unwrap_err();
            assert!(!err.is_transient(), "body {body} 应为确定性失败");
        }
    }

    /// 401/403 → 确定性 + 重新登录引导；429 → 瞬时。
    #[tokio::test]
    async fn auth_failure_hints_relogin() {
        for status in [401u16, 403] {
            let mut mock = MockHttp::ok("");
            mock.status = status;
            let err = query_with_token(&token(), &mock).await.unwrap_err();
            assert!(!err.is_transient());
            assert_eq!(err.message(), RELOGIN_HINT);
        }
        let mut mock = MockHttp::ok("");
        mock.status = 429;
        assert!(
            query_with_token(&token(), &mock)
                .await
                .unwrap_err()
                .is_transient()
        );
    }

    /// 请求契约：UA codex-cli + ChatGPT-Account-Id 必带形态。
    #[tokio::test]
    async fn hits_wham_usage_with_cli_headers() {
        let mock = MockHttp::ok(
            r#"{"rate_limit": {"primary_window": {"used_percent": 1.0, "limit_window_seconds": 18000}}}"#,
        );
        query_with_token(&token(), &mock).await.unwrap();
        let reqs = mock.captured_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, "https://chatgpt.com/backend-api/wham/usage");
        let header = |name: &str| {
            reqs[0]
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(header("user-agent"), Some("codex-cli"));
        assert_eq!(header("chatgpt-account-id"), Some("acct-123"));
        assert_eq!(header("authorization"), Some("Bearer eyJ-test-codex-token"));
        // 无 account_id 时不发该头
        let no_acct = CodexToken {
            access_token: "t".into(),
            account_id: None,
        };
        let mock2 = MockHttp::ok(
            r#"{"rate_limit": {"primary_window": {"used_percent": 1.0, "limit_window_seconds": 18000}}}"#,
        );
        query_with_token(&no_acct, &mock2).await.unwrap();
        let req2 = &mock2.captured_requests()[0];
        assert!(
            req2.headers
                .iter()
                .all(|(k, _)| !k.eq_ignore_ascii_case("chatgpt-account-id"))
        );
    }

    /// 凭据解析：chatgpt 模式 / auth_mode 缺省放行 / API key 模式引导 /
    /// 缺 token / 坏 JSON。
    #[test]
    fn parses_credentials_modes() {
        let ok = r#"{"auth_mode":"chatgpt","tokens":{"access_token":"tok","account_id":"a1"},"last_refresh":"2026-08-20T00:00:00Z"}"#;
        let parsed = parse_codex_token(ok).unwrap();
        assert_eq!(parsed.access_token, "tok");
        assert_eq!(parsed.account_id.as_deref(), Some("a1"));

        let no_mode = r#"{"tokens":{"access_token":"tok"}}"#;
        assert!(parse_codex_token(no_mode).is_ok(), "auth_mode 缺省放行");

        let api_key_mode = r#"{"auth_mode":"api_key","tokens":{}}"#;
        let err = parse_codex_token(api_key_mode).unwrap_err();
        assert!(err.contains("api_key"), "应指明模式：{err}");

        for bad in [
            "{}",
            r#"{"auth_mode":"chatgpt"}"#,
            r#"{"auth_mode":"chatgpt","tokens":{"access_token":" "}}"#,
            "not json",
        ] {
            assert!(parse_codex_token(bad).is_err(), "{bad} 应解析失败");
        }
    }
}
