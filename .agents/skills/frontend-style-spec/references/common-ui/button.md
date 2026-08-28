# 按钮（Button / IconButton）

> 组件粒度：`ui.tsx` 的 `Button`、`IconButton`，index.css 的 `qt-btn*` / `qt-icon-btn*`。
> 索引见 [SKILL.md](../../SKILL.md)。

## T-004 按钮（button / icon-button）

**文字按钮变体矩阵**（`qt-btn` + variant 类，不得自造第六种）：

| variant | 底/字 | 场景 |
| --- | --- | --- |
| `primary` | accent 底 / on-accent 字 | 每视图至多一个主行动 |
| `secondary` | surface 底 + border / text-soft | 常规操作 |
| `ghost` | 透明底 / text-soft，hover 变 `surface-soft` 底 + text | 卡片内工具行 |
| `danger` | danger 底 / on-accent 字 | 破坏性操作（删除、导入覆盖） |

**激活态**（toggle 语义，如代理开关）：一律 `color: var(--qt-accent-strong)`
（2026-08-28 修复硬编码蓝 `#2f6fde`），需要更强区分时叠 accent-soft 底，禁自选色。

**图标钮尺寸档位**（宽高 / 圆角档）：

| 档 | 尺寸 | 圆角 | 实例 |
| --- | --- | --- | --- |
| 常规 | 34px | sm | `qt-icon-btn`（主窗各处） |
| 紧凑 | 31px | sm | `qt-hover-icon-btn`（托盘悬停窗） |
| 迷你 | 26px | xs | `qt-ai-assist-close` |
| 行内迷你 | 18px | xs | `qt-provider-error-copy`（嵌 12px 文本行） |

新图标钮按场景选档，不另设尺寸；hover 一律 `surface-soft` 底 + `text` 字
（danger 变体例外：danger-soft 底 + danger 字）。

**特例登记**：

- `qt-gate-info-btn`（便携首启确认页问号钮）：26px（同迷你档尺寸）但
  `border-radius: 50%` 圆形——圆形与药丸 `999px` 同为不入档位形态；
  danger 变体（danger 字，hover danger-soft 底，展开态同 hover）。
  同时是 disclosure 按钮（aria-expanded），非纯图标钮（2026-08-28 登记）。

**文字链接式按钮**（行内下划线动作）：`accent-strong` 字 + hover 下划线；
删除类动作 hover 转 `danger` 字。不得引入 Tailwind 色板（见 T-008）。
