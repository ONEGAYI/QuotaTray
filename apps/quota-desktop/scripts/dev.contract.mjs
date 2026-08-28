import assert from "node:assert/strict";
import net from "node:net";
import test from "node:test";

import {
  DEFAULT_PORT_BASE,
  DEFAULT_PORT_SPAN,
  isPortFree,
  parsePortConfig,
  resolveDevPort,
  tauriDevConfig,
} from "./dev.mjs";

function listenOn(host) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.listen(0, host, () => resolve(server));
  });
}

function closeServer(server) {
  return new Promise((resolve) => server.close(resolve));
}

test("端口配置缺省时为 1420 起顺延 500 个端口", () => {
  assert.deepEqual(parsePortConfig({}), { base: DEFAULT_PORT_BASE, span: DEFAULT_PORT_SPAN });
  assert.equal(DEFAULT_PORT_BASE, 1420);
});

test("端口配置读取 QUOTA_DEV_PORT_BASE / QUOTA_DEV_PORT_SPAN 覆盖", () => {
  assert.deepEqual(parsePortConfig({ QUOTA_DEV_PORT_BASE: "1500", QUOTA_DEV_PORT_SPAN: "20" }), {
    base: 1500,
    span: 20,
  });
});

test("端口配置对显式提供的非法值快速失败而非静默回退", () => {
  for (const bad of ["abc", "-1", "0", "3.5", ""]) {
    assert.throws(() => parsePortConfig({ QUOTA_DEV_PORT_BASE: bad }));
    assert.throws(() => parsePortConfig({ QUOTA_DEV_PORT_SPAN: bad }));
  }
});

test("端口配置拒绝使候选越过 65535 的组合", () => {
  assert.throws(() => parsePortConfig({ QUOTA_DEV_PORT_BASE: "65535" }));
  assert.throws(() => parsePortConfig({ QUOTA_DEV_PORT_BASE: "60000", QUOTA_DEV_PORT_SPAN: "6000" }));
  assert.deepEqual(parsePortConfig({ QUOTA_DEV_PORT_BASE: "60000", QUOTA_DEV_PORT_SPAN: "5535" }), {
    base: 60000,
    span: 5535,
  });
});

test("tauriDevConfig 仅覆盖 build.devUrl 且指向给定端口", () => {
  assert.deepEqual(tauriDevConfig(1435), { build: { devUrl: "http://localhost:1435" } });
});

test("resolveDevPort 返回探测可用的第一个端口", async () => {
  const busy = new Set([1420, 1421, 1422]);
  const probe = async (port) => !busy.has(port);
  assert.equal(await resolveDevPort(1420, 500, probe), 1423);
  assert.equal(await resolveDevPort(1500, 500, probe), 1500);
});

test("resolveDevPort 在整个顺延窗口耗尽时抛错并给出排查指引", async () => {
  const probe = async () => false;
  await assert.rejects(() => resolveDevPort(1420, 3, probe), /顺延|1420/);
});

test("isPortFree 在 IPv4 被占时即判不可用，释放后恢复 true", async () => {
  const server = await listenOn("127.0.0.1");
  const port = server.address().port;
  try {
    assert.equal(await isPortFree(port), false);
  } finally {
    await closeServer(server);
  }
  assert.equal(await isPortFree(port), true);
});

test("isPortFree 在仅 IPv6 被占时即判不可用", async () => {
  const server = await listenOn("::1");
  const port = server.address().port;
  try {
    assert.equal(await isPortFree(port), false);
  } finally {
    await closeServer(server);
  }
});

test("resolveDevPort 真实探测时跳过被占端口并落在双栈可绑端口上", async () => {
  const server = await listenOn("127.0.0.1");
  const port = server.address().port;
  try {
    const picked = await resolveDevPort(port, 500);
    assert.ok(picked > port && picked <= port + 500, `picked=${picked} base=${port}`);
    assert.equal(await isPortFree(picked), true);
  } finally {
    await closeServer(server);
  }
});
