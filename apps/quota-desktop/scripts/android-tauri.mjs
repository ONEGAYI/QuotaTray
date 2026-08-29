import { existsSync, readFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const BINDGEN_TARGETS = [
  "aarch64_linux_android",
  "x86_64_linux_android",
  "armv7_linux_androideabi",
  "i686_linux_android",
];

export function androidHostTag(platform = process.platform) {
  switch (platform) {
    case "win32":
      return "windows-x86_64";
    case "linux":
      return "linux-x86_64";
    case "darwin":
      return "darwin-x86_64";
    default:
      throw new Error(`不支持的 Android NDK 主机平台：${platform}`);
  }
}

export function androidBuildEnvironment(
  source = process.env,
  platform = process.platform,
) {
  if (!source.NDK_HOME) {
    throw new Error("Android 构建需要设置 NDK_HOME");
  }
  const ndkHome = source.NDK_HOME.replaceAll("\\", "/").replace(/\/$/, "");
  const sysroot = `${ndkHome}/toolchains/llvm/prebuilt/${androidHostTag(platform)}/sysroot`;
  const environment = { ...source };
  for (const target of BINDGEN_TARGETS) {
    environment[`BINDGEN_EXTRA_CLANG_ARGS_${target}`] = `--sysroot=${sysroot}`;
  }
  return environment;
}

export function parseJavaMajor(releaseSource) {
  const match = releaseSource.match(/^JAVA_VERSION="(\d+)(?:\.|"|$)/m);
  if (!match) throw new Error("JDK release 文件缺少 JAVA_VERSION");
  return Number.parseInt(match[1], 10);
}

export function cargoWorkspaceVersion(tomlSource) {
  const section = tomlSource.match(/\[workspace\.package\][\s\S]*?(?=\n\[|$)/);
  if (!section) throw new Error("Cargo.toml 缺少 [workspace.package] 段");
  const version = section[0].match(/^version\s*=\s*"([^"]+)"/m);
  if (!version) throw new Error("[workspace.package] 缺少 version 字段");
  return version[1];
}

export function androidBuildArgs(rawArgs, version) {
  // tauri.conf.json 不写 version（crate 继承 workspace），而 tauri-cli 仅在配置
  // 显式含 version 时才生成 tauri.properties 派生 Android versionCode；经
  // --config 注入 workspace 版本，恢复原生派生（major*1000000+minor*1000+patch）
  return ["--config", `{"version":"${version}"}`, ...rawArgs];
}

function assertAndroidToolchain(environment) {
  if (!environment.JAVA_HOME) {
    throw new Error("Android 构建需要设置 JAVA_HOME，并使用 JDK 17");
  }
  const javaRelease = join(environment.JAVA_HOME, "release");
  if (!existsSync(javaRelease)) {
    throw new Error(`JAVA_HOME 不是有效 JDK：${environment.JAVA_HOME}`);
  }
  const javaMajor = parseJavaMajor(readFileSync(javaRelease, "utf8"));
  if (javaMajor !== 17) {
    throw new Error(`Android 构建锁定 JDK 17，当前 JAVA_HOME 为 JDK ${javaMajor}`);
  }
  const sysroot = environment.BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android.replace(
    "--sysroot=",
    "",
  );
  if (!existsSync(sysroot)) {
    throw new Error(`Android NDK sysroot 不存在：${sysroot}`);
  }
}

export async function main() {
  const [command, ...rawArgs] = process.argv.slice(2);
  if (command !== "build" && command !== "dev") {
    throw new Error("用法：node scripts/android-tauri.mjs <build|dev> [Tauri 参数]");
  }
  const environment = androidBuildEnvironment();
  assertAndroidToolchain(environment);
  const tauriCli = fileURLToPath(
    new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url),
  );
  if (!existsSync(tauriCli)) {
    throw new Error("未安装 @tauri-apps/cli，请先执行 pnpm install");
  }
  const workspaceCargoToml = fileURLToPath(
    new URL("../../../Cargo.toml", import.meta.url),
  );
  const version = cargoWorkspaceVersion(
    readFileSync(workspaceCargoToml, "utf8"),
  );
  const args = androidBuildArgs(
    rawArgs[0] === "--" ? rawArgs.slice(1) : rawArgs,
    version,
  );
  const exitCode = await new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      [tauriCli, "android", command, ...args],
      { env: environment, stdio: "inherit" },
    );
    child.once("error", reject);
    child.once("exit", (code) => resolve(code ?? 1));
  });
  process.exitCode = exitCode;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
