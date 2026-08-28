# 反馈提示块与进度条（feedback banner / progress）

> 组件粒度：index.css 的 `qt-inline-error`、`qt-inline-warning`、`qt-ai-assist-safety`、
> `qt-transfer-intro`、`qt-update-status`、`qt-hover-progress`、`qt-update-progress-track`。
> 索引见 [SKILL.md](../../SKILL.md)。

## T-007 反馈提示块与进度条（feedback-banner / progress）

**提示块两档**（2026-08-28 统一原先四套圆角/内距）：

| 档 | 形态 | 关键取值 |
| --- | --- | --- |
| 行内档 | soft 底 + 语义色字的纯文本提示 | md 圆角、padding 11px 13px、字号 12（`qt-inline-error` / `qt-inline-warning` / `qt-ai-assist-safety`） |
| 横幅档 | 带图标/标题的结构化状态卡 | md 圆角、padding 13px（`qt-transfer-intro` / `qt-update-status`） |

- 语义配色只从 `success/warning/danger/accent` 四组 soft+主色对里选。
- 托盘悬停窗 `qt-hover-error` 是紧凑变体（sm 圆角、10px 字），密度豁免。

**细进度条**：高 5px、药丸 999px、轨道 `text-faint 17%` 混色底、填充 accent
（`qt-hover-progress` 与 `qt-update-progress-track` 共用此规格，2026-08-28 统一）。
