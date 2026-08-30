# 设计令牌规范（design-tokens 域）

> 归属范围：`index.css` `:root` / `:root.dark` 的 `--qt-*` 令牌体系——每档值是什么、
> 何时用哪档。**组件粒度条目只写使用场景（用哪个令牌），不重复记录值**；值的增删改
> 只发生在本域。索引与维护规则见 [SKILL.md](../../SKILL.md)。

## DT-001 颜色令牌（33 × 明暗两套）

**结构**：`:root` 定义亮色，`:root.dark` 同名覆盖暗色——任何颜色令牌都必须两套成对
出现，只加亮色不加暗色视为违反。

| 组 | 令牌 | 语义 |
| --- | --- | --- |
| 表面 | `shell(-highlight)` / `surface(-soft/-hover)` | 窗体底 → 卡片面 → 弱底 → hover 底，四层递进 |
| 文本 | `text` / `text-soft` / `text-faint` | 正文 / 次要 / 弱化，三级不许跳档混用 |
| 边框 | `border` / `border-strong` | 常规分隔 / 强调边框（输入框边用 strong） |
| 强调 | `accent(-rgb/-strong/-soft)` / `on-accent` | 品牌紫系；`-rgb` 供 `rgb()` 透明度合成；激活态一律走 `accent-strong` |
| 语义 | `success` `warning` `danger`（各带 `-soft`） | 成功 / 警示 / 危险，配对 soft 底构成提示块 |
| 峰谷 | `peak` / `offpeak` | 定价高峰橙 / 空闲蓝，凡峰谷语义必用此对，禁自选色 |
| 图表 | `chart-axis` `chart-grid` `chart-gap-stripe` / `series-1..4` | 图表轴 / 网格 / 断档条纹 / 固定顺序的比较曲线色槽 |
| 其他 | `shadow` `scrollbar-thumb(-hover)` | 阴影色 / 滚动条 |

**半透明派生**一律 `color-mix(in srgb, var(--qt-*) N%, transparent)`，禁止新写死 rgba/hex。

## DT-002 圆角档位（五档，2026-08-28 立）

| 令牌 | 值 | 适用 |
| --- | --- | --- |
| `--qt-radius-xs` | 6px | 微元素：code 芯片、菜单项、迷你图标钮内圆、气泡 |
| `--qt-radius-sm` | 8px | 控件：输入框、select、按钮、图标钮、分段按钮 |
| `--qt-radius-md` | 10px | 区块：提示块、下拉面板、嵌入面板、编辑器卡 |
| `--qt-radius-lg` | 13px | 卡片：provider 卡、占位卡、悬停面板、定价区块 |
| `--qt-radius-xl` | 15px | 弹窗与大内容卡：dialog、图表卡 |

- 药丸（badge、进度条、拖拽把手）用 `999px` 直写，不入档位。
- 容器与内嵌元素的圆角差保持 2-3px 视觉内缩（如分段容器 sm / 按钮钮 xs）。

## DT-003 动效时长档位（四档，2026-08-28 立）

| 令牌 | 值 | 适用 |
| --- | --- | --- |
| `--qt-dur-1` | .07s | 微反馈：颜色/透明度/光效即时跟随 |
| `--qt-dur-2` | .14s | 标准过渡：按钮、气泡、图标 hover |
| `--qt-dur-3` | .18s | 面板级：卡片、次级区展开、主题切换 |
| `--qt-dur-4` | .24s | 大编排：拖拽让位、面板切换动画 |

- 长动画（旋转 .8s、主题扩散 .5s、进度循环 1.2s）是业务专用，不入档位。
- `prefers-reduced-motion` 全局降级已由样式表兜底，无需逐条处理。

## DT-004 不随主题的例外组（登记制）

以下色值**有意不进**明暗互换体系（`:root` 直挂、dark 不覆盖），新增此类例外必须在
本条目登记，未登记的硬编码色即违反 DT-001：

- `--qt-logo-chip-*`（bg/fg/border + invert 变体）：品牌图标容器固定浅底/反转变体，
  保证单色深 logo 明暗主题均可见（provider avatar、native-picker 图标三处共享）。
- `#e81123`：Windows 标题栏关闭钮 hover 红（平台惯例）。
- 圆角/时长令牌本身不随主题。

## 间距与字号现状（暂不令牌化）

间距、字号、z-index 目前无令牌，沿用既有 px 值；新增组件粒度条目时应**登记所用
关键取值**，等漂移再次出现时再评估是否立档（2026-08-28 决策）。
