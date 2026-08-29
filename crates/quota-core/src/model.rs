//! 统一数据契约：所有类型的 Provider（native / template / script）
//! 的查询结果都汇聚为 [`UsageData`]，错误统一为 [`QueryError`] 双轨分类。
//! 约定来源：`docs/项目方案预研.md` §5.2，继承 cc-switch 的生产实践。

use serde::{Deserialize, Serialize};

/// 单个套餐/时间窗口的用量数据。
///
/// 约定：
/// - 百分比统一为**已用百分比**（0-100），不是剩余百分比；
/// - 多窗口（如 5 小时限额 + 周限额）由查询层返回多条 `UsageData`；
/// - `extra` 承载平台特有的结构化附加信息（如 resetTime、planLabel）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageData {
    /// 套餐名（如 "GLM Coding Plan"、"five_hour"）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_name: Option<String>,
    /// 总额度。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    /// 已用额度。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    /// 剩余额度。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<f64>,
    /// 单位："USD" / "CNY" / "%"。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// 额度窗口重置时刻（epoch 毫秒）。订阅/限额窗口的翻转点
    /// （如智谱 5h/周/MCP 窗口的 nextResetTime）；余额类无此概念。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<i64>,
    /// 凭据/套餐是否有效（如 key 过期、未订阅时为 false）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_valid: Option<bool>,
    /// 失效原因，与 `is_valid: false` 搭配出现。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_message: Option<String>,
    /// 平台特有的结构化附加信息。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

/// 查询错误的双轨分类。
///
/// 分类决定调用方行为：[`Transient`](Self::Transient) 可重试且触发
/// keep-last-good（窗口内继续展示上次成功值）；[`Deterministic`](Self::Deterministic)
/// 重试无意义，应立即透出错误文案。
///
/// `detail` 是供用户显式查询的排查信息（脱敏后的响应体片段等），
/// 不参与 [`Display`](std::fmt::Display) 与常规展示，避免日志与
/// 界面膨胀；两变体语义一致，仅随错误类型透传。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// 瞬时失败：网络中断、超时、5xx、429。
    Transient {
        message: String,
        detail: Option<String>,
    },
    /// 确定性失败：401/403 认证失效、响应解析失败、未配置凭据。
    Deterministic {
        message: String,
        detail: Option<String>,
    },
}

impl QueryError {
    pub fn transient(message: impl Into<String>) -> Self {
        Self::Transient {
            message: message.into(),
            detail: None,
        }
    }

    pub fn deterministic(message: impl Into<String>) -> Self {
        Self::Deterministic {
            message: message.into(),
            detail: None,
        }
    }

    /// 附加错误详情（脱敏后的响应体片段、serde 解析位置等），不影响
    /// 分类与 [`message`](Self::message)。链式调用：`deterministic(..).with_detail(..)`。
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = Some(detail.into());
        match &mut self {
            Self::Transient { detail: d, .. } | Self::Deterministic { detail: d, .. } => {
                *d = detail
            }
        }
        self
    }

    /// 是否为瞬时失败（可重试、触发 keep-last-good）。
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Transient { .. })
    }

    /// 错误文案（不含凭据信息，可直接透出给用户）。
    pub fn message(&self) -> &str {
        match self {
            Self::Transient { message, .. } | Self::Deterministic { message, .. } => message,
        }
    }

    /// 排查详情（已脱敏，仅供用户显式复制；日志与常规展示不使用）。
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Transient { detail, .. } | Self::Deterministic { detail, .. } => {
                detail.as_deref()
            }
        }
    }
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = if self.is_transient() {
            "瞬时"
        } else {
            "确定性"
        };
        write!(f, "[{kind}] {}", self.message())
    }
}

impl std::error::Error for QueryError {}

