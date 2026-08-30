import assert from "node:assert/strict";
import test from "node:test";

import {
  androidApkInstallHelperSource,
  androidBackgroundWorkerSource,
  androidKeyringBridgeSource,
  androidNotificationHelperSource,
  hardenAndroidManifest,
  initializeAndroidKeyringInMainActivity,
  injectAndroidReleaseSigning,
  injectAndroidWorkManagerDependency,
  proguardKeepRulesSource,
} from "./android-post-init.mjs";

test("Android manifest 关闭系统备份且重复执行幂等", () => {
  const source = '<manifest xmlns:android="http://schemas.android.com/apk/res/android"><application android:allowBackup="true" android:theme="@style/AppTheme" /></manifest>';
  const hardened = hardenAndroidManifest(source);
  assert.match(hardened, /android:allowBackup="false"/);
  assert.match(hardened, /android:fullBackupContent="false"/);
  assert.equal(hardened.match(/android:allowBackup=/g)?.length, 1);
  assert.equal(hardenAndroidManifest(hardened), hardened);
});

test("Android Keyring 桥接类从 Tauri 主库导出 JNI 初始化入口", () => {
  const source = androidKeyringBridgeSource();
  assert.match(source, /package io\.crates\.keyring/);
  assert.match(source, /System\.loadLibrary\("quota_desktop_lib"\)/);
  assert.match(source, /external fun initializeNdkContext\(context: Context\)/);
  assert.doesNotMatch(source, /@JvmStatic/);
});

test("MainActivity 在进入 Tauri 生命周期前初始化 Android Keyring 且幂等", () => {
  const source = `package com.quotatray.android

import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }
}
`;
  const initialized = initializeAndroidKeyringInMainActivity(source);
  assert.match(initialized, /import io\.crates\.keyring\.Keyring/);
  assert.ok(
    initialized.indexOf("Keyring.initializeNdkContext(applicationContext)") <
      initialized.indexOf("super.onCreate(savedInstanceState)"),
  );
  assert.equal(initializeAndroidKeyringInMainActivity(initialized), initialized);
});

test("MainActivity 模板锚点漂移时拒绝生成缺少 import 的工程", () => {
  const source = `package com.quotatray.android

import android.os.Bundle

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
  }
}
`;
  assert.throws(
    () => initializeAndroidKeyringInMainActivity(source),
    /缺少 enableEdgeToEdge import 锚点/,
  );
});

function buildGradleKtsFixture() {
  return `import java.util.Properties

plugins {
    id("com.android.application")
}

android {
    compileSdk = 36
    namespace = "com.quotatray.android"
    defaultConfig {
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    buildTypes {
        getByName("debug") {
            isDebuggable = true
        }
        getByName("release") {
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.12.0")
}
`;
}

