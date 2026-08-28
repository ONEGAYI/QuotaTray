# 焦点环（focus-visible 两档契约）

> 组件粒度：index.css 的 `button:focus-visible` 全局兜底与输入族弱化规则。
> 索引见 [SKILL.md](../../SKILL.md)。

## T-003 焦点环（focus ring）

**两档契约**（2026-08-28 立，收敛原先三体系五种浓度）：

| 档 | 样式 | 适用 |
| --- | --- | --- |
| 控件档 | `outline: 2px solid var(--qt-accent); offset 2px` | 所有 button（全局兜底） |
| 输入档 | `outline: 2px solid color-mix(accent 20%, transparent); offset 0` + 边框 accent | input / select / textarea / picker 触发器 |

**机制**：控件档由 `button:focus-visible` 全局兜底——**新增按钮类自动获得焦点环，
没有白名单可漏登**（原白名单式枚举导致 hover 面板图标钮、设置导航等四类按钮无焦点环，
2026-08-28 修复）。输入框文本编辑用 `:focus`（持续输入需常显），select 用
`:focus-visible`。

**特例覆盖**（specificity 高于全局兜底，改动需在本条登记）：

- `qt-page-tab`：控件档但 `offset 6px`（大标题页签视觉需要）。
- `qt-drag-handle`：输入档浓度（把手不宜实心环）。
- `qt-native-picker-group-button` / `qt-native-picker-option`：背景反馈型
  （`surface-soft` 底变化代替环，`outline: none`）。

**禁止**：新增 `outline: none` 而不提供替代焦点指示——违反即无障碍 bug。
