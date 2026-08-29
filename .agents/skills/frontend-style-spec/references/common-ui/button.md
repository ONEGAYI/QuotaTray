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

Android 仍沿用视觉图标尺寸，但触摸命中区须通过容器或平台覆盖扩到至少 44×44px；
移动端激活反馈使用 `:active` / `aria-pressed`，不得只定义 hover（见 T-010）。

**特例登记**：

- `qt-gate-info-btn`（便携首启确认页问号钮）：26px（同迷你档尺寸）但
  `border-radius: 50%` 圆形——圆形与药丸 `999px` 同为不入档位形态；
  danger 变体（danger 字，hover danger-soft 底，展开态同 hover）。
  同时是 disclosure 按钮（aria-expanded），非纯图标钮（2026-08-28 登记）。
- `qt-console-btn`（余额卡片「访问控制台」·桌面）：迷你档 26px/xs 圆角，尺寸
  入档；hover/focus 转 accent 10% 底 + accent 字（默认弱化色 `--qt-text-faint`），
  偏离「hover 一律 surface-soft 底 + text 字」规则——2026-08-28 所有者定案，
  依据 docs/specs/console-link-spec.md §2（2026-08-29 登记）。
- `qt-console-text-btn`（同入口 · Android trailing 文字按钮）：仅移动端渲染于
  route 行最右（`margin-left: auto`），13px 字 + 16px 图标、sm 圆角、默认
  `text-soft` 无实心底、`:active` 出 surface-soft pressed 底，命中区 min-height
  44px（T-010；「视觉小、命中大」惯例）。语义优先于纯图标（2026-08-29 所有者
  定案：文字「控制台」/"Console" + ArrowUpRight 比裸 ↗ 图标信息价值高）。
  mobile-style 契约锁定（min-height/margin-left/pressed 底/route 行 flex）。

**文字链接式按钮**（行内下划线动作）：`accent-strong` 字 + hover 下划线；
删除类动作 hover 转 `danger` 字。不得引入 Tailwind 色板（见 T-008）。
