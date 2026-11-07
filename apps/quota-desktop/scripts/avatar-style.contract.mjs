// 条目头像样式契约（沿用仓库既有 node:test 样式脚本约定，不进 vitest）：
// hover 才浮出的齿轮 + 底层变灰遮罩、自定义图 cover 与品牌图 contain 的形态
// 分流、Android 无 hover 时的常显兜底、减弱动效。CSS 断言只锁「有这条规则
// 且方向正确」，不冻结具体时长/缓动值。
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const css = await readFile(new URL("../src/index.css", import.meta.url), "utf8");
const component = await readFile(
  new URL("../src/components/ProviderAvatar.tsx", import.meta.url),
  "utf8",
);
const dialog = await readFile(
  new URL("../src/components/AvatarDialog.tsx", import.meta.url),
  "utf8",
);
const hover = await readFile(
  new URL("../src/components/HoverPanel.tsx", import.meta.url),
  "utf8",
);
const avatarView = await readFile(
  new URL("../src/components/avatarView.ts", import.meta.url),
  "utf8",
);

/** 取某条 CSS 规则的声明块；选择器按字面匹配。 */
function block(selector) {
  const index = css.indexOf(selector);
  assert.notEqual(index, -1, `缺少样式规则：${selector}`);
  const open = css.indexOf("{", index);
  const close = css.indexOf("}", open);
  return css.slice(open + 1, close);
}

test("齿轮与遮罩默认不可见，hover/focus 才浮出", () => {
  const gear = block(".qt-avatar-gear {");
  assert.match(gear, /opacity:\s*0;/, "齿轮默认应隐藏");
  const scrim = block(".qt-avatar-scrim {");
  assert.match(scrim, /opacity:\s*0;/, "遮罩默认应隐藏");

  const hover = block(".qt-avatar-edit:hover .qt-avatar-gear");
  // 规则体在 `,` 续行的选择器后；直接查全文关键组合更稳
  assert.match(
    css,
    /\.qt-avatar-edit:hover \.qt-avatar-gear,\s*\.qt-avatar-edit:focus-visible \.qt-avatar-gear \{[^}]*opacity:\s*1;/,
    "hover 与键盘 focus 都要浮出齿轮",
  );
  assert.match(
    css,
    /\.qt-avatar-edit:hover \.qt-avatar-scrim,\s*\.qt-avatar-edit:focus-visible \.qt-avatar-scrim \{[^}]*opacity:\s*1;/,
    "hover 与键盘 focus 都要压暗底层",
  );
  assert.ok(hover.length > 0);
});

