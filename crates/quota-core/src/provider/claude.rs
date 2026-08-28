//! Claude 订阅（Pro/Max）用量查询——CLI 凭据复用。
//!
//! 凭据只读 `~/.claude/.credentials.json`（Claude Code 登录后生成），
//! QuotaTray 不做 OAuth 登录、不刷新、不落盘：token 仅在查询期间
//! 存在于内存。过期不预判（`expiresAt` 多态格式启发不可靠），401/403
//! 由服务端兜底并透出重新登录引导。
//!
//! `GET https://api.anthropic.com/api/oauth/usage`（Bearer +
//! `anthropic-beta: oauth-2025-04-20`）：顶层为窗口名 →
//! `{ utilization: 已用百分比, resets_at: RFC3339 }` 的 map。已知
//! 四窗口按固定顺序取，未知顶层 key（跳过 `extra_usage`）同样解析
//! ——API 新增窗口自动兼容；`extra_usage`（Claude API 月度积分）以
//! 原始 JSON 透传首行 extra。
//!
//! 契约移植自 cc-switch subscription.rs:99-446（未经真机验证，
//! 端点与文件格式均无官方文档）。

use async_trait::async_trait;
use serde_json::Value;

use super::{
    NativeMeta, NativeProvider, fetch_json_relogin, parse_error, parse_num, rfc3339_to_epoch_ms,
};
use crate::config::{Credentials, PlanVariant};
use crate::http::HttpClient;
use crate::model::{QueryError, UsageData};

const RELOGIN_HINT: &str = "Claude 订阅凭据已失效，请在 Claude Code 中重新登录后再查询";

/// 已知窗口（响应顶层 key → plan_name 括号标注）。
const KNOWN_TIERS: [(&str, &str); 4] = [
    ("five_hour", "5h"),
    ("seven_day", "week"),
    ("seven_day_opus", "week·Opus"),
    ("seven_day_sonnet", "week·Sonnet"),
];

pub struct Claude;

#[async_trait]
impl NativeProvider for Claude {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "claude",
            name: "Claude 订阅",
            console_url: Some("https://claude.ai/settings/billing"),
        }
    }

    async fn query(
        &self,
        _creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let token = read_claude_token().map_err(QueryError::deterministic)?;
        query_with_token(&token, http).await
    }
}

/// 默认路径读取并解析凭据（文件缺失/格式非法均为确定性失败 + 引导）。
fn read_claude_token() -> Result<String, String> {
    let Some(home) = dirs::home_dir() else {
        return Err("无法定位用户主目录".into());
    };
    let path = home.join(".claude").join(".credentials.json");
    let content = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "未找到 {}，请先在本机安装并登录 Claude Code 后再添加本平台",
            path.display()
        )
    })?;
    parse_claude_token(&content)
}

