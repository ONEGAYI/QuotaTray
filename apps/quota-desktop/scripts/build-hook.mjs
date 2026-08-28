import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

/**
 * Tauri 构建钩子的目标映射。仅 Windows ARM64 需要显式交叉 target；
 * x64 与非 Windows 原生构建沿用 Cargo 宿主默认目录。
 */
export function cargoTargetFor(platform, arch) {
  const normalizedPlatform = platform?.toLowerCase();
  const normalizedArch = arch?.toLowerCase();
  if (["windows", "win32"].includes(normalizedPlatform) && normalizedArch === "aarch64") {
    return "aarch64-pc-windows-msvc";
  }
  return null;
}

export function pnpmInvocation(platform) {
  return platform === "win32"
    ? { command: "pnpm build", args: [], shell: true }
    : { command: "pnpm", args: ["build"], shell: false };
}

function run(command, args, cwd, shell = false) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit", shell });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} 失败（退出码 ${result.status}）`);
  }
}

export function main() {
  const appRoot = fileURLToPath(new URL("..", import.meta.url));
  const target = cargoTargetFor(process.env.TAURI_ENV_PLATFORM, process.env.TAURI_ENV_ARCH);
  const cargoArgs = ["build", "-p", "quota-cli", "--release"];
  if (target) cargoArgs.push("--target", target);

  run("cargo", cargoArgs, appRoot);
  const pnpm = pnpmInvocation(process.platform);
  run(pnpm.command, pnpm.args, appRoot, pnpm.shell);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
