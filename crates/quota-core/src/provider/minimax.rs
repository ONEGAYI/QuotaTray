//! MiniMax Coding Plan 套餐额度查询，国内/国际双站。
//!
//! `GET {base}/v1/api/openplatform/coding_plan/remains`（Bearer api key）
//! 响应：`{"model_remains": [...], "base_resp": {"status_code": 0}}`。
//! 端点与字段契约参考 cc-switch coding_plan.rs（其单测样例即本模块
//! 测试 fixture）：`model_remains[]` 只取 `model_name == "general"`，
//! 5h 桶读 `current_interval_remaining_percent`，周桶仅在
//! `current_weekly_status == 1`（有周限额）时展示——==3 为无周限额套餐
//! （剩余恒 100），展示会制造「0% 已用」假窗口。

use async_trait::async_trait;
use serde_json::Value;

use super::{
    NativeMeta, NativeProvider, fetch_json, parse_error, parse_int, parse_num, redact_error_message,
};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest};
use crate::model::{QueryError, UsageData};

/// 双站共享的实现，域名随站点实例化（请求/解析逻辑两站一致）。
pub struct MiniMax {
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    /// 控制台直达预置 URL（双站域名分立）。
    console_url: Option<&'static str>,
}

/// 国内站（api.minimaxi.com）。
pub const MINIMAX_CN: MiniMax = MiniMax {
    id: "minimax",
    name: "MiniMax Coding Plan",
    base_url: "https://api.minimaxi.com",
    console_url: Some("https://platform.minimaxi.com/user-center/payment/balance"),
};

/// 国际站（api.minimax.io）。
pub const MINIMAX_GLOBAL: MiniMax = MiniMax {
    id: "minimax_global",
    name: "MiniMax Coding Plan（国际站）",
    base_url: "https://api.minimax.io",
    console_url: Some("https://platform.minimax.io/user-center/payment/balance"),
};

/// 单窗口行：响应给的是剩余百分比，归一为已用百分比
/// （不裁剪范围，超 100 的已用值保留超用信息）。
fn window_row(
    item: &Value,
    percent_field: &str,
    label: &str,
    reset_field: &str,
) -> Option<UsageData> {
    let remain = parse_num(item.get(percent_field))?;
    Some(UsageData {
        plan_name: Some(format!("MiniMax Coding Plan（{label}）")),
        total: Some(100.0),
        used: Some(100.0 - remain),
        remaining: Some(remain),
        unit: Some("%".into()),
        reset_at: item.get(reset_field).and_then(parse_int),
        is_valid: None,
        invalid_message: None,
        extra: None,
    })
}

/// general 条目 → 5h / 周 两行（顺序固定）；剩余百分比缺失的窗口跳过，
/// 两窗口全无由调用方按结构异常报错。
fn parse_rows(general: &Value) -> Vec<UsageData> {
    let five_hour = window_row(
        general,
        "current_interval_remaining_percent",
        "5h",
        "end_time",
    );
    // ==1 才是有周限额的套餐；==3（无周限，剩余恒 100）等一律不展示
    let weekly_active = general.get("current_weekly_status").and_then(Value::as_i64) == Some(1);
    let weekly = weekly_active
        .then(|| {
            window_row(
                general,
                "current_weekly_remaining_percent",
                "week",
                "weekly_end_time",
            )
        })
        .flatten();
    [five_hour, weekly].into_iter().flatten().collect()
}

