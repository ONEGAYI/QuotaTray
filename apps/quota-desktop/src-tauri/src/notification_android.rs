//! Android 系统通知设置页 JNI 桥：Rust → `NotificationHelper`（Kotlin）。
//!
//! 职责边界与类加载约束同 [`crate::apk_install`]（公共桥
//! `with_helper_class_named` 的模式说明见彼处模块注释）：Intent 构造与
//! 主线程派发归 Kotlin helper（`android-post-init.mjs` 注入，行为由其
//! 契约测试锁定），本模块只做桥接与错误转译。

use jni::objects::JValue;

/// 注入的 Kotlin helper 全限定类名（与 android-post-init.mjs 生成一致）。
const HELPER_CLASS: &str = "com.quotatray.android.NotificationHelper";

/// 跳系统「应用通知设置」页（`ACTION_APP_NOTIFICATION_SETTINGS`，
/// API 26+ 均有；本应用 minSdk 24，API 24/25 无该 action 时 Kotlin 侧
/// 返回 false 降级）。
///
/// - `Ok(true)`：设置页已派发；
/// - `Ok(false)`：系统无该设置页（低版本 ROM），调用方降级为纯文案引导；
/// - `Err`：桥本身故障，确定性错误。
pub fn open_notification_settings() -> Result<bool, String> {
    crate::apk_install::with_helper_class_named(HELPER_CLASS, |env, context, helper| {
        env.call_static_method(
            helper,
            "openNotificationSettings",
            "(Landroid/content/Context;)Z",
            &[JValue::Object(context)],
        )
        .map_err(|e| format!("调用 NotificationHelper.openNotificationSettings 失败：{e}"))?
        .z()
        .map_err(|e| format!("读取通知设置页调用结果失败：{e}"))
    })
}

#[cfg(test)]
mod tests {
    /// 编译期契约：桥函数签名保持命令层友好的形状（真实 JNI 路径仅
    /// 真机/模拟器可验）。
    #[test]
    fn bridge_signatures_are_command_friendly() {
        let _open: fn() -> Result<bool, String> = super::open_notification_settings;
    }
}
