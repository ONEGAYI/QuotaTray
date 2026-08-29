import assert from "node:assert/strict";
import test from "node:test";

import {
  androidKeyringBridgeSource,
  hardenAndroidManifest,
  initializeAndroidKeyringInMainActivity,
  injectAndroidReleaseSigning,
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
