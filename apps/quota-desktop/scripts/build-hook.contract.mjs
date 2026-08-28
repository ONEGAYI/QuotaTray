import assert from "node:assert/strict";
import test from "node:test";

import { cargoTargetFor, pnpmInvocation } from "./build-hook.mjs";

test("Windows ARM64 构建钩子把 CLI 指向同一目标三元组", () => {
  assert.equal(cargoTargetFor("windows", "aarch64"), "aarch64-pc-windows-msvc");
});

test("Windows x64 与非 Windows 原生构建不额外指定目标", () => {
  assert.equal(cargoTargetFor("windows", "x86_64"), null);
  assert.equal(cargoTargetFor("linux", "aarch64"), null);
});

test("Windows 通过 shell 启动 pnpm，规避 Node 24 直接执行 cmd 的 EINVAL", () => {
  assert.deepEqual(pnpmInvocation("win32"), { command: "pnpm build", args: [], shell: true });
  assert.deepEqual(pnpmInvocation("linux"), { command: "pnpm", args: ["build"], shell: false });
});
