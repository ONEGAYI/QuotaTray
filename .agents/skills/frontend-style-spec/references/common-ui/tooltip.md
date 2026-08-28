# 悬停气泡（Tooltip / IconButton / data-tooltip 机制）

> 组件粒度：ui.tsx 的 `Tooltip`、`IconButton` 与 index.css 的 `[data-tooltip]` 全局机制。
> 条目按 ID 排列；索引与维护规则见 [SKILL.md](../../SKILL.md)。

条目字段：**标准样式**（视觉契约）/ **实现机制**（怎么写代码）/ **禁止**（违反即 bug）/
**例外与豁免**（附依据）/ **代码锚点**（回源码对质）。

## T-001 悬停气泡（tooltip）

**标准样式**（以 provider 卡片「更多操作」按钮的气泡为准，2026-08-28 所有者确认）：

- 反色配色：背景 `var(--qt-text)`、文字 `var(--qt-surface)`，明暗主题自动互换
- 定位：目标上方 7px、水平居中；6px 圆角；内边距 5px 8px；最大宽 240px
- 单行不换行；无边框、无阴影、无箭头
- hover / focus-visible 触发：0.14s ease 淡入 + 3px 上浮归位

**实现机制**：全局属性选择器 `[data-tooltip]::after` 渲染（`content: attr(data-tooltip)`），
按场景选择封装入口：

| 场景 | 做法 |
| --- | --- |
| 图标按钮 | `IconButton` 组件的 `label` 自动挂 `data-tooltip` |
| 包裹任意元素 | `Tooltip` 组件（`qt-tooltip-anchor`，已含 `position: relative`） |
| 手动挂载 | 元素加 `data-tooltip` 且自身 `position` 非 static（气泡绝对定位的锚点） |
| 长文本 | 另挂 `is-multiline` 类：允许换行并放宽限宽（用于错误详情等长内容） |

注意：`select` 等 replaced 元素上伪元素不渲染，须用 `Tooltip` 组件包裹而非直接挂属性。

**禁止**：原生 `title=` 属性——系统默认样式不可定制、出现有延迟、观感与标准气泡不一致。

**例外与豁免**：

- `<option title>`：原生下拉选项由操作系统渲染，CSS 无法作用，允许保留（技术限制）。
- `qt-usage-tooltip`（使用统计图表数据悬浮卡）：富内容数据卡（时间 + 数值 + 说明，
  跟随数据点定位），不适用纯文字气泡形态，整体豁免本条目（2026-08-28 所有者确认）。

**代码锚点**：`index.css` 的 `[data-tooltip]::after` 与 `.qt-tooltip-anchor`；
`ui.tsx` 的 `IconButton`、`Tooltip`。
