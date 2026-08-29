//! Android APK 安装引导 JNI 桥：Rust → `ApkInstallHelper`（Kotlin）。
//!
//! 职责边界：本模块只做桥接——输入校验（content:// 前缀）与错误文案
//! 归命令层（commands.rs），Intent 构造与主线程派发归 Kotlin helper
//! （`android-post-init.mjs` 注入，行为由其契约测试锁定）。
//!
//! 类查找必须走 `Context.getClassLoader().loadClass`：native attach 的
//! 线程上 `find_class` 只挂 boot classloader，应用 dex 类（helper 在
//! `com.quotatray.android` 包）必须经应用 ClassLoader 加载——keyring 桥
//! 先例只调系统类（Keystore/SharedPreferences），无此约束，不可照抄。

use jni::{
    JNIEnv, JavaVM,
    objects::{JClass, JObject, JValue},
};

/// 注入的 Kotlin helper 全限定类名（与 android-post-init.mjs 生成一致）。
const HELPER_CLASS: &str = "com.quotatray.android.ApkInstallHelper";

/// 以系统安装器打开 SAF 保存的 APK（`content://` URI）。
///
/// - `Ok(true)`：已派发安装器（用户侧随后看到系统安装确认页）；
/// - `Ok(false)`：系统无安装器可处理（裁剪 ROM），调用方降级为手动引导；
/// - `Err`：桥本身故障（ndk-context 未初始化 / JNI 异常），确定性错误。
///
/// 「安装未知应用」未授权时系统以 toast 弹回（不报错）——授权状态
/// 程序化不可知（PackageManager/AppOps 查询均需先声明自安装权限，
/// 本项目永不声明，API 36 实证 2026-08-29：AppOps allow 亦被弹回、
/// 授权页开关置灰），前端提示行以文件管理器为主出路，并以
/// [`open_install_consent`] 作为旧版系统的授权页次出路。
pub fn open_apk(uri: &str) -> Result<bool, String> {
    with_helper_class(|env, context, helper| {
        let j_uri = env
            .new_string(uri)
            .map_err(|e| format!("构造 URI 字符串失败：{e}"))?;
        let ret = env
            .call_static_method(
                helper,
                "openApk",
                "(Landroid/content/Context;Ljava/lang/String;)Z",
                &[JValue::Object(context), JValue::Object(&j_uri)],
            )
            .map_err(|e| format!("调用 ApkInstallHelper.openApk 失败：{e}"))?;
        ret.z().map_err(|e| format!("读取安装器调用结果失败：{e}"))
    })
}

/// 打开本应用的「允许安装未知应用」系统授权页（用户开关，非权限声明）。
///
/// - `Ok(true)`：授权页已派发（API 26+ 均有该 Settings action）；
/// - `Ok(false)`：API 26 以下系统无此页面，调用方降级为纯文案引导；
/// - `Err`：桥本身故障，确定性错误。
pub fn open_install_consent() -> Result<bool, String> {
    with_helper_class(|env, context, helper| {
        env.call_static_method(
            helper,
            "openInstallConsent",
            "(Landroid/content/Context;)Z",
            &[JValue::Object(context)],
        )
        .map_err(|e| format!("调用 ApkInstallHelper.openInstallConsent 失败：{e}"))?
        .z()
        .map_err(|e| format!("读取授权页调用结果失败：{e}"))
    })
}

/// 公共桥：ndk-context 取 VM/Context → attach 线程 → 经应用 ClassLoader
/// 加载 helper 类 → 以 `(env, context, helper)` 执行回调。
fn with_helper_class<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce(&mut JNIEnv, &JObject, &JClass) -> Result<T, String>,
{
    // ndk-context 由 MainActivity.onCreate 的 Keyring 桥初始化（启动序保证）
    let ctx = ndk_context::android_context();
    let vm = ctx.vm().cast();
    // SAFETY: vm 指针来自 JVM 全局（ndk-context 初始化时缓存），进程内有效
    let java_vm = unsafe { JavaVM::from_raw(vm) }.map_err(|e| format!("JNI VM 获取失败：{e}"))?;
    let mut env = java_vm
        .attach_current_thread()
        .map_err(|e| format!("JNI 线程附加失败：{e}"))?;

    // context() 指向 ndk-context 持有的 GlobalRef，进程内稳定（keyring 同款用法）
    // SAFETY: 见上；raw 指针即该 GlobalRef 的底层引用，非线程局部引用
    let j_context = unsafe { JObject::from_raw(ctx.context() as jni::sys::jobject) };

    // 经应用 ClassLoader 加载 helper 类（见模块注释）
    let loader = env
        .call_method(
            &j_context,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("获取应用 ClassLoader 失败：{e}"))?;
    let class_name = env
        .new_string(HELPER_CLASS)
        .map_err(|e| format!("构造类名失败：{e}"))?;
    let helper_obj = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&class_name)],
        )
        .and_then(|v| v.l())
        .map_err(|e| format!("加载 {HELPER_CLASS} 失败：{e}"))?;
    // loadClass 返回的 java.lang.Class 实例即静态调用的类对象；
    // into_raw 移交 JObject 所有权（jni 0.21 的 JObject/JClass 无 Drop，
    // 局部引用靠 AttachGuard detach 时随线程引用表整体释放，每次调用
    // 量级 <16 个，无泄漏）。
    // SAFETY: raw 指针来自同一线程的有效局部引用
    let helper = unsafe { JClass::from_raw(helper_obj.into_raw()) };
    let result = f(&mut env, &j_context, &helper);
    // jni crate 的 call_* 出错不清理 pending exception（官方文档明示由
    // 调用方负责）；带着未清异常 detach 违反 JNI 规范，CheckJNI 下可
    // abort——骨架层统一收口：回调失败即清理。
    if result.is_err() {
        let _ = env.exception_clear();
    }
    result
}

#[cfg(test)]
mod tests {
    /// 编译期契约：桥函数签名保持命令层友好的形状（真实 JNI 路径仅
    /// 真机/模拟器可验）。
    #[test]
    fn bridge_signatures_are_command_friendly() {
        let _open: fn(&str) -> Result<bool, String> = super::open_apk;
        let _consent: fn() -> Result<bool, String> = super::open_install_consent;
    }
}
