# 使用统计多曲线比较

> 适用范围：`UsageStatsPage`、`UsageComparisonDialog` 与 `qt-usage-*` 样式。
> 颜色、圆角和时长引用 design-tokens 域；索引见 [SKILL.md](../../SKILL.md)。

## T-013 多曲线比较与组合浮窗

**组合契约**（2026-08-30 所有者确认）：最多四条 `Provider + 模型/额度窗口`
组合；添加和管理分开，管理页逐项删除。组合、顺序与色槽落 `settings.json`，
颜色按固定色槽保持稳定；百分比可与一种绝对单位同图，第二种绝对单位不可添加。

**图表契约**：桌面悬浮详情按同一时间桶列出曲线；聚焦时只列聚焦项，清除聚焦
恢复全量。气泡横向跟随光标、纵向吸附到光标相反半区。短缺失用低透明连接，长缺失
直接断线，不覆盖整图斜纹、不生成插值。移动端使用全宽响应式图表，单指拖动更新时间
游标，读数常驻图表下方；聚焦过滤与桌面一致。

**Android 组合浮窗特例**（2026-08-30 所有者确认）：本弹窗不沿用 T-011 的
设置/编辑全屏页，改为视口居中浮窗；宽度为视口减 20px，高度
`min(82dvh, 680px)`，xl 圆角、强边框、面板阴影。遮罩使用 text 28% 混色与
10px 背景模糊；点击遮罩、关闭钮、Esc 或 Android 返回键只关闭该浮窗。浮窗内部
点击不关闭；按钮、下拉与删除动作命中区至少 44px。

**代码锚点**：`UsageStatsPage.tsx`、`UsageComparisonDialog.tsx`、
`usageComparisonView.ts`、`ui.tsx` 的 `DialogShell`、`index.css` 的
`qt-usage-*` 与 `qt-dialog-usage-comparison`。
