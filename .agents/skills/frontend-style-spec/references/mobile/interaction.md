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

**触摸目标**：主要按钮、图标按钮、底部导航项、选择器项与拖拽把手的可点击区域至少
44×44px；视觉图标可保持 16–24px。触摸拖拽只从把手启动并设置 `touch-action: none`，
卡片其他区域继续允许纵向滚动。

**代码锚点**：`runtimeView.ts`、`MobileChrome.tsx`、`ProviderCard.tsx`、
`NativeProviderPicker.tsx`、`UsageStatsPage.tsx` 与 `index.css` 的
`body.qt-mobile-runtime` 段。