/// 已用百分比（0-100）：`unit == "%"` 直读 `used`，否则 `used/total` 换算；
/// 数据不足返回 `None`。与前端 `display.ts` 的 `usedPercent` 互为镜像
/// （低余额卡片高亮与后端低余额提醒共用同一语义）。
pub fn used_percent(data: &UsageData) -> Option<f64> {
    if data.unit.as_deref() == Some("%") {
        return data.used;
    }
    match (data.used, data.total) {
        (Some(used), Some(total)) if total > 0.0 => Some(used / total * 100.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：全字段 UsageData 序列化往返无损。
    #[test]
    fn usage_data_full_roundtrip() {
        let data = UsageData {
            plan_name: Some("five_hour".into()),
            total: Some(100.0),
            used: Some(42.0),
            remaining: Some(58.0),
            unit: Some("%".into()),
            reset_at: Some(1_755_000_000_000),
            is_valid: Some(true),
            invalid_message: None,
            extra: Some(serde_json::json!({ "resetTime": "2026-08-23T12:00:00Z" })),
        };
        let json = serde_json::to_string(&data).unwrap();
        let back: UsageData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, back);
    }

    /// 契约：未设置的字段在 JSON 中省略（快照/配置文件保持紧凑）。
    #[test]
    fn usage_data_omits_unset_fields() {
        let json = serde_json::to_string(&UsageData::default()).unwrap();
        assert_eq!(json, "{}");
    }

    /// 契约：错误分类语义——Transient 可重试，Deterministic 不可。
    #[test]
    fn error_classification_semantics() {
        assert!(QueryError::transient("timeout").is_transient());
        assert!(!QueryError::deterministic("401 unauthorized").is_transient());
    }

    /// 契约：detail 排查详情——双变体均可附加、不影响分类与 message、
    /// Display 不输出 detail（日志/常规展示不携带排查详情的安全契约）。
    #[test]
    fn detail_is_optional_and_excluded_from_display() {
        let err = QueryError::deterministic("响应不是合法 JSON")
            .with_detail("JSON 解析错误：…\n响应体（已脱敏）：…");
        assert_eq!(err.detail(), Some("JSON 解析错误：…\n响应体（已脱敏）：…"));
        assert_eq!(err.message(), "响应不是合法 JSON");
        assert!(!err.is_transient());

        let transient = QueryError::transient("timeout").with_detail("x".repeat(10));
        assert!(transient.is_transient());
        assert_eq!(transient.detail(), Some("xxxxxxxxxx"));

        assert_eq!(QueryError::transient("t").detail(), None);

        let display = format!("{err}");
        assert_eq!(display, "[确定性] 响应不是合法 JSON");
        assert!(
            !display.contains("脱敏"),
            "Display 不应输出 detail：{display}"
        );
    }

    /// 契约：used_percent 与前端 display.ts usedPercent 镜像——
    /// "%" 单位直读 used；否则 used/total 换算；数据不足 None。
    #[test]
    fn used_percent_mirrors_frontend_semantics() {
        // "%" 单位直读 used（订阅/限额窗口的已用百分比）
        let pct = UsageData {
            used: Some(42.0),
            unit: Some("%".into()),
            ..Default::default()
        };
        assert_eq!(used_percent(&pct), Some(42.0));
        // 金额单位走 used/total 换算
        let amount = UsageData {
            used: Some(30.0),
            total: Some(200.0),
            unit: Some("USD".into()),
            ..Default::default()
        };
        assert_eq!(used_percent(&amount), Some(15.0));
        // total <= 0 无意义
        let bad_total = UsageData {
            used: Some(10.0),
            total: Some(0.0),
            ..Default::default()
        };
        assert_eq!(used_percent(&bad_total), None);
        // 字段缺失（余额型无 total 等）
        assert_eq!(used_percent(&UsageData::default()), None);
        // "%" 单位但 used 缺失
        let pct_missing = UsageData {
            unit: Some("%".into()),
            ..Default::default()
        };
        assert_eq!(used_percent(&pct_missing), None);
    }
}
