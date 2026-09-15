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

test("三形态字段序：凭据（key）优先，控制台地址次之，峰谷定价殿后", () => {
  // template/script 两个 provider 子页：baseUrl → 凭据 →（卡片闭合）→ 控制台 → 定价
  const providerSeq =
    /baseUrlField\}\s*\{credentialField\}\s*\{credential2Field\}\s*<\/div>\s*\{consoleUrlField\}\s*\{pricingSection\}/g;
  assert.equal(
    (editDialog.match(providerSeq) ?? []).length,
    2,
    "模板与脚本的 provider 子页应为 baseUrl→凭据→控制台→定价",
  );
  // native 分支：凭据（CLI/普通 + 第二槽）→ 控制台 → 定价
  assert.match(
    editDialog,
    /: credentialField\}\s*\{nativeKey2Required && credential2Field\}\s*<\/div>\s*\{consoleUrlField\}\s*\{pricingSection\}/,
  );
});

test("必填字段合并为单一卡片容器", () => {
  // 独立字段底座合并：template/script 子页与 native 分支各一个 qt-field-card
  // 大容器收纳全部必填字段，内部字段退回裸 qt-field（不再每字段各自带底座）
  const containers = (editDialog.match(/<div className="qt-field-card">/g) ?? []).length;
  assert.equal(containers, 3, "模板/脚本子页与 native 分支各一个必填卡片容器");
  assert.doesNotMatch(editDialog, /qt-field qt-field-card/);
  // 容器收纳清单：basics（名称/平台选择）+ baseUrl + 双凭据（template/script）
  assert.match(
    editDialog,
    /qt-field-card">\s*<div className="qt-edit-basics">\{nameField\}<\/div>\s*\{baseUrlField\}\s*\{credentialField\}\s*\{credential2Field\}/,
  );
  // native 容器收纳凭据（CLI/普通 + 条件第二槽）后收尾
  assert.match(
    editDialog,
    /: credentialField\}\s*\{nativeKey2Required && credential2Field\}\s*<\/div>/,
  );
  // 容器样式：grid 行距 + surface-soft 底；small 提示行样式随容器选择器生效
  assert.match(
    css,
    /\.qt-field-card\s*\{[^}]*display:\s*grid;[^}]*gap:\s*12px;[^}]*background:\s*var\(--qt-surface-soft\);/s,
  );
  assert.match(css, /\.qt-field-card small\s*\{/);
});