test("底层变灰用半透明叠加而非 grayscale 滤镜（保留图片可辨识性）", () => {
  const scrim = block(".qt-avatar-scrim {");
  assert.match(scrim, /background:\s*color-mix\(/, "遮罩应为半透明叠加");
  assert.doesNotMatch(scrim, /grayscale/, "不得用 grayscale 把彩色图整张去色");
});

test("自定义图铺满、平台品牌图留边：两条 img 规则分道", () => {
  const platformImg = block(".qt-avatar-inner img {");
  assert.match(platformImg, /object-fit:\s*contain;/, "品牌图按规范留边");
  const customImg = block(".qt-avatar-inner.is-upload img {");
  assert.match(customImg, /object-fit:\s*cover;/, "上传图铺满容器");
  assert.match(customImg, /width:\s*100%;/, "上传图应吃满容器宽度");
});

/** 契约：三个 avatar 容器（卡片 / 悬停面板 / 弹窗预览）都必须有 is-custom
 *  形态。曾经只有卡片有，悬停面板的 is-custom 是死类——结构改了、样式忘了。 */
test("三个头像容器的 is-upload 形态都落地（照片铺满）", () => {
  for (const container of ["qt-avatar-inner", "qt-hover-avatar-inner", "qt-avatar-preview-chip"]) {
    const rule = block(`.${container}.is-upload img {`);
    assert.match(rule, /object-fit:\s*cover;/, `${container} 的上传图应 cover 铺满`);
    assert.match(rule, /height:\s*100%;/, `${container} 的上传图应吃满高度`);
  }
});

/** 契约：内置品牌图即便被手动选中也保持 contain。
 *  品牌 SVG 的 viewBox 差异极大（openrouter 401×294、novita 24×15），
 *  cover 会裁得认不出；挑图格里是 contain，选中后突变也很突兀。 */
test("is-brand 不铺满：仍按品牌图规范留边", () => {
  assert.doesNotMatch(
    css,
    /\.is-brand img \{[^}]*object-fit:\s*cover;/,
    "is-brand 不得被套上 cover",
  );
  assert.match(avatarView, /innerClassName\("brand"/, "品牌覆盖应走 is-brand 分支");
  assert.match(avatarView, /innerClassName\("upload"/, "上传图应走 is-upload 分支");
});

/** 契约：解析层一次性给出展示类，消费点不再各自判别 kind。
 *  三处渲染点重复判别曾是死类的成因（其中一处忘加）。 */
test("展示类由解析层统一产出，消费点直接取用", () => {
  assert.match(avatarView, /className:\s*innerClassName\(/, "解析结果应自带 className");
  for (const [name, source] of [
    ["ProviderAvatar", component],
    ["HoverPanel", hover],
    ["AvatarDialog", dialog],
  ]) {
    assert.doesNotMatch(
      source,
      /kind === "custom" \? " is-(custom|upload|brand)"/,
      `${name} 不应自行判别填充形态，应使用 source.className`,
    );
  }
});

test("Android 无 hover：齿轮常显但降透明度", () => {
  const touch = block(".qt-avatar-edit.is-touch .qt-avatar-gear {");
  assert.match(touch, /opacity:\s*\.72;/, "触屏上齿轮应常显提示可点");
  assert.match(component, /is-touch/, "组件的触屏类必须与样式成对");
});

test("可交互壳用 button 承载键盘可达性，纯展示分支不嵌 button", () => {
  assert.match(component, /<button[\s\S]*?type="button"[\s\S]*?aria-label=\{t\("avatar\.open"\)\}/);
  // 悬停面板的行本身是 button，那里必须走纯展示分支（不传 onEdit）
  assert.match(component, /if \(!editable\) \{/, "无 onEdit 时应走纯展示分支");
});

/** 契约：纯展示分支不靠「生产路径总是传 onEdit」兜尺寸。
 *  .qt-avatar-inner 是 width/height:100%，外层没有 42px 基准时会撑满父容器；
 *  卡片头像的尺寸必须对两个分支都成立。 */
test("卡片头像的 42px 对纯展示分支同样生效", () => {
  const rule = block(".qt-provider-avatar:not(.qt-avatar-edit) {");
  assert.match(rule, /width:\s*42px;/, "纯展示分支需要 42px 宽度基准");
  assert.match(rule, /height:\s*42px;/, "纯展示分支需要 42px 高度基准");
});

/** 契约：挑图格也是品牌图容器，浅色图形要换深底——否则 stepfun 的纯白
 *  图形落在浅底上就是白底白图（同形态漏了第四个容器）。 */
test("挑图格支持浅色品牌图变体", () => {
  const rule = block(".qt-avatar-choice.is-light-logo {");
  assert.match(rule, /background:\s*var\(--qt-logo-chip-bg-invert\);/, "浅色品牌图应换深底");
  assert.match(dialog, /isLightLogo\(choice\.slug\)/, "组件应按 slug 判定浅色并加类");
});

test("头像弹窗的网格与预览块有对应样式，且移动端有触摸命中区", () => {
  assert.match(css, /\.qt-avatar-grid \{[^}]*grid-template-columns:\s*repeat\(auto-fill/);
  assert.match(css, /\.qt-avatar-preview-chip \{[^}]*width:\s*56px;/);
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-avatar-choice \{[^}]*min-height:\s*52px;/,
    "移动端选择格需够触摸",
  );
  assert.match(dialog, /role="radiogroup"/, "内置图标网格应是一组单选");
  assert.match(dialog, /role="radio"[\s\S]*?aria-checked=\{selectedBrand === choice\.slug\}/);
});

test("减弱动效偏好下关闭过渡", () => {
  assert.match(
    css,
    /@media \(prefers-reduced-motion: reduce\) \{\s*\.qt-avatar-inner, \.qt-avatar-scrim, \.qt-avatar-gear \{ transition: none; \}/,
  );
});

/** 契约：is-custom 不得压掉 is-light-logo 的深底。
 *  两者同特异性，若 is-custom 无条件改 background，文件顺序会让它覆盖浅色
 *  变体——选内置 stepfun（纯白图形）会得到白底白图，图标隐形。 */
test("自定义形态不覆盖浅色变体的深底", () => {
  const custom = block(".qt-avatar-inner.is-upload:not(.is-light-logo) {");
  assert.ok(custom.includes("background"), "非浅色时才允许改底色");
  for (const selector of [
    ".qt-hover-avatar-inner.is-upload:not(.is-light-logo) {",
    ".qt-avatar-preview-chip.is-upload:not(.is-light-logo) {",
  ]) {
    assert.doesNotThrow(() => block(selector), `缺少排除浅色变体的规则：${selector}`);
  }
});

/** 契约：纯展示分支不套空壳外层。
 *  悬停面板的列表行本身是 button，不能内嵌 button，所以不传 onEdit——
 *  那个分支若仍套一层无尺寸的 span，调用方的 42px 尺寸类会悬空。 */
test("纯展示分支把尺寸类落到内层 chip 上", () => {
  const start = component.indexOf("if (!editable)");
  assert.notEqual(start, -1, "应有纯展示分支");
  // 切分锚点不写死换行符：CRLF 检出下 "\n    <button" 不匹配会让 slice
  // 扫到文件尾，断言看到的是整个文件而不是那个分支——测试就空转了
  const rest = component.slice(start);
  const nextBranch = rest.search(/return\s*\(\s*<button/);
  assert.notEqual(nextBranch, -1, "纯展示分支之后应有 button 分支作为边界");
  const branch = rest.slice(0, nextBranch);
  assert.ok(branch.includes("qt-avatar-inner"), "纯展示分支应渲染内层 chip，而非空壳 span");
  assert.ok(branch.includes("${className ?"), "调用方的尺寸类应拼到内层 chip 上");
});

