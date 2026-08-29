import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const css = await readFile(new URL("../src/index.css", import.meta.url), "utf8");

test("Android 宽视口仍以显式详情按钮控制卡片展开", () => {
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-details-toggle\s*\{[^}]*display:\s*inline-flex;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-provider-card:not\(\.is-expanded\):hover \.qt-provider-secondary[^}]*max-height:\s*0;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-provider-card\.is-expanded \.qt-provider-secondary[^}]*max-height:\s*1400px;/s,
  );
});

test("Android 通用文字按钮与图标按钮具有按压反馈", () => {
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-btn:not\(:disabled\):active,[\s\S]*body\.qt-mobile-runtime \.qt-icon-btn:not\(:disabled\):active\s*\{[^}]*opacity:/,
  );
});

test("Android 控制台直达为 trailing 文字按钮且满足 44px 命中区（T-010）", () => {
  // 文字按钮：视觉小（13px 字 + 16px 图标）、命中区 ≥44px、默认无实心底
  assert.match(
    css,
    /\.qt-console-text-btn\s*\{[^}]*min-height:\s*44px;[^}]*margin-left:\s*auto;/s,
  );
  assert.match(
    css,
    /\.qt-console-text-btn:active\s*\{[^}]*background:/s,
  );
  // 所在的 route 行转 flex 使按钮 trailing 靠右，label 保持省略号
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-provider-route\s*\{[^}]*display:\s*flex;[^}]*align-items:\s*center;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-provider-route-label[^{]*\{[^}]*flex:\s*1;[^}]*min-width:\s*0;/s,
  );
});

test("Android 更新页主行动按钮满足 44px 命中区（T-010）", () => {
  // 检测/下载/安装是更新页唯一主行动（对话框 footer 外），2026-08-29
  // 审查修复补齐；与 dialog-footer 的 44px 同口径
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-update-status \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
});

test("Android 原生下拉满足 44px 命中区（T-010 全量整改）", () => {
  // #61 登记的模型选择器挂 qt-select 复合类，基类 .qt-select（源码顺序更晚）
  // 级联压过 .qt-provider-model-select 的死声明——移动端单条规则同时覆盖
  // 模型选择器与设置页语言/主题下拉（后两者本就 mobile-only 渲染）
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-select\s*\{[^}]*min-height:\s*44px;/s,
  );
});

test("Android 分段控件按钮满足 44px 命中区（T-010/T-002 联动）", () => {
  // 2026-08-29 所有者定案：移动端直接加高（视觉即热区），compact 仅收紧
  // 字号/边距、不豁免命中区；类名级视觉共 4 处实例（编辑页签/模板子页签/
  // 计费模式/峰谷切换）由单条规则全覆盖
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-segmented button\s*\{[^}]*min-height:\s*44px;/s,
  );
});

test("Android 模板预设钮与帮助折叠钮满足 44px 命中区（T-010 全量整改）", () => {
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-template-presets button\s*\{[^}]*min-height:\s*44px;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-template-help-toggle\s*\{[^}]*min-height:\s*44px;/s,
  );
});

test("Android 更新页行内链接按语境分治满足 44px 命中区（T-010）", () => {
  // 2026-08-29 所有者定案，分而治之：句中行内链接（qt-inline-link）保持
  // 行内排版、透明伪元素外扩热区（先例 .qt-page-tabs::before 的负 inset）；
  // 独立成行链接（qt-settings-manual-link）直接撑到 44px
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-settings-manual-link\s*\{[^}]*min-height:\s*44px;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-inline-link\s*\{[^}]*position:\s*relative;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-inline-link::before\s*\{[^}]*position:\s*absolute;[^}]*inset:\s*-15px -8px;/s,
  );
});

test("Android 统计页与正文主按钮满足 44px 命中区（T-010 审查轮补齐）", () => {
  // 2026-08-29 审查轮发现：统计页裸下拉（无 qt-select 类，38px）与重置钮
  // 29px、设置迁移区/模板试查区正文按钮（qt-btn 基类 36px，无移动覆盖）
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-usage-selector select\s*\{[^}]*min-height:\s*44px;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-usage-reset\s*\{[^}]*min-height:\s*44px;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-settings-content \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-template-actions \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
});

test("Android 峰谷编辑触摸目标补齐（T-010 审查轮补齐）", () => {
  // 周几切换钮挂语义类直接 44px；行内文字钮（清除覆盖/添加时段/移除时段/
  // 重置档位）沿用分治定案——qt-touch-inline 通用热区类与 qt-inline-link
  // 的伪元素外扩同构、不带视觉样式；预置模型下拉经补挂 qt-select 覆盖
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-pricing-day-chip\s*\{[^}]*min-height:\s*44px;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-touch-inline\s*\{[^}]*position:\s*relative;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-touch-inline::before\s*\{[^}]*position:\s*absolute;[^}]*inset:\s*-15px -8px;/s,
  );
});