/// 纯函数：凭据 JSON → access token。顶层条目两种拼写都兼容
/// （`claudeAiOauth` / `claude.ai_oauth`，cc-switch 同款）。
fn parse_claude_token(content: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(content)
        .map_err(|e| format!(".credentials.json 不是有效 JSON：{e}"))?;
    let entry = v
        .get("claudeAiOauth")
        .or_else(|| v.get("claude.ai_oauth"))
        .ok_or_else(|| "凭据文件缺少 claudeAiOauth 条目（未登录？）".to_string())?;
    entry
        .get("accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "凭据条目缺少 accessToken".to_string())
}

/// 主查询（token 注入，测试主力）。
async fn query_with_token(
    token: &str,
    http: &dyn HttpClient,
) -> Result<Vec<UsageData>, QueryError> {
    let req = crate::http::HttpRequest::get("https://api.anthropic.com/api/oauth/usage")
        .bearer(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json");
    let body = fetch_json_relogin(http, req, token, RELOGIN_HINT).await?;
    parse_usage(&body)
}

/// 窗口 map → UsageData 行：已知四窗口固定顺序 + 未知窗口自动兼容。
fn parse_usage(body: &Value) -> Result<Vec<UsageData>, QueryError> {
    let mut rows = Vec::new();
    let mut first_extra: Option<Value> = None;
    if let Some(extra) = body.get("extra_usage") {
        first_extra = Some(extra.clone());
    }

    let known: Vec<&str> = KNOWN_TIERS.iter().map(|(k, _)| *k).collect();
    // 已知窗口（固定顺序）在前，未知顶层 key 按键序补后
    // （serde_json Map 为 BTreeMap，未开 preserve_order）
    let keys: Vec<&str> = body
        .as_object()
        .map(|map| {
            map.keys()
                .map(String::as_str)
                .filter(|k| *k != "extra_usage" && !known.contains(k))
                .collect()
        })
        .unwrap_or_default();
    let ordered: Vec<&str> = known.into_iter().chain(keys).collect();

    for key in ordered {
        let Some(window) = body.get(key) else {
            continue;
        };
        let Some(utilization) = parse_num(window.get("utilization")) else {
            continue; // 单窗口字段缺失跳过，宁缺毋错
        };
        let label = KNOWN_TIERS
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, label)| *label)
            .unwrap_or(key);
        let mut row = window_row(utilization, label, window);
        if let Some(extra) = first_extra.take() {
            row.extra = Some(extra);
        }
        rows.push(row);
    }

    if rows.is_empty() {
        return Err(parse_error("Claude 订阅", "用量窗口（utilization）"));
    }
    Ok(rows)
}

