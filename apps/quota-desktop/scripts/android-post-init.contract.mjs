import assert from "node:assert/strict";
import test from "node:test";

import {
  androidApkInstallHelperSource,
  androidKeyringBridgeSource,
  hardenAndroidManifest,
  initializeAndroidKeyringInMainActivity,
  injectAndroidReleaseSigning,
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
  // keep 规则自身不得越界放行其他类（最小化面）
  assert.equal(source.match(/-keep/g)?.length, 1);
});
