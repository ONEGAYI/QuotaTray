# 余额卡片（hover 反馈与几何中性）

> 组件粒度：index.css 的 `.qt-provider-card` 三态反馈与 route 行叠放。
> 索引见 [SKILL.md](../../SKILL.md)。

## T-014 卡片 hover 反馈几何中性（hover 振荡防线）

**状态**：草案（2026-09-05 立，随 PR #99 修复沉淀）

**契约**：卡片 `:hover` / `:focus-within` / `.is-expanded` 三态反馈只
允许**不改变命中测试几何**的属性——边框高亮、阴影加深、opacity、
visibility。禁止两类几何变化源：

1. **transform 位移**（如 `translateY(-1px)` 上浮）：位移会移动 hover
   命中边界，压线悬停时与 hover 命中互为因果形成自持振荡（放大器，
   曾把振荡区扩大到下边线 ±1-2px）。
2. **display 互切换位**：route 行平台名文字与模型下拉框曾用 display
   互切换位，两态内容高度不同使 primary 高度瞬时跳 ±9.5px，是压线
   振荡的主因（仅带模型下拉的卡片触发，故抖动只出现在个别卡片）。

**几何恒定手法**：route 行 grid 同格叠放——label 与
`.qt-tooltip-anchor` 均 `grid-area: 1 / 1`，下拉框常驻占位
（`display: block; visibility: hidden`），hover 只切 `visibility`。
桌面主规则、窄屏 `@media (max-width: 700px)`、`body.qt-mobile-runtime`
三处分支必须保持同一 visibility 语义，不得回退 display 互切。

**背景案例**（2026-09-05，PR #99）：悬停展开带模型下拉的卡片后鼠标
压下边线疯狂抖动（动态实测 hover 每秒翻转 99 次）；两处几何源拆除后
下边线 -2/-1/0/+1px 全稳定，重新注入旧 translateY 压力测试亦稳定。
