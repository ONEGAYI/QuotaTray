# 迁移模态（TransferExportDialog / TransferImportDialog）

> 组件粒度：`TransferExportDialog.tsx` / `TransferImportDialog.tsx` 与 index.css 的
> `qt-transfer-export-*` / `qt-transfer-import-*` 类族。弹窗骨架（DialogShell、
> 按钮、密码输入框）走既有通用组件与 T-004/T-005，本条目只登记迁移模态特有
> 类族。索引见 [SKILL.md](../../SKILL.md)。

## T-016 迁移模态特有类族（transfer-dialog）

**状态**：草案（2026-09-18 随 spec #119 双档双模模态窗引入，随审查 #127 补登记）。

**覆盖代码**：`qt-transfer-export-body` / `qt-transfer-export-info`（导出模态：
档位分段 + 双档表单）与 `qt-transfer-import-body` / `qt-transfer-import-file` /
`qt-transfer-import-file-meta` / `qt-transfer-import-ack`（导入模态：文件信息卡
+ 策略单选 + 覆盖三重防线）。

**布局节奏**（导出/导入两模态同构）：

| 取值 | 说明 |
| --- | --- |
| 模态体 `display: grid; gap: 12px` | 纵向区块间距统一 12px，由容器分发；体内 `.qt-transfer-intro` 复位 `margin: 0`（警示卡自带外边距交给容器 gap） |
| 信息块 padding `10px 12px` | 导出 info 行与导入文件卡共用的内距 |
| 正文 `font-size: 12px; line-height: 1.5~1.55` | 信息行/文件元信息 12px；行高纯文本行 1.5、含勾选/图标对齐行 1.55 |

**颜色全部走 `--qt-*` 令牌**（DT-001 合规）：soft 信息底 `--qt-surface-soft` +
`--qt-text-soft` 字；边框 `--qt-border`；md 圆角（DT-002）。

**信息行图标对齐模式**（导出 info / 导入 ack 共用）：flex 顶部对齐、`gap: 8px`，
图标或勾选框 `flex: none; margin-top: 1px` 基线补偿——长文案折行时图标/控件
钉在首行，不随行距漂移。

**导入文件信息卡**：`gap: 10px`，前置成功色 `--qt-success` 图标；文件名
`min-width: 0` + ellipsis 截断（`--qt-text` + 字重 600），元信息行
`--qt-text-soft` 12px/1.5。

**覆盖风险勾选 danger 化**：`.qt-transfer-import-ack input` 用
`accent-color: var(--qt-danger)`——覆盖导入三重防线的末道勾选以危险色呈现，
与 danger 确认钮（T-004）同语义；这是本仓库勾选框唯一允许偏离 accent 的位置，
新增场景不得效仿，须先扩本条目。

**警示卡复用**：导出便捷档警示与导入覆盖警告复用 T-007 横幅档
`qt-transfer-intro`，不另建变体；模态内使用时只复位外边距（见布局节奏首行）。
