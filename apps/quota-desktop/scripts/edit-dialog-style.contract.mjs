import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const css = await readFile(new URL("../src/index.css", import.meta.url), "utf8");
const editDialog = await readFile(
  new URL("../src/components/EditDialog.tsx", import.meta.url),
  "utf8",
);
const zh = await readFile(new URL("../src/i18n/zh.ts", import.meta.url), "utf8");
const en = await readFile(new URL("../src/i18n/en.ts", import.meta.url), "utf8");

test("模板与脚本的 CodeMirror 编辑器跟随明暗主题", () => {
  assert.match(editDialog, /import \{ useTheme \} from "\.\.\/theme";/);
  const themed = editDialog.match(/theme=\{theme\}/g) ?? [];
  assert.equal(themed.length, 2, "模板与脚本两处 CodeMirror 都应传 theme={theme}");
});

test("allowInsecure 复选框内联于脚本操作行而非独立字段行", () => {
  const scriptForm = editDialog.slice(editDialog.indexOf("function ScriptForm"));
  assert.match(
    scriptForm,
    /qt-template-actions[\s\S]*?qt-check-inline/,
    "复选框应位于 qt-template-actions 操作行内（AI 调试按钮之后）",
  );
  assert.match(
    scriptForm,
    /<label className="qt-check-inline">[\s\S]*?edit\.allowInsecure[\s\S]*?<\/label>/,
  );
  assert.doesNotMatch(
    scriptForm,
    /<label className="qt-field">\s*<span[^>]*>\{t\("edit\.allowInsecure"\)\}/,
    "不应再用 qt-field 独立字段行承载该复选框",
  );
});

test("行内复选框样式走令牌且移动端满足 44px 命中区（T-005/T-010）", () => {
  assert.match(css, /\.qt-check-inline\s*\{[^}]*display:\s*inline-flex;/s);
  assert.match(css, /\.qt-check-inline input\s*\{[^}]*accent-color:\s*var\(--qt-accent\);/s);
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-check-inline\s*\{[^}]*min-height:\s*44px;/s,
  );
});

test("脚本形态效仿模板二级子页分栏", () => {
  // 二级子页状态与模板同构，默认先填运营商信息
  assert.match(editDialog, /type ScriptSub = "provider" \| "script";/);
  assert.match(editDialog, /useState<ScriptSub>\("provider"\)/);
  // script tab 渲染子页切换控件（SegmentedControl），文案键双语齐备
  assert.match(editDialog, /tab === "script" && \(\s*<div className="qt-edit-subtabs">/);
  assert.match(editDialog, /value=\{scriptSub\}[\s\S]*?edit\.subScript/);
  assert.match(zh, /"edit\.subScript":/);
  assert.match(en, /"edit\.subScript":/);
  // script 分支：provider 子页（CSS 隐藏不卸载）收纳 baseUrl 等基础字段，
  // script 子页条件渲染 ScriptForm（卸载，防 CodeMirror 隐藏容器测量问题）
  const scriptBranch = editDialog.slice(editDialog.indexOf('tab === "script" ? ('));
  assert.match(
    scriptBranch,
    /qt-edit-subpage[^"]*"[\s\S]*?baseUrlField[\s\S]*?scriptSub === "script" &&/,
  );
  // ScriptForm 不再内部渲染 baseUrl（已挪 provider 子页，与 TemplateForm 同形态）
  const scriptForm = editDialog.slice(editDialog.indexOf("function ScriptForm"));
  assert.doesNotMatch(scriptForm, /edit\.baseUrl/);
  assert.doesNotMatch(scriptForm, /setBaseUrl/);
  // 保存失败带回现场：脚本校验失败跳 script 子页、consoleUrl 失败跳 provider 子页
  assert.match(editDialog, /setScriptSub\("script"\)/);
  assert.match(editDialog, /setScriptSub\("provider"\)/);
});
