# 悬停气泡（Tooltip / IconButton / data-tooltip 机制）

> 组件粒度：ui.tsx 的 `Tooltip`、`IconButton` 与 index.css 的 `[data-tooltip]` 全局机制。
> 条目按 ID 排列；索引与维护规则见 [SKILL.md](../../SKILL.md)。

条目字段：**标准样式**（视觉契约）/ **实现机制**（怎么写代码）/ **禁止**（违反即 bug）/
**例外与豁免**（附依据）/ **代码锚点**（回源码对质）。

## T-001 悬停气泡（tooltip）

**标准样式**（以 provider 卡片「更多操作」按钮的气泡为准，2026-08-28 所有者确认）：

- 反色配色：背景 `var(--qt-text)`、文字 `var(--qt-surface)`，明暗主题自动互换
- 定位：目标上方 7px、水平居中；xs 圆角；内边距 5px 8px；最大宽 360px
- 单行不换行；无边框、无阴影、无箭头
- 字号固定 12px（对齐卡片更新时间字号，不随锚点继承；2026-08-28 所有者确认）
- hover / focus-visible 触发：标准档时长淡入 + 3px 上浮归位

**实现机制**：全局属性选择器 `[data-tooltip]::after` 渲染（`content: attr(data-tooltip)`），
按场景选择封装入口：

| 场景 | 做法 |
| --- | --- |
| 图标按钮 | `IconButton` 组件的 `label` 自动挂 `data-tooltip` |
| 包裹任意元素 | `Tooltip` 组件（`qt-tooltip-anchor`，已含 `position: relative`） |
| 长文本 | `Tooltip` 加 `multiline` prop（或手动另挂 `is-multiline` 类）：允许换行（限宽同全局 360px） |
| 手动挂载 | 元素加 `data-tooltip` 且自身 `position` 非 static（气泡绝对定位的锚点） |

注意：`select` 等 replaced 元素上伪元素不渲染，须用 `Tooltip` 组件包裹而非直接挂属性；
`text` 可能缺省时传 `?? ""`——空串不渲染气泡（对齐原生 title 的 undefined 语义）。

**长文本判定与病根**（2026-08-28，限宽 360px / 字号 12px 下复核）：默认气泡
`nowrap`，溢出文字以 `--qt-surface` 色落在页面同色底上**不可见**（观感是"被截断"）。
按 12px 估算凡文案可能超过约 55 个拉丁字符或 28 个汉字的挂载点必须 multiline。
已知长文本实例：

- provider 卡模型窗口选择器：select 气泡（`Tooltip multiline`）与默认态路由标签
  气泡（`is-multiline` 类）——同一段「平台 · 模型」长标题，两个挂载点都要挂
  （路由标签气泡在窄屏媒体查询下可悬停触发，非死代码）
- 定价三档解释（PricingSection，英文 47-51 字符）
- 错误详情（ProviderCard / SettingsDialog，headline + 脱敏 detail）

**禁止**：原生 `title=` 属性——系统默认样式不可定制、出现有延迟、观感与标准气泡不一致。

**Android 分流**：`body.qt-mobile-runtime` 下统一禁用伪元素气泡；纯操作图标保留
`aria-label`，必要解释改为常显或点击 disclosure。不得把 `:hover` 触发原样带到触摸端，
完整状态机见 T-009。

**例外与豁免**：

- `<option title>`：原生下拉选项由操作系统渲染，CSS 无法作用，允许保留（技术限制）。
- `qt-usage-tooltip`（使用统计图表数据悬浮卡）：富内容数据卡（时间 + 数值 + 说明，
  跟随数据点定位），不适用纯文字气泡形态，整体豁免本条目（2026-08-28 所有者确认）。
- `.qt-gate-info-btn[data-tooltip]::after` 右对齐变体（便携首启确认页问号钮）：
  `left: auto; right: 0; transform: none` 覆盖居中锚定与 3px 上浮——按钮贴卡片右缘，
  居中锚定的气泡会被滚动卡片（overflow 裁剪容器）右缘裁掉（2026-08-28，技术限制）。

**代码锚点**：`index.css` 的 `[data-tooltip]::after` 与 `.qt-tooltip-anchor`；
`ui.tsx` 的 `IconButton`、`Tooltip`。
