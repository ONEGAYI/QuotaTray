import assert from "node:assert/strict";
import test from "node:test";

import { hardenAndroidManifest } from "./android-post-init.mjs";

test("Android manifest 关闭系统备份且重复执行幂等", () => {
  const source = '<manifest xmlns:android="http://schemas.android.com/apk/res/android"><application android:allowBackup="true" android:theme="@style/AppTheme" /></manifest>';
  const hardened = hardenAndroidManifest(source);
  assert.match(hardened, /android:allowBackup="false"/);
  assert.match(hardened, /android:fullBackupContent="false"/);
  assert.equal(hardened.match(/android:allowBackup=/g)?.length, 1);
  assert.equal(hardenAndroidManifest(hardened), hardened);
});
