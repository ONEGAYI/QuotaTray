//! Provider 类型定义：凭据形状与查询方式分派。

use serde::{Deserialize, Serialize};

/// 明文凭据。仅在解密后、发请求前的短暂窗口内存在于内存。
#[derive(Debug, Clone)]
pub struct Credentials {
    pub api_key: String,
}

/// 供应商条目的查询方式。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderKind {
    /// 预置原生实现，`provider` 指向 native 注册表 id（如 "deepseek"）。
    Native { provider: String },
    // M2：Template（声明式模板）；M4：Script（QuickJS 沙箱脚本）。
}
