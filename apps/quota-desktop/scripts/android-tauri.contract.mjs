import assert from "node:assert/strict";
import test from "node:test";

import {
  androidBuildArgs,
  androidBuildEnvironment,
  androidHostTag,
  cargoWorkspaceVersion,
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

test("workspace Cargo.toml 版本提取供 versionCode 派生注入", () => {
  const toml = [
    "[workspace.package]",
    'version = "0.8.1"',
    "edition = " + '"2021"',
    "",
    "[workspace.dependencies]",
    'serde = "1"',
  ].join("\n");
  assert.equal(cargoWorkspaceVersion(toml), "0.8.1");
  // version 必须取自 [workspace.package] 段，不能匹配到依赖段
  const noSection = '[workspace.dependencies]\nversion_tomb = "x"\n';
  assert.throws(() => cargoWorkspaceVersion(noSection), /workspace\.package/);
});

test("版本提取免疫注释伪段、行内注释与 version.workspace 写法", () => {
  // 注释里引用段名不得被当作伪段头（否则吞掉真段 version 或静默取错值）
  const withComment = [
    "# 版本段见 [workspace.package]",
    "[workspace.package]",
    'version = "0.8.1"',
  ].join("\n");
  assert.equal(cargoWorkspaceVersion(withComment), "0.8.1");
  const inlineComment = '[workspace.package] # 根版本\nversion = "1.2.3"\n';
  assert.equal(cargoWorkspaceVersion(inlineComment), "1.2.3");
  // ^version\s*= 不得吃到 version.workspace（点号挡住等号），此处应报缺字段而非误取
  const memberStyle = "[workspace.package]\nversion.workspace = true\n";
  assert.throws(() => cargoWorkspaceVersion(memberStyle), /version 字段/);
});

test("Android 构建参数注入 version 配置驱动 tauri versionCode 派生", () => {
  // tauri.conf.json 无 version 字段时 tauri-cli 不生成 tauri.properties，
  // Android versionCode 恒为 gradle fallback 1；经 --config 注入 workspace
  // 版本后由 tauri 原生公式派生（major*1000000+minor*1000+patch）
  assert.deepEqual(androidBuildArgs(["--ci", "--apk"], "0.8.1"), [
    "--config",
    '{"version":"0.8.1"}',
    "--ci",
    "--apk",
  ]);
  assert.deepEqual(androidBuildArgs([], "0.9.0"), [
    "--config",
    '{"version":"0.9.0"}',
  ]);
});
