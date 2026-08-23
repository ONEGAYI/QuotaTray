//! Provider 类型定义：凭据形状与查询方式分派。

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// 明文凭据。
///
/// 生命周期：解密创建 → 构造请求头 → 发出请求后尽快 drop。
/// `api_key` 用 [`Zeroizing`]`<String>` 包装，drop 时擦除堆内存；
/// Debug 输出打码，不得把明文带进任何日志。
#[derive(Clone)]
pub struct Credentials {
    pub api_key: Zeroizing<String>,
}

impl Credentials {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: Zeroizing::new(api_key.into()),
        }
    }
}

// 明文 key 不进 Debug 输出（安全红线 1）。
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("api_key", &"<redacted>")
            .finish()
    }
}

/// 供应商条目的查询方式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderKind {
    /// 预置原生实现，`provider` 指向 native 注册表 id（如 "deepseek"）。
    Native { provider: String },
    /// 声明式模板（M2）：零代码接入任意平台。Box 收窄枚举尺寸
    /// （模板配置约 384 字节，远大于 Native 的 24 字节）。
    Template(Box<crate::template::TemplateConfig>),
    // M4：Script（QuickJS 沙箱脚本）。
}

/// 订阅套餐变体：某些平台的套餐版本决定限额窗口结构，查询解析据此
/// 过滤窗口行（当前智谱 GLM Coding Plan 使用：v1 无周限额、v2/v3 有）。
/// 其他订阅型平台（如 Kimi/MiniMax）可复用同一语义。
///
/// - [`PlanVariant::Auto`]：按响应自动推断（默认，缺省字段等价于 Auto）；
/// - [`PlanVariant::NoWeekly`]：声明无周限额——只保留短窗（5h）行，
///   其余窗口（周/MCP 等）一律不显示；
/// - [`PlanVariant::Weekly`]：声明有周限额——unit 未标注的窗口条目可
///   兜底填入周槽（用户声明背书，比 Auto 的宁缺毋错更宽松）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanVariant {
    #[default]
    Auto,
    NoWeekly,
    Weekly,
}

impl PlanVariant {
    pub fn is_auto(&self) -> bool {
        *self == Self::Auto
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 安全契约：Debug 输出打码，明文 key 不得出现。
    #[test]
    fn debug_masks_api_key() {
        let creds = Credentials::new("sk-plaintext-secret");
        let dbg = format!("{creds:?}");
        assert!(!dbg.contains("sk-plaintext-secret"), "明文泄漏：{dbg}");
        assert!(dbg.contains("<redacted>"), "应有打码占位：{dbg}");
    }
}
