import assert from "node:assert/strict";
import test from "node:test";

import {
  androidKeyringBridgeSource,
  hardenAndroidManifest,
  initializeAndroidKeyringInMainActivity,
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
