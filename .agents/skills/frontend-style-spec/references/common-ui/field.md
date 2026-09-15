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

**行内复选框**（`qt-check-inline`，2026-09-15 草案）：嵌在操作行
（`qt-template-actions` 等 flex 容器）内联使用的带文字复选框——inline-flex +
text-soft 13px 与 label 风格同档，checkbox 走 `accent-color: var(--qt-accent)`，
不使用原生默认蓝。与 `qt-field`（label 独占行、checkbox 换行堆叠）互斥使用：
凡与按钮同行的开关型选项用行内形态。移动端命中区 44px（T-010 全量口径，
视觉保持 13px 行高不放大）。代码锚点：`EditDialog.tsx` ScriptForm 的
allowInsecure 开关；契约测试 `scripts/edit-dialog-style.contract.mjs`。

**关键字段卡片**（`qt-field-card`，2026-09-15 草案，同日由六处独立字段底座
收敛为容器形态）：单一容器收纳编辑弹窗的必填字段——grid 12px 行距 +
surface-soft 底 + border 边 + md 圆角 + 13px 内边距；template/script 子页与
native 分支各一个，内部字段（名称、baseUrl 或平台选择、主/第二凭据含 CLI
凭据）为裸 `qt-field`。原 `qt-credential-field` 仅用于凭据（承载提示行与
「不回显」安全语义），扩散后抽为通用类；提示行样式经容器选择器
`.qt-field-card small` 生效。控制台直达、套餐变体、定价区及编辑器区块
不入卡片（非清单字段）。代码锚点：`EditDialog.tsx` 三处容器；契约测试
`scripts/edit-dialog-style.contract.mjs`。
