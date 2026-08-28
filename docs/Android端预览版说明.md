# Android 端预览版说明

## 定位

Android 端首期按 **Preview** 发布。它复用 QuotaTray 的 Rust core、配置格式和查询能力，
但不是桌面窗口的等比缩小版：移动端采用触摸优先壳层，并明确裁掉托盘、悬停窗、自启动和
桌面安装包更新。

当前实现已在 API 36 的 Pixel 8 `x86_64` 模拟器上通过 ARM64 转译运行验收，包括冷启动、
底部导航、全屏设置与返回键、平台选择、卡片点击展开及 Android Keystore 重启持久化。
模拟器不能替代厂商 WebView、系统文件选择器和真实设备 Keystore 的兼容性验收；完成真实
Android 设备的完整运行验收前，不得宣称稳定支持。

## 首期能力

| 能力 | Android Preview |
| --- | --- |
| 预置、模板、脚本 Provider | 支持新增、编辑、试查和查询；依赖桌面 CLI 登录文件的四项预置除外 |
| 账户卡与历史统计 | 支持；仅应用在前台时轮询刷新 |
| 配置导入/导出 | 支持 Android Storage Access Framework 文档 URI |
| 凭据保护 | Android Keystore 保护主密钥；配置仅存 AES-GCM 密文 |
| 语言、主题、阈值、刷新间隔、代理 | 支持 |
| 托盘、悬停窗、圆环图标 | 不适用 |
| 开机自启、后台定时刷新 | 首期不提供 |
| 桌面安装包自更新、内置 CLI/本地 Agent 求助 | 不提供 |

移动端在接入 Android 专属 APK 资产与下载流程前，后端会确定性拒绝桌面更新检测、下载、
打开下载目录和安装命令；前端隐藏入口不是唯一安全边界，也不会误下 WoA ARM64 zip。

Claude、Codex、Gemini 和 Grok 订阅查询依赖各自桌面 CLI 的本机登录文件，Android 无等价
凭据来源：新增选择器会隐藏这四项；从迁移包带入的存量条目保留但返回明确的确定性错误。

Android 端退出或进入后台后，系统可以挂起或终止 WebView/Rust 进程，因此当前刷新间隔只
承诺前台语义。后续若加入 WorkManager，系统允许的周期与触发时刻也会单独立项，不复用
桌面端的分钟级承诺。

## 安全实现

- workspace 最低 Rust 版本提升至 1.88，并统一迁移到 `keyring-core 1.x`。
- Windows、Linux、Apple 与 Android 分别注册原生 Store，但 core 继续只暴露一个
  `KeyringStore`；`QuotaTray / master-key` 标识不变，Windows 旧主密钥无需迁移。
- Android Store 使用 Keystore 加密 SharedPreferences 中的主密钥材料；`config.json`
  位于应用私有数据目录，凭据字段仍为 `v1:<base64>` AES-GCM 密文。
- Android 工程生成后由 `android-post-init.mjs` 强制关闭系统自动备份，避免只恢复配置
  密文、却没有原设备 Keystore 密钥的不可解状态。
- 同一后处理脚本在 `MainActivity.onCreate` 进入 Tauri 生命周期前初始化 `ndk-context`，
  使 Android Keyring 的 JNI 桥接稳定取得应用 Context；该顺序由契约测试锁定。
- 配置迁移包的读写留在 Rust 侧，通过文件系统插件打开 `content://` 文档描述符；迁移包
  字节不回传 WebView。

## 移动交互

- 账户与使用统计使用底部双导航，添加和设置固定在顶部应用栏。
- 添加/编辑、设置及确认页使用全屏页面，正文独立滚动，底部操作避让系统安全区。
- Provider 卡详情以点击切换 `aria-expanded`；Android 禁止依赖悬停展开。
- 平台聚合选择器由点击切换分组；图表通过触摸按下和拖动选择数据点。
- Android 不渲染伪元素 tooltip；必要解释改为常显文案或点击 disclosure。
- 主要触摸目标不小于 44×44px，卡片排序只从专用把手启动。

详细视觉与交互契约以项目技能
`.agents/skills/frontend-style-spec/references/mobile/` 为唯一事实源。

## 构建与验收

本机准备好 JDK 17、Android SDK 36、Build-Tools 36.0.0 与 NDK 27.2.12479018，并设置
`JAVA_HOME`、`ANDROID_HOME`、`NDK_HOME` 后，在 `apps/quota-desktop` 执行：

```powershell
pnpm install --frozen-lockfile
pnpm android:init
pnpm android:build -- --ci --debug --apk --target aarch64
```

`pnpm android:init` 会生成 `src-tauri/gen/android`，随后自动加固 Manifest 并注入 Android
Keyring JNI 初始化桥。生成目录不作为手写源码维护；CI 每次从配置重新生成，以检查初始化
过程可复现。`android:build` 与 `android:dev` 由仓库脚本根据 `NDK_HOME` 和当前操作系统
自动推导 Bindgen sysroot，并在运行前拒绝非 JDK 17 环境；开发者无需配置额外变量。

PR 的 `android-preview` job 会上传 `QuotaTray-android-arm64-preview-debug` artifact。
真实设备验收至少覆盖：首次建库、重启解密、三类 Provider 查询、前后台切换、触摸展开、
图表拖动、配置导入导出、清空数据、深浅主题和中英文布局。
