# 表单字段（qt-field / qt-input / qt-select）

> 组件粒度：index.css 的 `qt-field`、`qt-input`、`qt-select`、`qt-input-compact`。
> 索引见 [SKILL.md](../../SKILL.md)。

## T-005 表单字段（field / input / select）

**基础规格**（`qt-input` / `qt-select` 共用）：`border-strong` 边 + surface 底 +
sm 圆角 + 36px 高；紧凑场景（嵌卡片行）加 `qt-input-compact`（32px）。移动端
联动（T-010 全量口径）：Android 下 `qt-select` 命中区提至 44px（下拉属可点
控件）；`qt-input` 是文本输入控件、不在此列，与相邻 select 的高度差为有意决策。

**select 统一**（2026-08-28 修三套分叉）：边框一律 `border-strong`、圆角 sm、
无阴影；悬停面板等高密度场景允许 `surface-soft` 底作紧凑变体，尺寸收缩但边框/
圆角不变。禁止再出现独立圆角或带阴影的 select 变体。

**焦点**：输入档（见 T-003）——`accent` 边框 + 20% 弱化环；select 用 `:focus-visible`。

**label 风格**：主窗表单 `qt-field > span`（text-soft 13px）；托盘悬停窗等紧凑域
允许 11px 缩档；图表工具栏 eyebrow（10px 大写字距）是独立风格，勿混用于普通表单。
