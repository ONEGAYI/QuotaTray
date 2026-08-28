# 消息中心（MessageCenter / 标题栏铃铛下拉）

> 组件粒度：`MessageCenter.tsx`，index.css 的 `qt-msg-*`。
> 索引见 [SKILL.md](../../SKILL.md)。

## T-009 消息中心（铃铛 + 红点 + 点击展开面板）

**标准样式**（2026-08-28 草案，随静默安装改造引入）：

- 触发钮：复用 `qt-icon-btn`（常规 34px / sm 圆角档）+ `qt-titlebar-menu-anchor`
  锚定（与语言/主题菜单同构）；tooltip 走 `IconButton` label 机制。
- 红点：`danger` 实心圆点 8px，绝对定位于触发钮右上角（offset 7px），药丸
  直写不入圆角档位（对齐 DT-002 药丸例外）；仅在有未读消息时渲染。
- 面板：复用 `DropdownMenu`（外点关闭）+ 定制类 `qt-msg-panel`——
  `qt-dropdown` 基础上放宽至 280px；圆角 / 边框 / 阴影随 `qt-dropdown`
  （md 档，见 DT-002）不变。
- 消息卡片：面板内一条消息一块；标题行（text）+ 说明行（text-soft，
  12px）+ 动作按钮（`qt-btn secondary` 小号）+ 后果提示行（text-faint，
  12px）。卡片间距 8px，以 `border` 色分隔（不引入新色）。
- 空态：面板内居中文本（text-faint，12px），不另起空态卡（T-006 的
  empty-state 面向视图级，面板级轻量文本豁免）。
- 已读语义：打开面板即全量已读（红点消失）；消息为会话级内存态，
  不持久化。

**实现机制**：消息列表与已读集合由 App 级 state 持有、props 下传
`TitleBar` → `MessageCenter`；`update-ready` 事件（后端 `UPDATE_READY_EVENT`）
驱动入列。安装动作直调 `api.installUpdate()`——卡片文案已明示
「退出并自动重启」后果，点击即确认，不再叠加系统 confirm
（与设置页安装按钮的 confirm 入口语境不同）。

**禁止**：红点自选红色（必须 `var(--qt-danger)`）；面板宽度照抄
`qt-dropdown` 160px（消息卡片需要更宽）或擅自偏离登记宽度 280px
（调整宽度须同步修改本条目登记值）。

**例外与豁免**：无。

**代码锚点**：`MessageCenter.tsx`；`index.css` 的 `.qt-msg-dot` /
`.qt-msg-panel` / `.qt-msg-card`。
