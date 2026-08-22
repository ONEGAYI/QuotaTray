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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// 瞬时失败：网络中断、超时、5xx、429。
    Transient { message: String },
    /// 确定性失败：401/403 认证失效、响应解析失败、未配置凭据。
    Deterministic { message: String },
}

impl QueryError {
    pub fn transient(message: impl Into<String>) -> Self {
        Self::Transient {
            message: message.into(),
        }
    }

    pub fn deterministic(message: impl Into<String>) -> Self {
        Self::Deterministic {
            message: message.into(),
        }
    }

    /// 是否为瞬时失败（可重试、触发 keep-last-good）。
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Transient { .. })
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
}
