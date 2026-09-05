---
name: frontend-style-spec
description: QuotaTray 桌面与移动前端样式/交互规范的唯一事实源与查询维护入口。凡涉及 apps/quota-desktop 的组件样式、悬停或触摸交互、移动端布局、按钮、弹窗、动效、title 属性、设计令牌及样式 review，动手前必须按索引读取对应 references；约定变更必须与代码同 PR 回写，散落说明一律不作为规范。
---

# 前端样式规范（Spec）

## 核心约定

- **references/ 是唯一事实源**：规范正文按**组件归属**组织——每个组件域一个文件夹
  （如 `common-ui/`），域内**按组件粒度**分指导文件（如 `tooltip.md`）；每条规范是
  一个条目，带稳定 ID（T- 组件条目 / DT- 令牌条目）、状态（草案/生效/废弃）与最后
  确认日期。
- **令牌分层**（2026-08-28 立）：组件条目只写**使用场景**（什么情况用哪档令牌），
  不重复记录令牌值；值与档位定义只在 `design-tokens/` 域维护——改档位只动一处，
  全部条目自动跟随。
- **SKILL.md 是索引**：只承载条目索引表与维护规则，不承载规范内容本身。
- **先查后写**：前端样式工作动笔前先按下表定位条目；没有条目的模式，先在对应归属
  文件起草条目（状态标「草案」）再写代码。
- **同 PR 回写**：引入新样式模式或修改既有约定时，代码与规范在同一 PR 内变更；
  既有条目的行为级修改须经项目所有者确认后更新状态与日期。
- **规范可回源码对质**：条目尽量绑定代码锚点（组件名、CSS 选择器、文件路径），
  写不可验证的描述时主动收窄。
- **设计令牌优先**：颜色必走 `--qt-*` 颜色令牌（或其 `color-mix` 派生）；圆角走
  `--qt-radius-*` 五档、动效时长走 `--qt-dur-*` 四档（见 DT-002/003）；间距与字号
  暂不令牌化但须在条目中登记取值（见 tokens.md 尾节）。禁止在组件里写死魔法值，
  不随主题的例外色必须在 DT-004 登记。

## 条目索引

### 组件条目（T-）

| ID | 模式 | 归属（references 文件） | 适用范围 | 状态 | 最后确认 |
| --- | --- | --- | --- | --- | --- |
| T-001 | 悬停气泡（tooltip） | [common-ui/tooltip.md](references/common-ui/tooltip.md) | 全部组件 | 生效 | 2026-08-28 |
| T-002 | 分段控件（segmented） | [common-ui/segmented.md](references/common-ui/segmented.md) | 全部组件 | 生效 | 2026-08-29 |
| T-003 | 焦点环（focus ring） | [common-ui/focus.md](references/common-ui/focus.md) | 全部可交互元素 | 生效 | 2026-08-28 |
| T-004 | 按钮（button / icon-button） | [common-ui/button.md](references/common-ui/button.md) | 全部组件 | 生效 | 2026-08-31 |
| T-005 | 表单字段（field / input / select） | [common-ui/field.md](references/common-ui/field.md) | 主窗与弹窗表单 | 生效 | 2026-08-28 |
| T-006 | 空态与占位卡（empty-state） | [common-ui/empty-state.md](references/common-ui/empty-state.md) | 全部视图 | 生效 | 2026-08-28 |
| T-007 | 反馈提示块与进度条 | [common-ui/feedback-banner.md](references/common-ui/feedback-banner.md) | 全部视图 | 生效 | 2026-08-28 |
| T-008 | 定价编辑区禁用 Tailwind 色板 | [edit-dialog/pricing-section.md](references/edit-dialog/pricing-section.md) | PricingSection | 生效 | 2026-08-28 |
| T-009 | 消息中心（铃铛 + 红点 + 点击展开面板） | [common-ui/message-center.md](references/common-ui/message-center.md) | 标题栏 | 草案 | 2026-08-28 |
| T-010 | 移动端触摸交互与 disclosure | [mobile/interaction.md](references/mobile/interaction.md) | Android 前端 | 生效 | 2026-08-29 |
| T-011 | 移动端壳层与全屏页面 | [mobile/layout.md](references/mobile/layout.md) | Android 前端 | 生效 | 2026-08-28 |
| T-012 | 配置指引渲染器（文档排版） | [common-ui/guide-viewer.md](references/common-ui/guide-viewer.md) | 全部组件 | 生效 | 2026-08-30 |
| T-013 | 使用统计多曲线比较与定位线 | [usage-stats/comparison-chart.md](references/usage-stats/comparison-chart.md) | 使用统计页 | 生效 | 2026-09-05 |

### 令牌条目（DT-）

| ID | 内容 | 归属 |
| --- | --- | --- |
| DT-001 | 颜色令牌结构与成对规则 | [design-tokens/tokens.md](references/design-tokens/tokens.md) |
| DT-002 | 圆角五档 | 同上 |
| DT-003 | 动效时长四档 | 同上 |
| DT-004 | 不随主题例外组（登记制） | 同上 |

## 维护流程

1. **查询**：按索引表「归属」列定位 references 文件，读对应条目。
2. **变更**：改代码的同时更新条目；新增例外或豁免须注明日期与依据（所有者确认/技术限制）。
3. **归属文件增建**：新条目按组件归属定位——域文件夹已存在则在其下新建组件粒度文件
   （如 `common-ui/button.md`），域不存在则新建 `references/<组件域>/<组件>.md`，
   并在索引表登记。
4. **索引同步**：新增条目、变更状态或新建归属文件时，同步本文件索引表。
5. **令牌变更**：档位值的增删改只发生在 design-tokens 域；组件条目引用档位名而非值，
   无需跟随修改。
