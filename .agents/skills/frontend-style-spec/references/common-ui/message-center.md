# 消息中心（MessageCenter / 标题栏铃铛下拉 + 移动顶部应用栏入口）

> 组件粒度：`MessageCenter.tsx`，index.css 的 `qt-msg-*`。
> 索引见 [SKILL.md](../../SKILL.md)。

## T-009 消息中心（铃铛 + 红点 + 点击展开面板）

**标准样式**（2026-08-28 草案，随静默安装改造引入；2026-08-30 增移动形态
与新消息类型）：

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

**消息类型**（按 kind 分支渲染，事件源头按平台分流——UI 不做平台判定）：

- `update-ready`（仅桌面产生）：安装包已下载，「现在安装」直调静默安装；
- `update-available`（仅移动产生）：检测到新版本（无自动下载），按钮
  「查看更新」跳设置·更新页（`onViewUpdates` prop，面板随跳转收起）；
- `low-balance`（两端）：某条目任一窗口已用百分比达阈值，纯展示无按钮；
  按条目 id 去重并存，上限 5 条（超限丢最旧），其余 kind 每类仅留最新一条。

**实现机制**：消息列表与已读集合由 App 级 state 持有、props 下传
`TitleBar` / `MobileTopBar` → `MessageCenter`；`update-ready` /
`update-available` / `low-balance` 三个后端事件驱动入列（合并语义见
`messageCenterView.ts` 的 `mergeMessage`）。安装动作直调
`api.installUpdate()`——卡片文案已明示「退出并自动重启」后果，点击即
确认，不再叠加系统 confirm（与设置页安装按钮的 confirm 入口语境不同）。

**移动形态**（2026-08-30 引入，T-010/T-011 联动；同日真机截屏回归修订定位与层叠）：

- 入口挂 `MobileTopBar` 动作区（消息、设置、添加三个常显动作），
  铃铛命中区随 `.qt-mobile-topbar .qt-icon-btn` 44px 规则覆盖；
- 面板贴 44px 按钮下缘（`top: 48px`）；**右缘以 `right: -96px` 负偏移
  对齐动作区右缘**（= 铃铛右侧 gap4 + 设置钮 44 + gap4 + 加号钮 44，
  动作区按钮增减须同步此值）——铃铛是动作区最左按钮，`right: 0` 会把
  280px 面板推出屏幕左缘（2026-08-30 真机截屏实证）；宽度
  `min(280px, 100vw - 28px)` 防小屏溢出；面板内按钮 `min-height: 44px`；
  红点沿用桌面样式；
- **层叠**：`.qt-mobile-topbar` 持 `position: relative; z-index: 60`
  （与桌面 `.qt-titlebar` 同口径）——topbar 的 `backdrop-filter` 使其
  成为层叠上下文，不显式抬升时面板 `z-index: 60` 被困其中，内容区
  `position: relative` 的卡片会盖住面板（同日真机截屏实证）；
- 外点关闭依赖 WebView 触摸合成 `mousedown`（模拟器已列实证项，
  契约由 `mobile-style.contract.mjs` 锁定）。

**禁止**：红点自选红色（必须 `var(--qt-danger)`）；面板宽度照抄
`qt-dropdown` 160px（消息卡片需要更宽）或擅自偏离登记宽度 280px
（调整宽度须同步修改本条目登记值；移动端 `min()` 收窄是登记宽度
的小屏下限，不算偏离）。

**例外与豁免**：无。

**代码锚点**：`MessageCenter.tsx`、`messageCenterView.ts`；
`index.css` 的 `.qt-msg-dot` / `.qt-msg-panel` / `.qt-msg-card`；
移动覆盖规则与 `mobile-style.contract.mjs`。
