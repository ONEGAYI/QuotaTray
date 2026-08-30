import assert from "node:assert/strict";
import fs from "node:fs";
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

test("tauri.conf.json 图标列表含 PNG 源（Android launcher 图标防回归）", () => {
  // Android launcher 图标由 tauri android init 从 bundle.icon 里的 PNG 生成各
  // 密度 mipmap；列表只剩 .ico 时 init 无 PNG 源可用，gen 模板回落 Tauri
  // 默认双弧图标（2026-08-30 v0.8.4 及此前安装包实证）。断言至少一张 PNG
  // 且最大边 ≥512（xxxhdpi adaptive foreground 432px 的缩放源，再小会上
  // 采样发虚）。
  const conf = JSON.parse(
    fs.readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
  );
  const pngs = conf.bundle.icon.filter((entry) => entry.endsWith(".png"));
  assert.ok(pngs.length > 0, "bundle.icon 必须包含 PNG（Android mipmap 生成源）");
  let maxWidth = 0;
  for (const rel of pngs) {
    const buf = fs.readFileSync(new URL(`../src-tauri/${rel}`, import.meta.url));
    maxWidth = Math.max(maxWidth, buf.readUInt32BE(16)); // PNG IHDR 宽度，大端
  }
  assert.ok(
    maxWidth >= 512,
    `最大 PNG 源须 ≥512px（adaptive foreground 缩放源），实际 ${maxWidth}px`,
  );
});

test("android:init 链包含品牌图标生成步骤（launcher 回落默认防回归）", () => {
  // tauri android init 生成的是带 Tauri 默认图标的模板工程，品牌 launcher
  // 图标（含 adaptive XML 与各密度 mipmap）必须由 `tauri icon` 以品牌
  // master 源图（src/assets/brand-mark.png，见 icons/manifest.json 的
  // export 约定）重写——2026-08-30 前链路缺此步骤，v0.8.4 及此前全部
  // Android 安装包 launcher 回落默认双弧图标。断言 init 链在模板生成后、
  // post-init 注入前执行 icon 且源为 master 品牌图。
  const pkg = JSON.parse(
    fs.readFileSync(new URL("../package.json", import.meta.url), "utf8"),
  );
  const init = pkg.scripts["android:init"];
  assert.ok(init.includes("tauri android init"), "init 链须含模板生成");
  const iconAt = init.indexOf("tauri icon src/assets/brand-mark.png");
  assert.ok(iconAt > 0, "init 链须以品牌 master 源图执行 tauri icon");
  assert.ok(
    iconAt > init.indexOf("tauri android init") &&
      iconAt < init.indexOf("android-post-init"),
    "icon 须位于模板生成之后、post-init 注入之前",
  );
});
