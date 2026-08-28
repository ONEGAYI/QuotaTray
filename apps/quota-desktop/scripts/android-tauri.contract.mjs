import assert from "node:assert/strict";
import test from "node:test";

import {
  androidBuildEnvironment,
  androidHostTag,
  parseJavaMajor,
} from "./android-tauri.mjs";

test("Android NDK host tag 覆盖 CI 与本地开发系统", () => {
  assert.equal(androidHostTag("win32"), "windows-x86_64");
  assert.equal(androidHostTag("linux"), "linux-x86_64");
  assert.equal(androidHostTag("darwin"), "darwin-x86_64");
  assert.throws(() => androidHostTag("freebsd"), /不支持/);
});

test("Android 构建环境从 NDK_HOME 推导全部 Bindgen sysroot", () => {
  const environment = androidBuildEnvironment(
    { NDK_HOME: "D:\\Android\\ndk\\27.2.12479018" },
    "win32",
  );
  const expected =
    "--sysroot=D:/Android/ndk/27.2.12479018/toolchains/llvm/prebuilt/windows-x86_64/sysroot";
  assert.equal(
    environment.BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android,
    expected,
  );
  assert.equal(
    environment.BINDGEN_EXTRA_CLANG_ARGS_x86_64_linux_android,
    expected,
  );
  assert.equal(
    environment.BINDGEN_EXTRA_CLANG_ARGS_armv7_linux_androideabi,
    expected,
  );
  assert.equal(
    environment.BINDGEN_EXTRA_CLANG_ARGS_i686_linux_android,
    expected,
  );
});

test("Android 构建环境拒绝缺失 NDK_HOME", () => {
  assert.throws(() => androidBuildEnvironment({}, "win32"), /NDK_HOME/);
});

test("JDK release 版本解析锁定 Java 17 门禁", () => {
  assert.equal(parseJavaMajor('JAVA_VERSION="17.0.12"'), 17);
  assert.equal(parseJavaMajor('JAVA_VERSION="25"'), 25);
  assert.throws(() => parseJavaMajor("IMPLEMENTOR=Adoptium"), /JAVA_VERSION/);
});
