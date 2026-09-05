import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const css = await readFile(new URL("../src/index.css", import.meta.url), "utf8");
const ui = await readFile(new URL("../src/components/ui.tsx", import.meta.url), "utf8");
const usageStats = await readFile(new URL("../src/components/UsageStatsPage.tsx", import.meta.url), "utf8");

test("统计曲线色槽由明暗成对 CSS 令牌单一维护（DT-001/T-013）", () => {
  const darkRoot = css.match(/:root\.dark\s*\{(?<body>[^}]*)\}/s)?.groups?.body ?? "";
  for (let slot = 1; slot <= 4; slot += 1) {
    assert.match(css, new RegExp(`--qt-series-${slot}:`));
    assert.match(darkRoot, new RegExp(`--qt-series-${slot}:`));
    assert.match(usageStats, new RegExp(`var\\(--qt-series-${slot}\\)`));
  }
  assert.doesNotMatch(usageStats, /#df6f9f|#3d9b87|#e49537/);
});

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

test("Android 更新错误详情图标为可点 disclosure 且满足 44px 命中区（T-010）", () => {
  // 悬停气泡被移动端全局禁用（body.qt-mobile-runtime [data-tooltip]::after
  // display:none），错误详情（如 403 的 GitHub message）唯一通路是点击
  // 展开收起——图标按钮化 + 触摸目标 44px + 按压反馈；桌面仍走悬停气泡
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-error-detail-toggle\s*\{[^}]*min-height:\s*44px;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-error-detail-toggle\s*\{[^}]*min-width:\s*44px;/s,
  );
  assert.match(
    css,
    /\.qt-error-detail-toggle:active\s*\{[^}]*background:/s,
  );
  // 展开态的视觉确认：收起态压暗与桌面悬停图标同口径，展开时全亮
  assert.match(
    css,
    /\.qt-error-detail-toggle\[aria-expanded="true"\]\s*\{[^}]*opacity:\s*1;/s,
  );
  // 展开的详情正文：块级次级小字，长 message 任意断行不撑破错误卡片
  assert.match(
    css,
    /\.qt-error-detail-body\s*\{[^}]*display:\s*block;[^}]*overflow-wrap:\s*anywhere;/s,
  );
});

test("Android 统计页与正文主按钮满足 44px 命中区（T-010 审查轮补齐）", () => {
  // 统计页工具栏组合按钮与重置钮、设置迁移区/模板试查区正文按钮
  // 均需覆盖 qt-btn 基类的 36px 高度。
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-usage-comparison-actions \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-usage-reset\s*\{[^}]*min-height:\s*44px;/s,
  );
  // 卡头按钮组（定位线开关 + 重置）按组覆盖（2026-09-05 定位线）
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-usage-head-actions \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
  // 定位线手柄命中圆是 SVG 属性（CSS 正则锁不到），按 TSX 源码锁取值：
  // 移动端 r=22（viewBox 360 近 1:1 渲染，直径 ≈44px 触达）
  assert.match(
    usageStats,
    /className="qt-usage-marker-hit"[^>]*r=\{mobile \? 22 : 18\}/,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-settings-content \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-template-actions \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
  // 范围切换容器是纯布局钩子（T-002）：分段视觉一律走全局 qt-segmented，
  // 移动端 44px 命中区由 .qt-segmented button 单条规则覆盖
  assert.match(
    css,
    /\.qt-usage-range-switch\s*\{\s*flex:\s*none;\s*\}/,
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

test("Android 消息中心入口与面板满足触摸规范（T-009/T-010/T-011）", () => {
  // 铃铛入口在 MobileTopBar 内：qt-icon-btn 44px 规则覆盖全部图标钮
  assert.match(
    css,
    /\.qt-mobile-topbar \.qt-icon-btn\s*\{[^}]*width:\s*44px;[^}]*height:\s*44px;/s,
  );
  // 下拉面板贴 44px 按钮下缘（桌面 38px 是 34px 按钮的锚定值）
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-mobile-topbar \.qt-titlebar-menu-anchor \.qt-dropdown\s*\{[^}]*top:\s*48px;/s,
  );
  // 小屏宽度收窄防溢出
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-msg-panel\s*\{[^}]*width:\s*min\(280px,\s*calc\(100vw - 28px\)\);/s,
  );
  // 面板内按钮（现在安装/查看更新）触摸目标 44px
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-msg-panel \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
});

test("Android 消息面板不被内容卡片遮挡且右缘对齐 actions 组（真机截屏回归）", () => {
  // topbar 的 backdrop-filter 使其成为层叠上下文——不显式抬升 z-index 时，
  // 面板的 z-index 60 被困其中，内容区 .qt-provider-card（position: relative，
  // DOM 靠后）会绘制在 topbar 之上盖住面板（2026-08-30 真机截屏确认）。
  // 取 40：高于内容卡片与图表提示（auto/z-5），低于全屏弹窗遮罩 z-50——
  // 移动端弹窗 header 与 topbar 几何重叠，topbar 高于遮罩会劫持弹窗关闭
  // 按钮的点击（review 轮发现的回归）。断言按声明各自独立匹配（顺序无关）。
  assert.match(
    css,
    /\.qt-mobile-topbar\s*\{[^}]*position:\s*relative;/s,
  );
  assert.match(
    css,
    /\.qt-mobile-topbar\s*\{[^}]*z-index:\s*40;/s,
  );
  // 面板右缘不再对齐铃铛锚点（铃铛是 actions 组最左按钮，right:0 会把
  // 280px 面板推出屏幕左缘）——负偏移 96px = 铃铛右侧 gap4 + 设置钮44 +
  // gap4 + 加号钮44，使面板右缘对齐 actions 组右缘（距屏右 14px，与
  // topbar padding 对齐）。MobileTopBar actions 按钮增减时需同步此值。
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-mobile-topbar \.qt-titlebar-menu-anchor \.qt-dropdown\s*\{[^}]*right:\s*-96px;/s,
  );
});

test("Android 使用统计组合为居中 82dvh 浮窗并虚化背景（T-013）", () => {
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-dialog-backdrop\.qt-usage-dialog-backdrop\s*\{[^}]*place-items:\s*center;[^}]*backdrop-filter:\s*blur\(10px\);/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-dialog\.qt-dialog-usage-comparison\s*\{[^}]*width:\s*calc\(100% - 20px\);[^}]*height:\s*min\(82dvh,\s*680px\);[^}]*border-radius:\s*var\(--qt-radius-xl\);/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-dialog-usage-comparison \.qt-select,[\s\S]*body\.qt-mobile-runtime \.qt-dialog-usage-comparison \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
});

test("DialogShell 仅在启用时点击遮罩关闭，内部点击不关闭", () => {
  assert.match(
    ui,
    /if \(closeOnBackdrop && event\.target === event\.currentTarget\) onClose\(\);/,
  );
});
