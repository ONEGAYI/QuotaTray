# 分段控件（SegmentedControl / qt-segmented）

> 组件粒度：`ui.tsx` 的 `SegmentedControl`、index.css 的 `qt-segmented`。值（圆角/时长）
> 见 design-tokens 域，本条目只写场景与契约。索引见 [SKILL.md](../../SKILL.md)。

## T-002 分段控件（segmented）

**唯一实现**：`qt-segmented`（容器）+ 其 `button[aria-pressed="true"]` 激活态。
**禁止**再建同视觉的平行实现（2026-08-28 收敛了 qt-edit-tabs / qt-pricing-mode /
Tailwind ModeButton 三套分叉）。

**视觉契约**（激活态）：surface 白底浮起 + `accent-strong` 文字 + 微阴影 + 字重 600；
非激活：透明底 `text-soft`。容器：`surface-soft` 底、内缩圆角（容器 sm / 按钮 xs）。

**使用场景**：

- 二至五个互斥视图切换（编辑弹窗一级页签、定价模式切换、时段预设/自定义切换）。
- 选项超过五个或需要描述文案时用别的模式（如设置页左导航），不硬塞分段。
- 密度收紧（嵌入卡片头部）：容器加 `qt-segmented-compact`——收紧字号、边距
  与桌面密度高度（34→30px），移动端命中区不豁免（见「移动端触摸目标」）。
- 容器边距等布局差异：允许叠加布局钩子类（如 `qt-pricing-mode` 只管 margin），
  但不得重新定义分段视觉。

**无障碍**：按钮用 `aria-pressed` 表达选中态；禁用态 `opacity: .4`（通用规则已含）。

**移动端触摸目标**（T-010 联动，2026-08-29 所有者定案）：Android 下
`.qt-segmented button` 命中区 `min-height: 44px`，视觉即热区、直接加高；compact
同样适用，密度收紧只作用于字号与边距。