#[async_trait]
impl NativeProvider for MiniMax {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: self.id,
            name: self.name,
            console_url: self.console_url,
        }
    }

    async fn query(
        &self,
        creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let req = HttpRequest::get(format!(
            "{}/v1/api/openplatform/coding_plan/remains",
            self.base_url
        ))
        .bearer(&creds.api_key)
        .header("Accept", "application/json");
        let snapshot = req.clone();
        let body = fetch_json(http, req).await?;

        // base_resp.status_code != 0 为业务错误（兼容数字/字符串形态，
        // status_code 缺失时靠后续结构校验兜底）
        if let Some(code) = body
            .get("base_resp")
            .and_then(|r| r.get("status_code"))
            .and_then(parse_int)
            && code != 0
        {
            let message = body
                .get("base_resp")
                .and_then(|r| r.get("status_msg"))
                .and_then(Value::as_str)
                .unwrap_or("未知业务错误");
            return Err(redact_error_message(
                QueryError::deterministic(format!("MiniMax {code}：{message}")),
                &snapshot,
            ));
        }

        // general 条目位置无关；缺失即结构异常（video 等其他模型条目丢弃）
        let general = body
            .get("model_remains")
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item.get("model_name").and_then(Value::as_str) == Some("general"))
            });
        let rows = general.map(parse_rows).unwrap_or_default();
        if rows.is_empty() {
            return Err(parse_error(
                self.name,
                "model_remains 中 general 条目或剩余百分比数值",
            ));
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    fn creds() -> Credentials {
        Credentials::new("sk-minimax-test")
    }

    async fn query_cn(mock: MockHttp) -> Result<Vec<UsageData>, QueryError> {
        MINIMAX_CN.query(&creds(), &mock, PlanVariant::Auto).await
    }

    /// 主路径（cc-switch 单测真实样例）：general 5h 剩 98% / 周剩 95%，
    /// 归一为已用 2% / 5%；video 条目丢弃；重置时间为 epoch 毫秒透传。
    #[tokio::test]
    async fn parses_five_hour_then_week() {
        let body = r#"{
            "model_remains": [
                {
                    "model_name": "general",
                    "current_interval_remaining_percent": 98.0,
                    "current_weekly_remaining_percent": 95.0,
                    "current_interval_status": 1,
                    "current_weekly_status": 1,
                    "end_time": 1780329600000,
                    "weekly_end_time": 1780848000000
                },
                {
                    "model_name": "video",
                    "current_interval_remaining_percent": 100.0,
                    "current_weekly_remaining_percent": 100.0
                }
            ],
            "base_resp": {"status_code": 0, "status_msg": "success"}
        }"#;
        let rows = query_cn(MockHttp::ok(body)).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].plan_name.as_deref(),
            Some("MiniMax Coding Plan（5h）")
        );
        assert_eq!(rows[0].used, Some(2.0));
        assert_eq!(rows[0].remaining, Some(98.0));
        assert_eq!(rows[0].total, Some(100.0));
        assert_eq!(rows[0].unit.as_deref(), Some("%"));
        assert_eq!(rows[0].reset_at, Some(1_780_329_600_000));
        assert_eq!(
            rows[1].plan_name.as_deref(),
            Some("MiniMax Coding Plan（week）")
        );
        assert_eq!(rows[1].used, Some(5.0));
        assert_eq!(rows[1].reset_at, Some(1_780_848_000_000));
    }

    /// 无周限额套餐（weekly_status == 3，剩余恒 100）：只有 5h 行，
    /// 不制造「0% 已用」假周窗口。
    #[tokio::test]
    async fn weekly_status_three_skips_weekly_bucket() {
        let body = r#"{
            "model_remains": [
                {
                    "model_name": "general",
                    "current_interval_remaining_percent": 99,
                    "current_weekly_status": 3,
                    "current_weekly_remaining_percent": 100,
                    "end_time": 1780365600000,
                    "weekly_end_time": 1780848000000
                }
            ],
            "base_resp": {"status_code": 0}
        }"#;
        let rows = query_cn(MockHttp::ok(body)).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].plan_name.as_deref(),
            Some("MiniMax Coding Plan（5h）")
        );
    }

    /// weekly_status == 2 同样跳过周桶（cc-switch 契约：仅 1 激活）。
    #[tokio::test]
    async fn weekly_status_two_skips_weekly_bucket() {
        let body = r#"{
            "model_remains": [
                {"model_name": "general",
                 "current_interval_remaining_percent": 50,
                 "current_weekly_status": 2,
                 "current_weekly_remaining_percent": 60}
            ]
        }"#;
        let rows = query_cn(MockHttp::ok(body)).await.unwrap();
        assert_eq!(rows.len(), 1, "仅 5h 行");
    }

    /// general 条目位置无关：video 在前仍能定位。
    #[tokio::test]
    async fn video_first_still_locates_general() {
        let body = r#"{
            "model_remains": [
                {"model_name": "video", "current_interval_remaining_percent": 100},
                {"model_name": "general",
                 "current_interval_remaining_percent": 88,
                 "current_weekly_status": 1,
                 "current_weekly_remaining_percent": 77}
            ]
        }"#;
        let rows = query_cn(MockHttp::ok(body)).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].used, Some(12.0));
        assert_eq!(rows[1].used, Some(23.0));
    }

    /// 剩余百分比不裁剪：负数保留超用信息（-5 → 已用 105）。
    #[tokio::test]
    async fn out_of_range_percent_is_not_clamped() {
        let body = r#"{
            "model_remains": [
                {"model_name": "general",
                 "current_interval_remaining_percent": -5.0,
                 "current_weekly_status": 1,
                 "current_weekly_remaining_percent": 150.0}
            ]
        }"#;
        let rows = query_cn(MockHttp::ok(body)).await.unwrap();
        assert_eq!(rows[0].used, Some(105.0));
        assert_eq!(rows[1].used, Some(-50.0));
    }

    /// 5h 百分比缺失只跳过 5h 行，周行有效即成功（部分成功先例同 Kimi Code）。
    #[tokio::test]
    async fn missing_interval_percent_keeps_weekly_only() {
        let body = r#"{
            "model_remains": [
                {"model_name": "general",
                 "current_weekly_status": 1,
                 "current_weekly_remaining_percent": 90}
            ]
        }"#;
        let rows = query_cn(MockHttp::ok(body)).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].plan_name.as_deref(),
            Some("MiniMax Coding Plan（week）")
        );
    }

    /// 周百分比缺失（status=1）同样只跳过周行。
    #[tokio::test]
    async fn missing_weekly_percent_skips_weekly_only() {
        let body = r#"{
            "model_remains": [
                {"model_name": "general",
                 "current_interval_remaining_percent": 30,
                 "current_weekly_status": 1}
            ]
        }"#;
        let rows = query_cn(MockHttp::ok(body)).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].plan_name.as_deref(),
            Some("MiniMax Coding Plan（5h）")
        );
    }

    /// 无 general 条目 / 空数组 / 两窗口全无 → 确定性失败。
    #[tokio::test]
    async fn no_general_entry_is_deterministic() {
        for body in [
            r#"{"model_remains":[{"model_name":"video","current_interval_remaining_percent":100}],"base_resp":{"status_code":0}}"#,
            r#"{"model_remains":[],"base_resp":{"status_code":0}}"#,
            r#"{"model_remains":[{"model_name":"general","current_weekly_status":3}]}"#,
        ] {
            let err = query_cn(MockHttp::ok(body)).await.unwrap_err();
            assert!(!err.is_transient(), "body {body} 应为确定性失败");
        }
    }

    /// 业务错误码（status_code != 0）→ 确定性失败并透出平台 status_msg。
    #[tokio::test]
    async fn business_error_code_is_deterministic() {
        let body = r#"{"base_resp":{"status_code":1004,"status_msg":"invalid api key"}}"#;
        let err = query_cn(MockHttp::ok(body)).await.unwrap_err();
        assert!(!err.is_transient());
        assert!(
            format!("{err}").contains("1004") && format!("{err}").contains("invalid api key"),
            "实际：{err}"
        );
    }

    /// 业务错误 status_msg 兼容字符串数字的 status_code。
    #[tokio::test]
    async fn string_status_code_still_checked() {
        let body = r#"{"base_resp":{"status_code":"1004","status_msg":"invalid api key"}}"#;
        let err = query_cn(MockHttp::ok(body)).await.unwrap_err();
        assert!(!err.is_transient());
    }

    /// 安全契约：业务错误 status_msg 中的回显凭据在透出前已脱敏。
    #[tokio::test]
    async fn business_error_message_redacts_echoed_secret() {
        let body = r#"{"base_resp":{"status_code":1004,"status_msg":"invalid key sk-minimax-test provided"}}"#;
        let err = query_cn(MockHttp::ok(body)).await.unwrap_err();
        assert!(
            !err.message().contains("sk-minimax-test"),
            "业务错误 message 泄漏回显凭据：{}",
            err.message()
        );
        assert!(err.message().contains("<redacted>"), "{}", err.message());
    }

    /// 双站契约：两站请求分别打各自域名（路径一致）。
    #[tokio::test]
    async fn both_sites_hit_their_own_domains() {
        let body = r#"{"model_remains":[{"model_name":"general","current_interval_remaining_percent":50}]}"#;
        for (provider, domain) in [
            (&MINIMAX_CN, "https://api.minimaxi.com"),
            (&MINIMAX_GLOBAL, "https://api.minimax.io"),
        ] {
            let mock = MockHttp::ok(body);
            provider
                .query(&creds(), &mock, PlanVariant::Auto)
                .await
                .unwrap();
            assert_eq!(
                mock.captured_requests()[0].url,
                format!("{domain}/v1/api/openplatform/coding_plan/remains")
            );
        }
    }

    /// 字符串数字的剩余百分比同样接受。
    #[tokio::test]
    async fn accepts_string_number_percent() {
        let body = r#"{
            "model_remains": [
                {"model_name": "general", "current_interval_remaining_percent": "98"}
            ]
        }"#;
        let rows = query_cn(MockHttp::ok(body)).await.unwrap();
        assert_eq!(rows[0].remaining, Some(98.0));
    }

    /// 重置时间兼容字符串数字毫秒（parse_int 语义，与其余数值字段一致）。
    #[tokio::test]
    async fn accepts_string_number_reset_time() {
        let body = r#"{
            "model_remains": [
                {"model_name": "general",
                 "current_interval_remaining_percent": 50,
                 "end_time": "1780329600000"}
            ]
        }"#;
        let rows = query_cn(MockHttp::ok(body)).await.unwrap();
        assert_eq!(rows[0].reset_at, Some(1_780_329_600_000));
    }
}
