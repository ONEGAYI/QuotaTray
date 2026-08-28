# 空态与占位卡（empty / loading / placeholder / state）

> 组件粒度：index.css 的 `qt-empty-state`、`qt-loading-card`、`qt-history-placeholder`、
> `qt-usage-state`、`qt-hover-empty`。索引见 [SKILL.md](../../SKILL.md)。

## T-006 空态与占位卡（empty-state）

**两档形态**（2026-08-28 立，统一原先 13/15 圆角交替）：

| 档 | 形态 | 关键取值 | 实例 |
| --- | --- | --- | --- |
| 基础档 | 虚线边框占位卡 | lg 圆角、dashed `border-strong`、`surface 78%` 底、padding 34px | `qt-empty-state` / `qt-loading-card` |
| 加强档 | 带图标块的大占位 | lg 圆角、图标块（48px accent-soft 底）或大标题 | `qt-history-placeholder`（44px 28px）、`qt-usage-state`（40px 24px） |

- 圆角一律 lg（卡片档），占位卡禁用 xl。
- 托盘悬停窗的 `qt-hover-empty` 是无边界紧凑变体（无虚线框），属密度豁免。
- 错误型占位（`qt-usage-state.is-error`）：dashed 边转 `danger` 混色 + `danger-soft` 底。