test("release 签名配置注入读取 keystore.properties 且幂等", () => {
  const injected = injectAndroidReleaseSigning(buildGradleKtsFixture());
  assert.match(injected, /signingConfigs \{/);
  assert.match(injected, /rootProject\.file\("keystore\.properties"\)/);
  assert.match(injected, /if \(keystorePropertiesFile\.exists\(\)\)/);
  // Properties.load(InputStream) 按 ISO-8859-1 解码，中文 keystore 路径会乱码导致
  // 签名文件找不到；必须以 UTF-8 Reader 读取（真实故障：2026-08-29 中文路径验收）
  assert.match(
    injected,
    /reader\(Charsets\.UTF_8\)\.use \{ keystoreProperties\.load\(it\) \}/,
  );
  assert.match(injected, /create\("release"\)/);
  assert.match(injected, /keyAlias = keystoreProperties\["keyAlias"\] as String/);
  assert.match(injected, /keyPassword = keystoreProperties\["keyPassword"\] as String/);
  assert.match(injected, /storeFile = file\(keystoreProperties\["storeFile"\] as String\)/);
  assert.match(
    injected,
    /storePassword = keystoreProperties\["storePassword"\] as String/,
  );
  assert.match(
    injected,
    /signingConfigs\.findByName\("release"\)\?\.let \{ signingConfig = it \}/,
  );
  assert.ok(
    injected.indexOf("signingConfigs {") < injected.indexOf("buildTypes {"),
  );
  // 挂载行必须落在 release 块体内（getByName("release") 行之后），而非 debug 块
  assert.ok(
    injected.indexOf('findByName("release")') >
      injected.indexOf('getByName("release")'),
  );
  assert.equal(injectAndroidReleaseSigning(injected), injected);
});

test("CRLF 模板注入保持 CRLF 行尾且幂等", () => {
  // 行尾正确性依赖 m 标志下 $ 视 \r 为行终止符的语义，此处以契约锁定防回退
  const crlf = buildGradleKtsFixture().replace(/\n/g, "\r\n");
  const injected = injectAndroidReleaseSigning(crlf);
  const block = injected.slice(
    injected.indexOf("    signingConfigs {"),
    injected.indexOf("    buildTypes {"),
  );
  assert.ok(!block.includes(" \n") || block.includes(" \r\n"));
  assert.match(block, /signingConfigs \{\r\n/);
  assert.equal(injectAndroidReleaseSigning(injected), injected);
});

test("上游模板自带 signingConfigs 时不误判为已注入", () => {
  const withForeign = buildGradleKtsFixture().replace(
    "    buildTypes {",
    '    signingConfigs {\n        create("debug")\n    }\n    buildTypes {',
  );
  // 幂等标记是 keystore.properties 特征串，模板原生 signingConfigs 不拦截注入
  const injected = injectAndroidReleaseSigning(withForeign);
  assert.match(injected, /rootProject\.file\("keystore\.properties"\)/);
});

test("build.gradle.kts 锚点漂移时拒绝静默生成无签名工程", () => {
  assert.throws(
    () => injectAndroidReleaseSigning("android {\n}\n"),
    /缺少 buildTypes 锚点/,
  );
  assert.throws(
    () =>
      injectAndroidReleaseSigning(
        'android {\n    buildTypes {\n        getByName("debug") {\n        }\n    }\n}\n',
      ),
    /缺少 getByName\("release"\) 锚点/,
  );
});

test("APK 安装引导 helper 以 ACTION_VIEW 交给系统安装器且不触碰自安装 API", () => {
  const source = androidApkInstallHelperSource();
  assert.match(source, /package com\.quotatray\.android/);
  // Rust 侧经 JNI 调用静态方法，必须 @JvmStatic 暴露稳定入口
  assert.match(source, /@JvmStatic/);
  assert.match(source, /fun openApk\(context: Context, uriString: String\): Boolean/);
  assert.match(source, /Intent\.ACTION_VIEW/);
  assert.match(source, /application\/vnd\.android\.package-archive/);
  // SAF 授予安装器临时读权 + 独立任务栈
  assert.match(source, /FLAG_GRANT_READ_URI_PERMISSION/);
  assert.match(source, /FLAG_ACTIVITY_NEW_TASK/);
  // startActivity 需主线程：Rust 命令线程发起，必须经主线程 Handler 派发。
  // 两个入口都要锁定（openInstallConsent 曾裸调绕过，审查 2026-08-29 抓出）
  const openApkBlock = source.slice(source.indexOf("fun openApk"), source.indexOf("fun openInstallConsent"));
  const consentBlock = source.slice(source.indexOf("fun openInstallConsent"));
  assert.match(openApkBlock, /Looper\.getMainLooper\(\)/);
  assert.match(consentBlock, /Looper\.getMainLooper\(\)/);
  assert.match(consentBlock, /ActivityNotFoundException/);
  // 系统无安装器时返回 false（前端降级手动引导），不得静默成功
  assert.match(source, /resolveActivity/);
  // 「安装未知应用」闸口：许可状态程序化不可知（PackageManager/AppOps
  // 查询均需先声明自安装权限，API 36 实证 SecurityException），不做预判
  // ——openApk 直接发安装请求，openInstallConsent 提供授权页次出路
  //（公开 Settings action，用户开关而非权限声明；API 26 引入，更低版本
  // 返回 false 交前端降级）。
  assert.match(source, /fun openInstallConsent\(context: Context\): Boolean/);
  assert.match(source, /ACTION_MANAGE_UNKNOWN_APP_SOURCES/);
  assert.match(source, /Build\.VERSION\.SDK_INT < 26/);
  // 插值必须精确匹配（可选反斜杠会放行 JS 转义回归：产出 \${...} 时
  // Kotlin 得到字面量不插值、运行时 URI 错误但编译通过）
  assert.match(source, /package:\$\{context\.packageName\}/);
  assert.doesNotMatch(source, /\\\$\{/);
  // 红线锁定：不出现应用自安装通道 API 与权限声明（Play 审核高危项）；
  // 负向后瞻放行 AppOps 常量形态（如历史方案回流可被捕获）
  assert.doesNotMatch(source, /installPackage\(|PackageInstaller/);
  assert.doesNotMatch(source, /(?<!OPSTR_)REQUEST_INSTALL/);
  assert.doesNotMatch(source, /canRequestPackageInstalls/);
});

test("R8 keep 规则保住仅由 Rust 反射加载的 helper（release minify 收缩防线）", () => {
  const source = proguardKeepRulesSource();
  assert.match(source, /-keep class com\.quotatray\.android\.ApkInstallHelper \{ \*; \}/);
  assert.match(source, /-keep class com\.quotatray\.android\.NotificationHelper \{ \*; \}/);
  assert.match(source, /-keep class com\.quotatray\.android\.Native \{ \*; \}/);
  assert.match(source, /-keep class com\.quotatray\.android\.BackgroundWorker \{ \*; \}/);
  assert.match(source, /-keep class com\.quotatray\.android\.BackgroundScheduler \{ \*; \}/);
  // keep 规则自身不得越界放行其他类（最小化面）
  assert.equal(source.match(/-keep/g)?.length, 5);
});

test("后台刷新三件套：冷启动补调 ndk-context、渠道幂等建、通知直发不越权", () => {
  const source = androidBackgroundWorkerSource();
  assert.match(source, /package com\.quotatray\.android/);
  // Native 桥：独立 object（companion external fun 符号名有 $ 转义陷阱）
  // + loadLibrary 与 tauri 运行时加载同一 so
  assert.match(source, /object Native \{/);
  assert.match(source, /System\.loadLibrary\("quota_desktop_lib"\)/);
  assert.match(source, /external fun backgroundRefresh\(dataDir: String\): String/);
  // 冷启动（WorkManager 拉起已死进程）必须先补调 initializeNdkContext
  // （幂等），且先于 JNI 刷新调用——Rust 侧 vault 经 ndk-context 取 Context
  const doWork = source.slice(source.indexOf("override fun doWork"), source.indexOf("private fun dispatchNotifications"));
  // 存在性先于顺序：删掉 initializeNdkContext 后 indexOf 得 -1，
  // -1 < 正数使顺序断言弱通过
  assert.match(doWork, /Keyring\.initializeNdkContext/);
  assert.ok(
    doWork.indexOf("Keyring.initializeNdkContext") <
      doWork.indexOf("Native.backgroundRefresh"),
    "initializeNdkContext 必须先于 backgroundRefresh",
  );
  // Rust 侧 new_string 兜底仍失败会返回 null：Kotlin 必须显式判空，
  // 不得把 null 直接喂 JSONObject（NPE 兜底吞掉诊断线索——模拟器
  // 验收 2026-08-30 抓出后补防御与日志）
  assert.match(doWork, /json == null/);
  assert.match(doWork, /JNI 调用异常/);
  // dataDir 与 tauri app_data_dir 同源（PathPlugin 经 activity.dataDir
  // 解析）；filesDir 会错位一级目录，Worker 读不到 settings.json——
  // 负向断言防回归（审查 M1；注释中提及 filesDir 的说明文字不算）
  assert.match(source, /applicationContext\.dataDir\.absolutePath/);
  assert.doesNotMatch(source, /applicationContext\.filesDir/);
  // Worker 永不 retry：失败也 success（下个周期自然重试，Rust 侧已静默）
  assert.doesNotMatch(source, /Result\.retry/);
  // 通知直发：渠道幂等创建（API 26+ 守卫）+ 未授权整体跳过
  // （areNotificationsEnabled 覆盖 Android 13+ 运行时权限与更早系统开关）
  assert.match(source, /createNotificationChannel/);
  assert.match(source, /Build\.VERSION\.SDK_INT >= 26/);
  assert.match(source, /areNotificationsEnabled\(\)/);
  // 插值必须精确匹配（产出 \${...} 时 Kotlin 得到字面量不插值，
  // 运行时日志错误但编译通过）
  assert.match(source, /失败：\$\{e\.message\}/);
  assert.doesNotMatch(source, /\\\$\{/);
});

test("后台调度入口：UPDATE 策略、网络约束、15 分钟双保险与关停路径", () => {
  const source = androidBackgroundWorkerSource();
  const scheduler = source.slice(source.indexOf("object BackgroundScheduler"));
  // Rust 反射调用静态方法，必须 @JvmStatic 暴露稳定入口（签名
  // (Context, Boolean, Long) 与 background.rs 的 JNI 调用一致）
  assert.match(scheduler, /@JvmStatic/);
  assert.match(
    scheduler,
    /fun schedule\(context: Context, enabled: Boolean, intervalMinutes: Long\): Boolean/,
  );
  // 开关关 → 取消注册（后台任务不残留）
  assert.match(scheduler, /cancelUniqueWork/);
  // UPDATE 策略：间隔变更即时生效且保留周期对齐；spec 不变 no-op
  assert.match(scheduler, /ExistingPeriodicWorkPolicy\.UPDATE/);
  // 网络约束（余额查询需联网；不加 batteryNotLow——流量极小）
  assert.match(scheduler, /NetworkType\.CONNECTED/);
  // 15 分钟系统硬限双保险（Rust sanitize 已收口，此处防手改 settings.json）
  assert.match(scheduler, /coerceAtLeast\(15\)/);
  // 红线：不出现前台服务/精确闹钟/电池优化豁免（所有者定案与权限克制）
  assert.doesNotMatch(source, /FOREGROUND_SERVICE|setExact|SCHEDULE_EXACT_ALARM|REQUEST_IGNORE_BATTERY/);
});

test("WorkManager 依赖注入幂等且锚点漂移拒绝", () => {
  const fixture = buildGradleKtsFixture();
  const injected = injectAndroidWorkManagerDependency(fixture);
  assert.match(
    injected,
    /dependencies \{\n    implementation\("androidx\.work:work-runtime-ktx:2\.9\.1"\)/,
  );
  assert.equal(injectAndroidWorkManagerDependency(injected), injected, "已注入幂等");
  // 上游模板自带该依赖（坐标任意版本）时不重复注入
  const withForeign = fixture.replace(
    "dependencies {",
    'dependencies {\n    implementation("androidx.work:work-runtime-ktx:2.10.0")',
  );
  assert.equal(injectAndroidWorkManagerDependency(withForeign), withForeign);
  // 锚点漂移即抛错（dependencies 块被上游重构时不得静默跳过）
  assert.throws(
    () => injectAndroidWorkManagerDependency("android {\n}\n"),
    /缺少 dependencies 锚点/,
  );
  // CRLF 模板保持行尾一致
  const crlf = injectAndroidWorkManagerDependency(
    fixture.replace(/\n/g, "\r\n"),
  );
  assert.match(crlf, /dependencies \{\r\n    implementation/);
});

test("通知设置页 helper 跳系统授权页且不触碰通知权限声明", () => {
  const source = androidNotificationHelperSource();
  assert.match(source, /package com\.quotatray\.android/);
  // Rust 侧经 JNI 调用静态方法，必须 @JvmStatic 暴露稳定入口
  assert.match(source, /@JvmStatic/);
  assert.match(
    source,
    /fun openNotificationSettings\(context: Context\): Boolean/,
  );
  // 公开的系统设置页 action + 包名 extra（用户开关授权，非权限声明）
  assert.match(source, /ACTION_APP_NOTIFICATION_SETTINGS/);
  assert.match(source, /EXTRA_APP_PACKAGE/);
  assert.match(source, /context\.packageName/);
  // 独立任务栈
  assert.match(source, /FLAG_ACTIVITY_NEW_TASK/);
  // startActivity 需主线程：Rust 命令线程发起，必须经主线程 Handler 派发
  assert.match(source, /Looper\.getMainLooper\(\)/);
  assert.match(source, /ActivityNotFoundException/);
  // ACTION_APP_NOTIFICATION_SETTINGS 为 API 26 引入，低版本返回 false
  // 由前端降级为纯文案引导（本应用 minSdk 24）
  assert.match(source, /Build\.VERSION\.SDK_INT < 26/);
  // 插值必须精确匹配（产出 \${...} 时 Kotlin 得到字面量不插值，
  // 运行时日志错误但编译通过）
  assert.match(source, /缺失：\$\{e\.message\}/);
  assert.doesNotMatch(source, /\\\$\{/);
  // 红线锁定：不出现通知权限声明/绕过（POST_NOTIFICATIONS 由
  // tauri-plugin-notification 的 AAR manifest 经 gradle merger 自动带入，
  // helper 只负责引导用户到系统页手动授权）
  assert.doesNotMatch(source, /POST_NOTIFICATIONS/);
  assert.doesNotMatch(source, /requestPermissions\(/);
  assert.doesNotMatch(source, /NotificationChannel|NotificationCompat/);
});
