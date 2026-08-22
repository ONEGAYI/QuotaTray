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
    // M2：Template（声明式模板）；M4：Script（QuickJS 沙箱脚本）。
}
