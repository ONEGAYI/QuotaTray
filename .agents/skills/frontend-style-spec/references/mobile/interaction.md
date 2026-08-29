# 移动端触摸交互

> 适用范围：Android 壳层及 `body.qt-mobile-runtime` 下的共享组件。索引见
> [SKILL.md](../../SKILL.md)。

## T-010 触摸交互与 disclosure

**平台判定**（2026-08-28 所有者确认）：由后端 `BootStateDto.platform` 声明 Android，
不得以窗口宽度或粗指针单独判定运行平台。窄桌面窗口仍保留桌面语义；Android 横屏仍是
移动语义。

**悬停替代契约**：

- Android 不得把必要信息只放在 `:hover`、`pointerenter` 或伪元素 tooltip 中。
- 卡片次级信息使用显式“详情/收起”按钮，点击驱动 `aria-expanded` 状态；再次点击、完成
  选择或关闭页面时收起。
- 多级选择器的一级分组以点击切换，选中二级项后关闭；鼠标悬停仅是桌面增强，不得改变
  移动端可达性。
- 图表以触摸按下/拖动更新选中点；离开图表后保留最后一个选中点，点击空白或切页清除。
- 纯操作图标依靠 `aria-label`；解释性长文必须常显或进入点击 disclosure，Android 下不
  渲染 `[data-tooltip]` 伪元素。

**触摸目标**：主要按钮、图标按钮、底部导航项、多级选择器项（NativePicker）与
拖拽把手的可点击区域至少 44×44px；视觉图标可保持 16–24px。触摸拖拽只从把手
启动并设置 `touch-action: none`，卡片其他区域继续允许纵向滚动。

**全量口径**（2026-08-29 所有者定案）：除上述枚举外的其余可点控件——原生下拉
（`qt-select`，含裸 select 与补挂类的变体）、分段控件、预设钮、折叠钮、正文
行动按钮（设置迁移/模板试查）等——命中区同样按 44×44px 执行；分段控件的高度
条款见 T-002。

**行内文字链接/文字钮**（2026-08-29 所有者定案，按语境分治）：嵌在句子或
标签行中间的行内控件保持行内排版，命中区经透明伪元素外扩——`position:
relative` + `::before { position: absolute; inset: -15px -8px }`（先例
`.qt-page-tabs::before`）；更新页句中链接用 `qt-inline-link`（自带行内视觉），
峰谷区行内文字钮（清除覆盖/添加时段/移除时段/重置档位）用 `qt-touch-inline`
（通用热区类，不带视觉样式，视觉由各自既有类承担）。独立成行的文字链接
（如发布页外链）直接 `min-height: 44px`。外扩热区允许覆盖相邻纯文本，但所在
段落/行内不得存在其他可点元素。

**代码锚点**：`runtimeView.ts`、`MobileChrome.tsx`、`ProviderCard.tsx`、
`NativeProviderPicker.tsx`、`UsageStatsPage.tsx`、`PricingSection.tsx` 与
`index.css` 的 `body.qt-mobile-runtime` 段。
