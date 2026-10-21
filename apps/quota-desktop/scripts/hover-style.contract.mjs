// 悬停面板样式契约（按仓库既有的 node:test 样式脚本约定，不进 vitest）：
// 抽屉非线性进出场、外侧条目区模糊、关闭态不可交互、减弱动效兜底。
// CSS 断言只锁「有这条规则且方向正确」，避免把具体时长/缓动冻结成回归点。
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const css = await readFile(new URL("../src/index.css", import.meta.url), "utf8");
const panel = await readFile(
  new URL("../src/components/HoverPanel.tsx", import.meta.url),
  "utf8",
);

/** 取某条 CSS 规则的声明块；选择器按字面匹配（需自行转义）。 */
function block(selector) {
  const index = css.indexOf(selector);
  assert.notEqual(index, -1, `缺少样式规则：${selector}`);
  const open = css.indexOf("{", index);
  const close = css.indexOf("}", open);
  return css.slice(open + 1, close);
}

test("抽屉关闭态不可交互：pointer-events 关闭 + visibility 延迟到动画结束", () => {
  const closed = block(".qt-hover-drawer {");
  assert.match(closed, /pointer-events:\s*none;/);
  assert.match(closed, /visibility:\s*hidden;/);
  // 滑出结束后才隐藏，保证出场动画完整可见
  assert.match(closed, /visibility\s+0s\s+linear\s+\.34s/);
  // 打开态恢复交互与可见
  const open = block(".qt-hover-drawer.is-open {");
  assert.match(open, /visibility:\s*visible;/);
  assert.match(open, /pointer-events:\s*auto;/);
});

test("进出场使用不同的非线性缓动，且时长与组件内的退场定时器一致", () => {
  const closed = block(".qt-hover-drawer {");
  const open = block(".qt-hover-drawer.is-open {");
  const easeOut = open.match(/transform\s+([\d.]+)s\s+(cubic-bezier\([^)]*\))/);
  const easeIn = closed.match(/transform\s+([\d.]+)s\s+(cubic-bezier\([^)]*\))/);
  assert.ok(easeOut, "打开态应有 transform 缓动");
  assert.ok(easeIn, "关闭态应有 transform 缓动");
  assert.notEqual(easeOut[2], easeIn[2], "进出场缓动不应相同");
  // 非线性：控制点不在 (0,0)/(1,1) 直线上
  for (const curve of [easeOut[2], easeIn[2]]) {
    assert.doesNotMatch(curve, /cubic-bezier\(0,\s*0,\s*1,\s*1\)/);
  }
  // 回弹会让抽屉越过边界再拉回：控制点的 y 不得超出 [0,1]
  for (const curve of [easeOut[2], easeIn[2]]) {
    const [, , y1, , y2] = curve.match(/cubic-bezier\(([\d.]+),\s*([\d.]+),\s*([\d.]+),\s*([\d.]+)\)/);
    for (const y of [Number(y1), Number(y2)]) {
      assert.ok(y >= 0 && y <= 1, `缓动控制点纵坐标越界（会产生回弹）：${curve}`);
    }
  }
  // CSS 出场时长必须等于组件里的 DRAWER_EXIT_MS，否则遮罩会比动画早撤
  const exitMs = Number(panel.match(/const DRAWER_EXIT_MS = (\d+);/)?.[1]);
  assert.equal(Number(easeIn[1]) * 1000, exitMs, "出场 transition 与 DRAWER_EXIT_MS 不一致");
  // 出场比入场短：退场不拖尾
  assert.ok(Number(easeIn[1]) < Number(easeOut[1]), "出场应比入场更快");
});

test("面板容器用 clip 裁切，避免焦点滚入把整面板横向拽动", () => {
  const panelCss = block(".qt-hover-panel {");
  assert.match(panelCss, /overflow:\s*clip;/);
  // focus 必须禁自动滚动：抽屉动画期间还在面板右外侧
  assert.match(panel, /focus\(\{\s*preventScroll:\s*true\s*\}\)/);
});

test("抽屉展开时外侧条目区模糊且不可点击", () => {
  const openList = block(".qt-hover-body-view.is-drawer-open .qt-hover-list {");
  assert.match(openList, /filter:\s*blur\(/);
  assert.match(openList, /pointer-events:\s*none;/);
  // 模糊本身要有过渡，避免开合瞬间跳变
  assert.match(block(".qt-hover-list {"), /transition:\s*filter/);
});

test("遮罩盖在列表之上、抽屉之下，只拦截外侧点击", () => {
  assert.match(block(".qt-hover-drawer-scrim {"), /z-index:\s*1;/);
  assert.match(block(".qt-hover-drawer {"), /z-index:\s*2;/);
});

test("prefers-reduced-motion 下抽屉与列表都关闭过渡", () => {
  // 文件里存在多个 reduce 媒体块：取包含悬停选择器的那一块（按花括号平衡截取）
  const marker = "@media (prefers-reduced-motion: reduce) {";
  let reduced = "";
  for (let start = css.indexOf(marker); start !== -1; start = css.indexOf(marker, start + 1)) {
    let depth = 0;
    for (let i = css.indexOf("{", start); i < css.length; i += 1) {
      if (css[i] === "{") depth += 1;
      else if (css[i] === "}") {
        depth -= 1;
        if (depth === 0) {
          const body = css.slice(start, i + 1);
          if (body.includes(".qt-hover-drawer")) reduced = body;
          break;
        }
      }
    }
    if (reduced) break;
  }
  assert.ok(reduced, "缺少悬停面板的 reduced-motion 兜底");
  assert.match(reduced, /\.qt-hover-drawer,/);
  assert.match(reduced, /\.qt-hover-list/);
  assert.match(reduced, /transition:\s*none;/);
});

test("副标题气泡由外层承载、内层负责截断（截断层会裁掉伪元素）", () => {
  assert.match(block(".qt-hover-row-sub-wrap {"), /position:\s*relative;/);
  assert.match(block(".qt-hover-row-sub {"), /overflow:\s*hidden;/);
  // 气泡挂在 wrap 的 ::after 上，而不是 overflow:hidden 的 sub
  assert.doesNotMatch(block(".qt-hover-row-sub {"), /::after/);
  assert.match(css, /\.qt-hover-row-sub-wrap::after\s*\{/);
});

test("组件侧同步维护交互锁定：遮罩在退场期间保留、行移出 Tab 序列", () => {
  assert.match(panel, /const drawerVisible = drawerOpen \|\|/);
  assert.match(panel, /\{drawerVisible && \(/);
  assert.match(panel, /tabIndex=\{drawerVisible \? -1 : undefined\}/);
  assert.match(panel, /drawerRef\.current\.inert = !drawerOpen/);
});
