# 配置指引渲染器（文档排版）

> 组件粒度：`GuideViewer.tsx`、`guideMd.ts` / `guideDocs.ts`（解析与资产收集），
> index.css 的 `qt-dialog-guide` / `qt-guide-*`。索引见 [SKILL.md](../../SKILL.md)。

## T-012 配置指引渲染器（guide viewer）

**形态**（2026-08-30 立，入口位置同日修订）：嵌套 `DialogShell size="lg"`（`qt-dialog-guide`，
定高 `min(78vh, 720px)` 与编辑弹窗同模式，长文档在 body 内滚动），从 EditDialog
**第二凭据槽 hint 行**的行内「配置指引」链接打开（仅 `guideDocs.ts` 的
`GUIDE_FOR_PROVIDER` 登记过的平台渲染）。渲染轻量 Markdown 子集
（解析见 `guideMd.ts`，子集清单见预研文档 §六）；文档 h1 在弹窗语境降级
为 h2（弹窗标题承担一级语境）。

**排版取值**（间距与字号暂不令牌化，按 SKILL 约定在此登记）：

| 元素 | 取值 |
| --- | --- |
| 标题 h2/h3/h4（文档 h1/h2/h3） | 15px / 14px / 13px，600 字重，`margin 4px 0 0`；h4 另有 `text-soft` 色 |
| 正文 / 列表 | 13px，行高 1.65 / 1.6；内容区块间 gap 10px；列表为原生布局保留 ::marker（禁用 `display: grid/flex`——grid item 会把 li blockify 成 block 吞掉圆点/序号），项间距用 `li + li { margin-top: 6px }`（`padding-left: 22px`） |
| 围栏代码块 | 12px，行高 1.55，`padding 10px 12px`，surface-soft 底 + border + radius-sm，横向滚动 |
| 行内代码 | `qt-guide-code-inline`：0.92em 等宽（`ui-monospace, Consolas`），`padding 1px 5px`，surface 底 + border + radius-xs |
| 引用块 | 13px，行高 1.6，`text-soft` 色，accent 左边线 3px + surface-soft 底 + radius-xs，`padding 8px 12px` |
| 分隔线 | border-top 1px，`margin 4px 0` |
| 图片 | `max-width: 100%` + border + radius-sm；资产未命中渲染虚线框占位（`padding 6px 10px`，text-faint，12px） |
| 指引入口 | 第二凭据槽 hint 行内的行内文字链接（`qt-guide-entry` + `qt-guide-link` 复用，`margin-top 2px`）：仅登记过指引（`GUIDE_FOR_PROVIDER`）且为 native 表单时渲染；`type="button"`（form 内防提交） |

**交互**：行内外链为文字链接式按钮（`qt-guide-link`，accent-strong 字 +
hover 下划线，T-004 行内链接式），点击经 `openConsoleUrl` 打开（白名单
http/https 与控制台直达同口径）；热区外扩 `::before inset: -15px -8px`
（T-010 行内链接同款）。图片资产经 vite `?url` glob 打包（源在
`docs/assets/bundle/`），文档内引用名与产物名由 `guideDocs.ts` 映射解耦。

**禁止**：新增块级/行内语法先扩 `guideMd.ts` 子集约定（预研文档 §六同步），
不得在组件里对原文做二次字符串处理；颜色一律走令牌。
