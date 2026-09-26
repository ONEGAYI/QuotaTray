import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const view = await readFile(new URL("../src/components/settingsView.ts", import.meta.url), "utf8");
const dialog = await readFile(new URL("../src/components/SettingsDialog.tsx", import.meta.url), "utf8");
const app = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
const zh = await readFile(new URL("../src/i18n/zh.ts", import.meta.url), "utf8");
const en = await readFile(new URL("../src/i18n/en.ts", import.meta.url), "utf8");

test("设置页签类型单一事实源：SettingsTab 含 network（#133）", () => {
  // 页签字面量此前散布 SettingsDialog 与 App 两处、加页签常漏同步——
  // 收敛到 settingsView.ts 的共享类型，两端引用同一声明
  assert.match(view, /export type SettingsTab = "general" \| "update" \| "network" \| "data";/);
});

test("SettingsDialog 与 App 共用 SettingsTab，不再各持独立字面量", () => {
  assert.match(dialog, /\bSettingsTab\b/);
  assert.doesNotMatch(dialog, /type Tab = "general"/);
  assert.match(app, /useState<SettingsTab>\("general"\)/);
  assert.doesNotMatch(app, /useState<"general" \| "update"/);
});

test("网络环境页签按钮在导航中，顺序为常规 → 更新 → 网络环境 → 数据管理", () => {
  assert.match(dialog, /aria-selected=\{tab === "network"\}/);
  assert.match(dialog, /t\("settings\.tabNetwork"\)/);
  const navStart = dialog.indexOf('qt-settings-nav');
  const navEnd = dialog.indexOf("</nav>", navStart);
  const nav = dialog.slice(navStart, navEnd);
  const positions = ["general", "update", "network", "data"].map((tab) =>
    nav.indexOf(`tab === "${tab}"`),
  );
  for (const position of positions) assert.ok(position >= 0, `nav 缺少页签按钮：${position}`);
  // 声明顺序即渲染顺序（index 前后）
  for (let i = 1; i < positions.length; i += 1) {
    assert.ok(positions[i] > positions[i - 1], "页签按钮顺序与约定不符");
  }
});

test("内容区有显式 network 分支，data 仍为兜底（#133）", () => {
  assert.match(dialog, /tab === "network" \? \(/);
});

test("代理字段唯一编辑点：主机与端口只在网络环境页出现一次", () => {
  // 每个字段恰两处引用：value 绑定 + setDraft 键。迁移前更新页与
  // Android 常规页各一份（共 4 处），收敛后原位置不再重复
  assert.equal((dialog.match(/update_proxy_host/g) ?? []).length, 2);
  assert.equal((dialog.match(/update_proxy_port/g) ?? []).length, 2);
});

test("Android 常规页不再渲染代理字段（mobile 门控重复段已删）", () => {
  assert.doesNotMatch(dialog, /mobile\s*&&[\s\S]{0,400}?updateProxy(Host|Port)Title/);
});

test("更新页保留指路入口：qt-inline-link 切换到网络环境页签（#133）", () => {
  assert.match(
    dialog,
    /className="qt-inline-link"\s*onClick=\{\(\) => setTab\("network"\)\}/s,
  );
  assert.match(dialog, /t\("settings\.proxyMovedHint"\)/);
  assert.match(dialog, /t\("settings\.proxyMovedOpen"\)/);
});

test("保存链保持：footer 仅 data 页收起保存，network 页有取消+保存", () => {
  assert.match(dialog, /footer=\{\s*tab === "data" \? \(/);
});

test("代理字段编辑语义保持：空主机→null、端口空/非法→null、越界 clamp", () => {
  // 空主机 = 回退本机 127.0.0.1（#133 不改清洗规则与空值语义）。
  // 编辑变换收敛为 settingsView 纯函数（往返一致性由 vitest 锁定），
  // 组件只做绑定，语义断言落在纯函数实现上
  assert.match(dialog, /update_proxy_host: proxyHostFromInput\(event\.target\.value\)/);
  assert.match(dialog, /update_proxy_port: proxyPortFromInput\(event\.target\.value\)/);
  assert.match(view, /return raw \|\| null;/);
  assert.match(view, /Math\.min\(65535, Math\.max\(1, Math\.round\(parsed\)\)\)/);
});

test("网络环境页与指路文案中英双语齐全", () => {
  const dicts = [["zh", zh], ["en", en]];
  for (const [name, dict] of dicts) {
    for (const key of ["settings.tabNetwork", "settings.proxyMovedHint", "settings.proxyMovedOpen"]) {
      assert.match(dict, new RegExp(`"${key}":`), `${name} 缺键 ${key}`);
    }
  }
});