fn window_row(utilization: f64, label: &str, window: &Value) -> UsageData {
    // 与 zhipu 对齐：钳制防远端异常值把 remaining 顶成负数
    let used = utilization.clamp(0.0, 100.0);
    UsageData {
        plan_name: Some(format!("Claude 订阅（{label}）")),
        total: Some(100.0),
        used: Some(used),
        remaining: Some(100.0 - used),
        unit: Some("%".into()),
        reset_at: window
            .get("resets_at")
            .and_then(Value::as_str)
            .and_then(rfc3339_to_epoch_ms),
        is_valid: None,
        invalid_message: None,
        extra: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    const TOKEN: &str = "sk-ant-oat-test-token";

    /// 正常：已知四窗口按固定顺序 + 未知窗口自动兼容 + extra_usage 透传首行。
    #[tokio::test]
    async fn parses_known_and_unknown_tiers() {
        let body = r#"{
            "seven_day_sonnet": {"utilization": 12.5, "resets_at": "2026-09-01T00:00:00Z"},
            "five_hour": {"utilization": 3.0, "resets_at": "2026-08-25T12:00:00Z"},
            "seven_day_opus": {"utilization": 45.0},
            "seven_day": {"utilization": 20.0},
            "future_window": {"utilization": 1.0},
            "extra_usage": {"is_enabled": true, "monthly_limit": 5.0}
        }"#;
        let data = query_with_token(TOKEN, &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data.len(), 5);
        // 固定顺序：5h → week → week·Opus → week·Sonnet → future_window
        assert_eq!(data[0].plan_name.as_deref(), Some("Claude 订阅（5h）"));
        assert_eq!(data[1].plan_name.as_deref(), Some("Claude 订阅（week）"));
        assert_eq!(
            data[2].plan_name.as_deref(),
            Some("Claude 订阅（week·Opus）")
        );
        assert_eq!(
            data[3].plan_name.as_deref(),
            Some("Claude 订阅（week·Sonnet）")
        );
        assert_eq!(
            data[4].plan_name.as_deref(),
            Some("Claude 订阅（future_window）")
        );
        assert_eq!(data[0].used, Some(3.0));
        assert_eq!(data[0].remaining, Some(97.0));
        assert_eq!(data[0].unit.as_deref(), Some("%"));
        // extra_usage 只透传首行
        assert!(data[0].extra.is_some());
        assert!(data[1].extra.is_none());
    }

    /// resets_at RFC3339 → epoch 毫秒；非法字符串不伪造时间戳。
    #[tokio::test]
    async fn resets_at_rfc3339() {
        let body = r#"{"five_hour": {"utilization": 3.0, "resets_at": "2026-08-25T12:00:00Z"}}"#;
        let data = query_with_token(TOKEN, &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].reset_at, Some(1787659200000));
        let bad = r#"{"five_hour": {"utilization": 3.0, "resets_at": "not-a-date"}}"#;
        let data = query_with_token(TOKEN, &MockHttp::ok(bad)).await.unwrap();
        assert_eq!(data[0].reset_at, None);
    }

    /// 所有窗口 utilization 缺失 → 确定性解析失败（宁缺毋错）。
    #[tokio::test]
    async fn missing_utilization_is_deterministic() {
        for body in [
            "{}",
            r#"{"five_hour": {"resets_at": "2026-08-25T12:00:00Z"}}"#,
            r#"{"five_hour": {"utilization": "abc"}}"#,
        ] {
            let err = query_with_token(TOKEN, &MockHttp::ok(body))
                .await
                .unwrap_err();
            assert!(!err.is_transient(), "body {body} 应为确定性失败");
        }
    }

    /// utilization 越界钳制（>100 不产负 remaining）。
    #[tokio::test]
    async fn clamps_out_of_range_utilization() {
        let body = r#"{"five_hour": {"utilization": 120.0}}"#;
        let data = query_with_token(TOKEN, &MockHttp::ok(body)).await.unwrap();
        assert_eq!(data[0].used, Some(100.0));
        assert_eq!(data[0].remaining, Some(0.0));
    }

    /// 401/403 → 确定性 + 重新登录引导；429 → 瞬时。
    #[tokio::test]
    async fn auth_failure_hints_relogin() {
        for status in [401u16, 403] {
            let mut mock = MockHttp::ok("");
            mock.status = status;
            let err = query_with_token(TOKEN, &mock).await.unwrap_err();
            assert!(!err.is_transient());
            assert_eq!(err.message(), RELOGIN_HINT);
        }
        let mut mock = MockHttp::ok("");
        mock.status = 429;
        assert!(
            query_with_token(TOKEN, &mock)
                .await
                .unwrap_err()
                .is_transient()
        );
    }

    /// 请求契约：usage 端点 + Bearer + anthropic-beta 头。
    #[tokio::test]
    async fn hits_usage_endpoint_with_beta_header() {
        let mock = MockHttp::ok(r#"{"five_hour": {"utilization": 1.0}}"#);
        query_with_token(TOKEN, &mock).await.unwrap();
        let reqs = mock.captured_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, "https://api.anthropic.com/api/oauth/usage");
        let header = |name: &str| {
            reqs[0]
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(
            header("authorization"),
            Some("Bearer sk-ant-oat-test-token")
        );
        assert_eq!(header("anthropic-beta"), Some("oauth-2025-04-20"));
        // token 已登记脱敏清单（错误响应体回显时打码）
        assert!(reqs[0].declared_secrets.contains(&TOKEN.to_string()));
    }

    /// 凭据解析纯函数：两种顶层拼写 / 缺条目 / 缺 token / 坏 JSON。
    #[test]
    fn parses_credentials_variants() {
        for key in ["claudeAiOauth", "claude.ai_oauth"] {
            let ok = format!(r#"{{"{key}": {{"accessToken": "tok-1", "expiresAt": 9999}}}}"#);
            assert_eq!(parse_claude_token(&ok).unwrap(), "tok-1");
        }
        for bad in [
            "{}",
            r#"{"claudeAiOauth": {}}"#,
            r#"{"claudeAiOauth": {"accessToken": "  "}}"#,
            "not json",
        ] {
            assert!(parse_claude_token(bad).is_err(), "{bad} 应解析失败");
        }
    }
}
