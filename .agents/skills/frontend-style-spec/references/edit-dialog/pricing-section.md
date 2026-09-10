# 定价编辑区（PricingSection 色板纪律）

> 组件归属：EditDialog 的 PricingSection 子组件（峰谷时段窗 + 三档价格编辑）。
> 索引见 [SKILL.md](../../SKILL.md)。

## T-008 定价编辑区禁用 Tailwind 色板（pricing-section）

**背景**：该组件自定义区分支曾整片使用 Tailwind slate/indigo/orange/blue/red 色板
（2026-08-28 修复），与同区块 `--qt-*` 令牌两套并存、暗色观感割裂。

**纪律**：

- **禁止** slate / indigo / orange / blue / red 等 Tailwind 颜色工具类进入本组件；
  Tailwind 布局类（flex/grid/间距）允许保留。
- 颜色一律走令牌或其任意值引用（`text-[var(--qt-text)]` 等）；派生透明度用
  `color-mix` 包令牌。
- **语义对号入座**：峰谷圆点必用 `--qt-peak` / `--qt-offpeak`；删除动作 hover 转
  `--qt-danger`；激活/品牌动作用 accent 系；弱化文本用 `text-soft`。
- 周中日按钮、窗口编辑卡、价格档卡的边框/底色组合已在 2026-08-28 定稿，新元素
  复用同组合，不另调色。

**代码锚点**：`PricingSection.tsx` 的 WindowEditor / PriceTierEditor / ModeButton。

**官方资料披露**（2026-09-10，目录验收修复）：`PricingProvenance` 供编辑页和主窗卡片复用。
以原生 details/summary 点击展开来源、核验日期和超过 30 天的未核验提示；summary 复用
`qt-btn qt-btn-ghost`，来源动作复用 Button ghost 且显式 type=button，避免编辑表单误提交；
移动端由 `.qt-pricing-provenance .qt-btn` 覆盖到至少 44×44px（T-010）。正文使用 text-soft、
12px 字与网格 4px 间距，URL 允许换行。模型下架标记常显，不收进披露内容；自定义模型不标为官方核验。
