# quota-desktop 子树视图

桌面端（Tauri 2 + React）的文件树子集。数据源为 file-tree 技能 `tree.json`，下方块由脚本渲染，禁止手改；AGENTS.md 主树中本子树折叠为一行，本页承载明细。

```
<!-- file-tree:tree^id=apps-desktop:begin 由脚本渲染，禁止手改 -->
QuotaTray/
└── apps/
    └── quota-desktop/ # 桌面端（子树视图拆出）
        ├── eslint.config.js    # ESLint 扁平配置
        ├── index.html          # Vite HTML 入口
        ├── package.json        # pnpm前端清单
        ├── pnpm-lock.yaml      # 前端依赖锁文件
        ├── pnpm-workspace.yaml # pnpm 构建许可
        ├── scripts/            # 构建辅助脚本目录
        │   ├── android-post-init.contract.mjs # Android初始化契约测试
        │   ├── android-post-init.mjs          # Android工程安全初始化
        │   ├── android-tauri.contract.mjs     # Android构建入口测试
        │   ├── android-tauri.mjs              # Android构建环境入口
        │   ├── build-hook.contract.mjs        # 构建钩子契约测试
        │   ├── build-hook.mjs                 # 跨目标Tauri构建钩子
        │   ├── dev.contract.mjs               # dev端口探测避让契约测试
        │   ├── dev.mjs                        # dev端口探测避让入口
        │   ├── edit-dialog-style.contract.mjs # 编辑弹窗样式契约测试
        │   └── mobile-style.contract.mjs      # 移动样式契约测试
        ├── src/                # React 前端源码
        │   ├── api.ts                  # invoke 封装
        │   ├── App.tsx                 # 跨端主界面壳层
        │   ├── assets/                 # 静态资源
        │   │   ├── brand-mark.png # 透明品牌主图
        │   │   └── providers/     # Provider SVG 图标集
        │   ├── components/             # 前端组件
        │   │   ├── aiAssistPack.test.ts         # AI 求助包测试
        │   │   ├── aiAssistPack.ts              # AI 求助包纯逻辑
        │   │   ├── AiAssistPanel.tsx            # AI 调试求助面板
        │   │   ├── BrandMark.tsx                # 品牌标志薄组件
        │   │   ├── ClearConfigDialog.tsx        # 清空配置二级确认弹窗
        │   │   ├── clearConfigView.test.ts      # 清空确认逻辑测试
        │   │   ├── clearConfigView.ts           # 清空确认纯逻辑
        │   │   ├── configTransferView.test.ts   # 迁移视图测试
        │   │   ├── configTransferView.ts        # 迁移视图纯逻辑
        │   │   ├── dragSortView.test.ts         # 拖拽排序逻辑测试
        │   │   ├── dragSortView.ts              # 拖拽排序几何纯逻辑
        │   │   ├── EditDialog.tsx               # 跨端添加编辑页
        │   │   ├── editDialogView.test.ts       # 保存键策略测试
        │   │   ├── editDialogView.ts            # 编辑弹窗保存键策略纯函数
        │   │   ├── guideDocs.test.ts            # 指引资产收集与语言选档测试
        │   │   ├── guideDocs.ts                 # 指引文档与图片资产收集（双语）
        │   │   ├── guideMd.test.ts              # 指引解析器契约测试
        │   │   ├── guideMd.ts                   # 指引 Markdown 子集解析器
        │   │   ├── GuideViewer.tsx              # 配置指引渲染组件
        │   │   ├── HoverPanel.tsx               # 托盘悬停浮窗
        │   │   ├── hoverPanelView.test.ts       # 悬停面板测试
        │   │   ├── hoverPanelView.ts            # 悬停面板纯逻辑
        │   │   ├── inlineMd.test.ts             # 行内 Markdown 解析测试
        │   │   ├── inlineMd.ts                  # 行内 Markdown 解析纯函数
        │   │   ├── MainPanelTabs.tsx            # 页签与鼠标聚光
        │   │   ├── mainPanelTabsView.test.ts    # 聚光视图测试
        │   │   ├── mainPanelTabsView.ts         # 聚光视图纯逻辑
        │   │   ├── MessageCenter.tsx            # 标题栏铃铛消息中心
        │   │   ├── messageCenterView.test.ts    # 消息中心逻辑测试
        │   │   ├── messageCenterView.ts         # 消息中心纯逻辑
        │   │   ├── MobileChrome.tsx             # 移动端应用壳组件
        │   │   ├── nativeProviderGroups.test.ts # 平台分组测试
        │   │   ├── nativeProviderGroups.ts      # 平台分组纯逻辑
        │   │   ├── NativeProviderPicker.tsx     # 跨端平台聚合选择器
        │   │   ├── PortableInitGate.tsx         # 便携首启确认页
        │   │   ├── presetTemplates.test.ts      # 预设库测试
        │   │   ├── presetTemplates.ts           # 模板预设库
        │   │   ├── pricingDraft.test.ts         # 定价草稿测试
        │   │   ├── pricingDraft.ts              # 定价草稿纯逻辑
        │   │   ├── PricingProvenance.tsx        # 官方模型资料披露
        │   │   ├── PricingSection.tsx           # 峰谷编辑区块
        │   │   ├── ProviderCard.test.tsx        # 卡片定价渲染测试
        │   │   ├── ProviderCard.tsx             # 余额卡片
        │   │   ├── providerCardView.test.ts     # 卡片视图测试
        │   │   ├── providerCardView.ts          # 卡片视图纯逻辑
        │   │   ├── providerIcon.test.ts         # 图标映射测试
        │   │   ├── providerIcon.ts              # Provider 图标映射
        │   │   ├── providerPricing.test.ts      # 定价镜像测试
        │   │   ├── providerPricing.ts           # 前端定价解析镜像
        │   │   ├── SettingsDialog.tsx           # 跨端设置页
        │   │   ├── settingsView.test.ts         # 设置视图测试
        │   │   ├── settingsView.ts              # 设置视图纯逻辑
        │   │   ├── TemplateHelpCard.tsx         # 模板说明折叠卡
        │   │   ├── TitleBar.tsx                 # 自定义标题栏
        │   │   ├── TransferExportDialog.tsx     # 双档导出模态窗
        │   │   ├── transferExportView.test.ts   # 导出模态测试
        │   │   ├── transferExportView.ts        # 导出模态纯逻辑
        │   │   ├── TransferImportDialog.tsx     # 双模导入模态窗
        │   │   ├── transferImportView.test.ts   # 导入模态测试
        │   │   ├── transferImportView.ts        # 导入模态纯逻辑
        │   │   ├── ui.tsx                       # 跨端共享基础组件
        │   │   ├── usageChartView.test.ts       # 统计图表逻辑测试
        │   │   ├── usageChartView.ts            # 统计图表纯逻辑
        │   │   ├── UsageComparisonDialog.tsx    # 统计组合添加弹窗
        │   │   ├── usageComparisonView.test.ts  # 统计组合逻辑测试
        │   │   ├── usageComparisonView.ts       # 统计组合纯逻辑
        │   │   ├── usageLegendView.test.ts      # 聚焦组合浮层测试
        │   │   ├── usageLegendView.ts           # 聚焦组合浮层纯逻辑
        │   │   └── UsageStatsPage.tsx           # 跨端使用统计页
        │   ├── display.test.ts         # display 文案测试
        │   ├── display.ts              # 时间与百分比文案
        │   ├── i18n/                   # 轻量自写 i18n
        │   │   ├── en.ts     # 英文字典（编译锁键）
        │   │   ├── index.tsx # LangProvider 与 t()
        │   │   └── zh.ts     # 中文字典（类型基准）
        │   ├── index.css               # 跨端令牌与全局样式
        │   ├── main.tsx                # React 入口
        │   ├── mainPanelView.test.ts   # 面板切换测试
        │   ├── mainPanelView.ts        # 面板切换状态机
        │   ├── queries.test.ts         # queries hooks 测试
        │   ├── queries.ts              # React Query hooks
        │   ├── runtimeView.test.ts     # 跨端界面策略测试
        │   ├── runtimeView.ts          # 跨端界面能力策略
        │   ├── theme.tsx               # ThemeProvider 三态
        │   ├── themeTransition.test.ts # 扩散动效测试
        │   ├── themeTransition.ts      # 主题扩散动效
        │   ├── types.ts                # 跨端IPC类型镜像
        │   ├── useCardDragSort.ts      # 卡片拖拽排序状态机
        │   └── vite-env.d.ts           # Vite 资源类型声明
        ├── src-tauri/          # Tauri Rust 后端
        │   ├── assets/                 # 原生桌面嵌入资源
        │   │   └── tray-font/ # 托盘数字字体资源
        │   │       ├── OFL.txt                      # 数字字体开源许可证
        │   │       ├── QuotaTrayTrayDigits-Bold.ttf # 托盘粗体数字子集
        │   │       └── README.md                    # 字体来源与子集制作说明
        │   ├── build.rs                # Tauri构建脚本
        │   ├── build_support.rs        # CLI产物路径纯函数
        │   ├── capabilities/           # 权限 ACL
        │   │   ├── default.json     # 主窗 ACL
        │   │   ├── hover-panel.json # 悬停窗 ACL
        │   │   └── mobile.json      # Android主窗ACL
        │   ├── Cargo.toml              # 桌面端 crate 清单
        │   ├── examples/               # 示例注入器
        │   │   └── smoke_setup.rs # GUI 冒烟注入器
        │   ├── icons/                  # 应用图标集
        │   ├── src/                    # 后端源码
        │   │   ├── alert_state.rs          # 提醒状态跨重启持久化
        │   │   ├── apk_install.rs          # APK安装JNI桥
        │   │   ├── background.rs           # Android 后台刷新编排核
        │   │   ├── catalog_sched.rs        # 跨端目录前台调度
        │   │   ├── commands.rs             # 跨端IPC命令集
        │   │   ├── hover_panel.rs          # 悬停窗口状态机
        │   │   ├── hover_panel_mobile.rs   # 移动悬停面板空实现
        │   │   ├── i18n.rs                 # 托盘/命令双语文案
        │   │   ├── lib.rs                  # 跨端Tauri装配
        │   │   ├── main.rs                 # 薄壳入口
        │   │   ├── notification_android.rs # 通知设置页JNI桥
        │   │   ├── ring.rs                 # 托盘细环大字渲染
        │   │   ├── settings.rs             # settings.json 读写
        │   │   ├── snapshot.rs             # cache.json 快照
        │   │   ├── state.rs                # AppState
        │   │   ├── tray.rs                 # 托盘菜单与图标
        │   │   ├── tray_mobile.rs          # 移动托盘空实现
        │   │   └── update_ctl.rs           # 更新检测控制
        │   ├── tauri.android.conf.json # Android Tauri配置
        │   ├── tauri.conf.json         # Tauri配置
        │   ├── tauri.windows.conf.json # Windows Tauri 覆盖配置
        │   └── tests/                  # 构建逻辑测试目录
        │       └── build_support.rs # CLI路径契约测试
        ├── tsconfig.json       # TS 编译配置
        └── vite.config.ts      # Vite 配置
<!-- file-tree:tree^id=apps-desktop:end -->
```
