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
  // template/script 两个 provider 子页：baseUrl → 凭据 → 控制台 → 定价
  const providerSeq =
    /baseUrlField\}\s*\{credentialField\}\s*\{credential2Field\}\s*\{consoleUrlField\}\s*\{pricingSection\}/g;
  assert.equal(
    (editDialog.match(providerSeq) ?? []).length,
    2,
    "模板与脚本的 provider 子页应为 baseUrl→凭据→控制台→定价",
  );
  // native 分支：凭据（CLI/普通 + 第二槽）→ 控制台 → 定价
  assert.match(
    editDialog,
    /: credentialField\}\s*\{nativeKey2Required && credential2Field\}\s*\{consoleUrlField\}\s*\{pricingSection\}/,
  );
});

test("关键字段卡片底座覆盖必填字段（名称/baseUrl/运营商/双凭据）", () => {
  // 凭据卡片的底座样式抽为通用 qt-field-card，并扩散到全部必填字段；
  // 旧类名 qt-credential-field 不再存在（语义从凭据专区演变为关键字段卡片）
  assert.doesNotMatch(editDialog, /qt-credential-field/);
  assert.doesNotMatch(css, /qt-credential-field/);
  assert.match(css, /\.qt-field-card\s*\{[^}]*background:\s*var\(--qt-surface-soft\);/s);
  assert.match(css, /\.qt-field-card small\s*\{/);
  // 底座套用清单：name / baseUrl / native 平台选择 / 三处凭据字段
  const cards = (editDialog.match(/qt-field-card/g) ?? []).length;
  assert.equal(cards, 6, "name+baseUrl+平台选择+三个凭据字段均应带卡片底座");
});
