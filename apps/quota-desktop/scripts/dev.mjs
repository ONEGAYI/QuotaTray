import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import net from "node:net";
import { fileURLToPath, pathToFileURL } from "node:url";

// dev 端口动态避让：WinNAT/Hyper-V 会动态圈占连续端口段（v4/v6 独立、段宽可达数百），
// 落入段内 bind 报 EACCES；程序真实占用报 EADDRINUSE。两者都靠试绑探测顺延避让。
export const DEFAULT_PORT_BASE = 1420;
export const DEFAULT_PORT_SPAN = 500;

const POSITIVE_INT = /^\d+$/;

function parsePositiveInt(value, name) {
  if (!POSITIVE_INT.test(value) || Number(value) < 1) {
    throw new Error(`${name} 必须是正整数，当前值：${value}`);
  }
  return Number(value);
}

export function parsePortConfig(env = process.env) {
  const base =
    env.QUOTA_DEV_PORT_BASE === undefined
      ? DEFAULT_PORT_BASE
      : parsePositiveInt(env.QUOTA_DEV_PORT_BASE, "QUOTA_DEV_PORT_BASE");
  const span =
    env.QUOTA_DEV_PORT_SPAN === undefined
      ? DEFAULT_PORT_SPAN
      : parsePositiveInt(env.QUOTA_DEV_PORT_SPAN, "QUOTA_DEV_PORT_SPAN");
  if (base > 65535 || base + span > 65535) {
    throw new Error(`端口候选越界：base=${base} span=${span}，需满足 base+span ≤ 65535`);
  }
  return { base, span };
}

export function tauriDevConfig(port) {
  return { build: { devUrl: `http://localhost:${port}` } };
}

function listenOnce(host, port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.once("error", () => resolve(false));
    server.once("listening", () => server.close(() => resolve(true)));
    server.listen(port, host);
  });
}

// vite 绑 localhost 时走 IPv4 还是 IPv6 取决于 Node 解析顺序，且两栈排除段独立漂移，
// 因此双栈都可绑定才判可用——宁可无谓顺延，不选单栈可绑的端口。
export async function isPortFree(port) {
  if (!(await listenOnce("127.0.0.1", port))) return false;
  return listenOnce("::1", port);
}

export async function resolveDevPort(base, span, probe = isPortFree) {
  for (let offset = 0; offset <= span; offset += 1) {
    const candidate = base + offset;
    if (await probe(candidate)) return candidate;
  }
  throw new Error(
    `${base} 起顺延 ${span} 个端口内无可绑定端口（被占用或落入 WinNAT 排除段）。` +
      `可用 netsh interface ipv4 show excludedportrange protocol=tcp 排查，` +
      `或调大 QUOTA_DEV_PORT_SPAN / 更换 QUOTA_DEV_PORT_BASE 后重试。`,
  );
}

export async function main() {
  const { base, span } = parsePortConfig();
  const port = await resolveDevPort(base, span);
  const offset = port - base;
  console.log(`[dev] dev server 端口 ${port}${offset > 0 ? `（自 ${base} 顺延 ${offset}）` : ""}`);
  const tauriCli = fileURLToPath(
    new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url),
  );
  if (!existsSync(tauriCli)) {
    throw new Error("未安装 @tauri-apps/cli，请先执行 pnpm install");
  }
  const exitCode = await new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      [tauriCli, "dev", "--config", JSON.stringify(tauriDevConfig(port))],
      { env: { ...process.env, QUOTA_DEV_PORT: String(port) }, stdio: "inherit" },
    );
    child.once("error", reject);
    child.once("exit", (code) => resolve(code ?? 1));
  });
  process.exitCode = exitCode;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
