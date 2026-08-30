# QuotaTray

托盘常驻的多平台 AI 账户余额监视器：预置官方平台查询 + 声明式模板/JS 脚本自定义查询，GUI 为薄层，业务核心与 CLI 平级共享。

- 调研基础：cc-switch v3.20.0（见 [docs/预研文档/2026-08-23 CC-Switch调研报告.md](<docs/预研文档/2026-08-23 CC-Switch调研报告.md>)）
- 设计方案：[docs/预研文档/2026-08-22 项目方案预研.md](<docs/预研文档/2026-08-22 项目方案预研.md>)

## 设计决策快照

以下决策已由项目所有者确认。初始快照形成于 2026-08-22；后续新增决策在决策项中
注明日期。修改既有结论需重新确认：

| 决策项 | 结论 | 备选（未采纳） |
|---|---|---|
| 技术栈 | Rust workspace 三端共享：`crates/core` + `apps/cli`（clap）+ `apps/desktop`（Tauri 2 + 托盘） | 异构 GUI sidecar；Go/Node 栈 |
| 凭据加密 | 随机 32 字节主密钥存系统凭据库（keyring-core + 平台原生 Store），凭据字段 AES-GCM 加密后存配置文件 | AES-SIV 确定性加密；凭据直存系统库；DPAPI 整体加密 |
| 自定义查询 | 声明式模板优先（零代码），QuickJS 沙箱脚本兜底复杂场景 | 全 JS 脚本；纯声明式 |
| 目标平台 | Windows 优先，全程使用跨平台库，不为未支持平台花工作量 | 仅 Windows（锁死）；三平台同步支持 |
| 便携版密钥（2026-08-27） | 采用方案 A：随机 32 字节便携主密钥常驻 `Data/portable.key`，首次创建前显式警告；便携目录保密等级等同明文凭据 | Argon2id 口令派生；便携版不携带凭据 |
| WoA 发布阶段（2026-08-27） | ARM64 资产先按 Preview 发布；Release 与 README 必须显式标注，真实 WoA 完整验收并经所有者重新确认后方可转稳定 | 仅凭交叉编译直接宣称稳定；暂不发布 ARM64 资产 |
| 便携提示呈现（2026-08-27） | GUI 首启确认页正文精简为「为什么 + 不要做什么」两行暗红警示，完整固定提示收进问号图标点击展开（InlineMd 渲染 `**`/反引号，字典值保持文档原文）；便携包内说明中英双 txt；README 与 CLI 保持全文原样 | 正文直排全文（字多无人读，起不到警示效果）；仅中文 txt |
| Android Preview（2026-08-28，所有者确认） | 首期仅承诺前台刷新；底部导航 + 顶部应用栏 + 全屏编辑页；统一使用 keyring-core 1 与四个平台原生 Store，真实设备完整验收前保持 Preview | 直接缩放桌面 UI；盲测即宣称稳定；首期引入常驻前台服务 |
| Android 更新链（2026-08-29，所有者确认） | 手动检测（进页+按钮；常驻轮询记为缺口）+ SAF 保存下载 + 自研薄 JNI 桥拉起系统安装器；不声明自安装权限，content URI 会话内存 | 引第三方 intent 插件（查证 0.1.0/400 下载/停更多年）；纯文案手动安装引导；自动下载 |

**并行开发约定**（2026-08-23 起）：core 的 M2 API 面已冻结（M2a 完成）。
CLI（M2b）与 GUI（M3）双工作树并行开发，共享文件仅 workspace
`Cargo.toml`、CI 与本文件树——先合的 PR 为准，后合的 rebase 更新文件树即可。
core 若需变更公开 API，先单独提 PR 合入再同步两端。
M3 期间 core 的 template/http 曾随桌面端 PR 做错误文案安全修复
（只增 `uses_api_key` 公开函数与测试，不改既有签名）。
M4-a（CLI i18n #4 / GUI 主题+圆环+i18n+标题栏 #5）沿用此约定：
CLI 先合，GUI rebase 后合并同步本文件树；Lang 枚举两端各自实现（core 不动）。

## 移动端能力缺口追踪（Android Preview）

活追踪独立建档：[docs/移动端能力缺口追踪.md](<docs/移动端能力缺口追踪.md>)——
凡合入影响任一条目的移动端变更必须同 PR 更新该文档（能力部分就绪即改写口径，
彻底闭环即移出条目；全部补齐后该文档删除）。现状底稿见
[2026-08-29 安卓缺口调研报告.md](<docs/预研文档/2026-08-29 安卓缺口调研报告.md>)。


## 工程规范

- 通用行为准则、提交规范（中文、`类型: 简述` + 正文）、发布规范遵循用户全局 AGENTS.md，此处不重复。
- **TDD**：实现功能、修复 BUG 前先添加契约测试；网络相关测试一律 mock（不依赖真实平台 API）。
- **最低 Rust 版本**：workspace MSRV 为 1.88；CLI、桌面、WoA 与 Android 工作树需同步
  使用满足该版本的 stable 工具链，依赖升级不得使实际要求高于 workspace 声明。
- **提交前格式化与静态检查（硬门禁）**：Rust 改动先 `cargo fmt --all`，再 `cargo clippy --workspace --all-targets -- -D warnings`（`--all-targets` 含 examples/测试，CI 同口径——漏跑会让 main 编译债拖垮后续所有 PR 的 CI）；前端改动先 `pnpm lint --fix`。CI 的 `cargo fmt --all --check` 作用于全 workspace（2026-08-24 v0.3.2 遗留三处未格式化、2026-08-25 v0.4.2 后三处 clippy 失败即为此例）。
  交叉 lint（2026-08-29 审查轮闭环）：host clippy 不编译 android/
  桌面 cfg 分叉的另一半，CI android-preview job 已加
  `cargo clippy -p quota-desktop --all-targets --target
  aarch64-linux-android -- -D warnings`（NDK CC/AR/sysroot env 就地配置，
  build.rs 的 C 依赖所需）。桌面/移动分叉的代码（cfg 门禁的方法、
  模块替身）改动时两半都要过；本地复跑可用同命令 + NDK env（参数
  见 ci.yml），无 NDK 环境时以 CI 为准。
- **Git hooks 本地门禁（两层）**：仓库内 `.githooks/` 提供按检查代价分层的钩子——
  `pre-commit`（秒级：`cargo fmt --all --check` + 前端 `pnpm lint`，按暂存文件按需触发）与
  `pre-push`（分钟级：`cargo clippy --workspace --all-targets -- -D warnings` + 前端
  `tsc --noEmit`，按推送区间按需触发，对应 PR 前最后防线）。hooks 需本机
  `core.hooksPath` 指向 `.githooks` 才生效，配置接口为仓库根 `setup-hooks.cmd`
  （幂等可重复执行；Unix/macOS 等价 `git config core.hooksPath .githooks`）。
  - **配置时机由人类决定**：仅当人类明确要求配置（如"这是第一次，配置一下 hooks"）时
    才执行 setup；Agent 不得主动配置、不得在会话中探测 hooks 是否已配置或建议配置。
  - hooks 是兜底而非替代：本机未配置 hooks 不免除上述手动硬门禁的执行义务。
  - Agent 会话禁止用 `--no-verify` / `-n` 绕过（人类紧急情况自行判断）。
- **构建与测试**：
  - 全量构建检查：`cargo build --workspace`（仅编译校验）
  - 测试：`cargo test --workspace`
  - 前端：`pnpm lint` / `pnpm build`（于 `apps/quota-desktop`，build 含 tsc 检查）
  - 桌面端开发：`pnpm desktop:dev`（于 `apps/quota-desktop`；`scripts/dev.mjs` 自
    1420 起对 v4/v6 双栈试绑，顺延避让 WinNAT/Hyper-V（WSL2/Docker 触发）动态圈占的
    排除端口段与已占端口——两者分别
    报 EACCES/EADDRINUSE，默认顺延上限 500，可用 `QUOTA_DEV_PORT_BASE/SPAN` 调节；
    选定端口经 `--config` 内联 JSON 覆盖 tauri `devUrl` 并以 `QUOTA_DEV_PORT`
    同步 vite。裸 `pnpm tauri dev` 仍可用但无避让。生产实例在跑时 dev 实例会被
    single-instance 弹退，先退出常驻 QuotaTray 再起 dev）
  - 桌面端产物：`pnpm tauri build --no-bundle`（出裸 exe；完整打包 M4）
  - ⚠️ 裸 `cargo build`（含 --release）的桌面端产物指向 devUrl（1420），
    无 vite dev server 时窗口空白——运行/分发一律走 tauri CLI
  - GUI 冒烟：`cargo run -p quota-desktop --example smoke_setup -- --data-dir <沙箱>
    --key-file <.DevApiKey.json>` 注入后以 `--data-dir` 启动 exe 验证
  - 开发目录清理：仓库根执行 `.\clean 1|2|3`；先预览用 `.\clean 3 -WhatIf`
  - 清理器契约测试：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/clean.tests.ps1`
- 文档用中文编写。

## 发布惯例

本章是本项目对通用发布规范的附加门禁。生成 Release notes、README 下载说明和发布
资产时必须同时遵守；固定文本不得缩写、改写或仅以链接代替。

### 基础产物

- 每个 Release 必须附带桌面端 x64 安装包与便携 zip：先把 workspace `Cargo.toml`
  版本号改为目标版本，再于仓库根运行 `.\package`（内部执行 `pnpm tauri build` 并
  组装全部资产）；上传 `target/release/bundle/nsis/*-setup.exe` 与
  `target/release/dist/*-portable.zip`。
- **Android 资产口径**（2026-08-29 更新，签名链已就绪）：推纯三段版本 tag
  （`vX.Y.Z`；含 `-` 后缀的预发布/测试 tag 在 job 层不进入构建，其他非三段
  tag 在断言步失败）后 `android-release.yml` 自动构建固定
  密钥签名的 APK（`QuotaTray_<版本>_android-arm64.apk`），Release 不存在时创建
  草稿并上传；正式发布时人工对同一 tag 执行 `gh release edit` 补全 notes，桌面
  资产经 `gh release upload` 上传（Android 资产由 CI 注入，桌面资产仍走本地
  `.\package` 打包）。Release notes 与 README 提及 Android 端时必须注明 Preview
  状态（真实设备完整验收未完成）。
  versionCode 实际由 workspace `Cargo.toml` 版本派生（`android-tauri.mjs` 经
  `--config` 注入，tauri-cli 按公式 `major*1000000 + minor*1000 + patch` 写入
  `tauri.properties`）；CI 断言该值等于 tag 派生值，因此**推 tag 前必须先 bump
  workspace 版本**，不一致时构建在断言步失败。段位约束：minor 与 patch 不得达到
  1000（与高位版本碰撞），major 不得超过 2147（Android versionCode 为 int32）。
  本项目不发 pre-release tag：tauri 派生忽略 pre 段，rc 与正式版会同
  versionCode，覆盖升级语义无法区分。
- 打包脚本已验证包内 GUI/CLI 的 PE 架构与资产名称一致（`scripts/package.ps1`
  逐 exe 断言 Machine 字段，契约测试 `scripts/package.tests.ps1`）；更新选择
  不得跨架构、跨安装/便携形态回退（core 资产选择器已实现精确匹配）。

### ARM64 Preview 声明（Windows on ARM）

- 本节条款仅约束 **WoA 资产**（`*-arm64-preview*.zip` 与 NSIS）；Android APK 资产
  是独立维度，不触发本节声明，其 Preview 口径见「Android 资产口径」与 Android
  Preview 声明。
- 在真实 WoA 完整验收并经项目所有者重新确认前，所有 WoA ARM64 资产名必须含
  `preview`，README 下载项必须写作“ARM64（预览版）”。
- 只要本次 Release 包含 WoA ARM64 资产，Release notes 与 README 下载节都必须原样包含：

> 🧪 **ARM64 预览版**：ARM64 构建已通过交叉编译与产物架构检查，但尚未完成真实 Windows on ARM 设备的完整运行验收。该资产仅供预览和反馈，不应视为稳定支持。

### Android Preview 声明

- 只要本次 Release 包含 Android APK 资产，Release notes 与 README 下载节都必须
  原样包含下段固定文本（与 README.md 的 Android 小节声明逐字一致，README.en.md
  对应英文版）：

> 🧪 **Android 预览版**：Android 端已在模拟器完成冒烟验收，但尚未完成真实设备的完整运行验收。该资产仅供预览和反馈，不应视为稳定支持。

### Portable 固定安全提示

- 从首次提供 Portable 资产起，**每个 Release** 的 notes 都必须在完整 CHANGELOG 内容
  之后原样追加下段文本；即使该版本未修改 Portable 功能，也不得省略。
- README 的 Portable 下载说明与便携包内说明（中文 `便携版说明.txt` 原样中文、
  英文 `PORTABLE-README.txt` 内固定提示与 README.en.md 逐字一致）必须原样展示
  下段文本。GUI 首启确认页为唯一例外（2026-08-27 所有者确认）：正文精简为核心
  警示两行，完整原文收进问号图标点击展开（悬停展开因卡片居中重排引发闪烁
  回路而弃用；文案键 `portable.noticeFull` 的值仍为下段原文），「取得显式
  确认」的要求不变：

> ⚠️ **便携版安全提示**：便携版会将用于解密凭据的主密钥保存在 `Data/portable.key`。虽然配置中的凭据仍以 AES-GCM 密文存储，但密钥与密文位于同一便携目录，因此整个 `Data/` 目录的保密级别等同明文凭据。请勿将其上传网盘、提交版本库或交给他人；若存储介质遗失或目录泄露，请立即轮换其中使用的全部 API Key。

- 使用 `gh release create --notes` 时，notes 顺序为：版本 CHANGELOG 完整内容 → Portable
  固定安全提示 → ARM64 Preview 声明（本次含 WoA ARM64 资产时）→ Android Preview
  声明（本次含 APK 资产时）。

## 安全红线（凭据处理）

本项目以"凭据不落明文"为差异化设计，以下为硬性红线，违反即 bug：

1. **主密钥与凭据的允许位置**：安装版主密钥只存系统凭据库与内存；已确认的
   Portable 例外允许便携主密钥常驻 `Data/portable.key`；经项目所有者 2026-08-28
   确认，Android 系统凭据库具体为 Keystore 加密的应用私有 SharedPreferences，且关闭
   系统自动备份；凭据明文只允许短暂存在于
   内存，持久化配置中必须是 AES-GCM 密文。任何日志、错误信息、调试输出不得包含
   凭据明文或密钥材料。
2. **源码零密钥**：不得硬编码任何密钥、盐、派生参数；配置文件中凭据字段必须是密文（`v1:<base64>` 格式，含版本号以便未来算法升级）。
3. **前端/GUI 永不接收明文凭据**：查询由 core 在后端完成，GUI 只展示结果；编辑凭据时走"写入专用"通道（空值 = 保持不变，不回显）。
4. **机器主密钥永不导出**。普通 `config.json` 不含任何解密能力，离开本机不可解；显式生成的 `.qtray-export` 迁移包例外携带每次导出新生成的一次性迁移密钥，敏感级别等同明文凭据。CLI/GUI 接入导出时必须在写文件前显式警告并建议用户迁移后删除。
5. **Portable 是受控安全例外**：`portable.key` 与配置密文同目录，整个 `Data/` 的
   保密等级等同明文凭据。首次创建前必须显示“发布惯例”中的固定安全提示并取得显式
   确认（GUI 确认页按发布惯例 2026-08-27 例外口径精简呈现，显式确认要求不变）；
   FAT/exFAT/NTFS 文件权限均不得作为安全承诺。Release、README 与便携包说明
   必须持续携带同一固定提示。

## 外部接口停用追踪（止血备忘）

- **SiliconFlow 国内站 `/v1/user/info` 已废弃**（issue #50）：官方于 2026-08-14
  停止服务（HTTP 410 / code 20092），替代 API 尚未发布。core provider 已做止血
  特判——仅国内站将 410 转译为「接口已停止服务、不代表 API Key 无效」的确定性
  错误；国际站保持通用 HTTP 错误路径。后续关注官方更新公告
  （docs.siliconflow.cn/cn/release-notes/overview），替代 API 发布后移除特判、
  接入新接口。

## 术语表

| 术语 | 含义 |
|---|---|
| 主密钥（KEK） | 首次运行随机生成的 32 字节密钥，存系统凭据库，仅用于加解密配置中的凭据字段 |
| 便携主密钥 | Portable 方案 A 使用的随机 32 字节主密钥，常驻 `Data/portable.key`；因与配置密文同行，整个便携数据目录保密等级等同明文凭据 |
| 一次性迁移密钥 | 每次导出随机生成的 32 字节密钥；源凭据先转写到该密钥，密钥随 `.qtray-export` 包携带，导入后再转写到目标机器主密钥 |
| 配置迁移包 | QuotaTray 私有、带版本和认证校验的二进制配置导出；虽然不可直接阅读，但因携带迁移密钥，保密级别等同明文凭据 |
| 预置平台（native provider） | core 内置 Rust 实现的官方查询（如 DeepSeek、SiliconFlow），随版本发布 |
| 声明式模板（template provider） | JSON 描述的查询配置（URL/头/字段映射/算术），零代码 |
| 第二凭据槽（apiKey2） | 模板/脚本可用的第二个加密凭据变量 `{{apiKey2}}`（如 new-api 系站点的用户 ID，注入 `New-Api-User` 头）；与主 key 同 vault 加密、同「空=保持不变」写入语义 |
| 脚本查询（script provider） | QuickJS 沙箱内运行的 `{request, extractor}` 脚本，兜底复杂平台 |
| 瞬时失败 / 确定性失败 | 网络抖动类错误（可重试、保留旧值）vs 认证/解析类错误（立即透出） |
| keep-last-good | 查询失败时在时限内继续展示上次成功结果的策略 |
| 峰谷定价 | 按「周几+时间段」划分高峰/空闲时段并配两档三价（缓存命中/未命中/输出，每 MTokens）的展示配置：预置随版本内置（DeepSeek），条目可字段级自定义（空=回退预置） |
| 历史库 | `~/.quotatray/history.db`（SQLite）：每次成功查询的余额/额度快照时序表，滚动保留 30 天，schema 走 user_version 版本化迁移 |
| 窗口键（window_key） | 历史行的窗口标识：`plan_name` 非空取之，否则回退序数 `w0/w1…`；同一多窗口条目每窗口一条时间线 |
| 迁移容器 v2 | `.qtray-export` 第 2 版信封 `{config, history}`：随配置携带历史数值行（幂等合并进目标机历史库）；v1 旧包仍可导入，旧二进制读 v2 拒绝 |

## 文件树（简版速览）

```
<!-- file-tree:tree:begin 由脚本渲染，禁止手改 -->
QuotaTray/
├── .agents/                # Agent 技能库（项目级）
│   └── skills/ # 技能目录
│       ├── file-tree/           # 文件树技能
│       │   ├── agents/   # Codex 元数据目录
│       │   │   └── openai.yaml # Codex 技能元数据
│       │   ├── scripts/  # 技能脚本
│       │   │   ├── tree_tool.py      # 文件树唯一维护脚本
│       │   │   └── tree_tool_test.py # 脚本契约测试
│       │   ├── SKILL.md  # 技能主入口
│       │   └── tree.json # 文件树唯一数据源
│       └── frontend-style-spec/ # 前端样式规范技能
│           ├── agents/     # Codex 元数据目录
│           │   └── openai.yaml # Codex 技能元数据
│           ├── references/ # 规范正文目录
│           │   ├── common-ui/     # 通用 UI 组件域
│           │   │   ├── button.md          # 按钮规范
│           │   │   ├── empty-state.md     # 空态卡规范
│           │   │   ├── feedback-banner.md # 反馈块规范
│           │   │   ├── field.md           # 表单字段规范
│           │   │   ├── focus.md           # 焦点环规范
│           │   │   ├── message-center.md  # 消息中心组件规范
│           │   │   ├── segmented.md       # 分段控件规范
│           │   │   └── tooltip.md         # 悬停气泡组件规范
│           │   ├── design-tokens/ # 设计令牌域目录
│           │   │   └── tokens.md # 设计令牌规范
│           │   ├── edit-dialog/   # 编辑弹窗域目录
│           │   │   └── pricing-section.md # 定价编辑区规范
│           │   └── mobile/        # 移动端规范域
│           │       ├── interaction.md # 移动触摸交互规范
│           │       └── layout.md      # 移动壳层布局规范
│           └── SKILL.md    # 跨端前端规范索引
├── .DevApiKey.json.example # 本地密钥文件模板
├── .gitattributes          # 行尾规则（技能 LF）
├── .githooks/              # Git hooks 本地门禁
│   ├── pre-commit # 提交级轻量门禁（fmt+前端lint）
│   └── pre-push   # 推送级重门禁（clippy+tsc）
├── .github/                # GitHub 配置
│   └── workflows/ # CI 工作流
│       ├── android-release.yml # Android签名发布链
│       └── ci.yml              # 桌面与Android CI
├── .gitignore              # 忽略清单（密钥/生成物）
├── AGENTS.md               # 项目规则单一事实源
├── apps/                   # 应用层（CLI 与桌面端）
│   ├── quota-cli/     # CLI 前端（bin 名 quota）
│   │   ├── Cargo.toml # CLI crate 清单
│   │   └── src/       # CLI 源码
│   │       ├── cmd/           # 子命令实现（每命令一模块）
│   │       │   ├── add.rs            # 交互添加向导
│   │       │   ├── assist.rs         # Agent 无凭据调试
│   │       │   ├── clear.rs          # 清空全部用户数据命令
│   │       │   ├── config.rs         # 配置导入导出
│   │       │   ├── devsmoke.rs       # 开发冒烟（仅 debug）
│   │       │   ├── edit.rs           # 编辑向导与启停
│   │       │   ├── history.rs        # history 命令（M5）
│   │       │   ├── list.rs           # 条目列表
│   │       │   ├── mod.rs            # 子模块声明
│   │       │   ├── natives.rs        # 预置平台表
│   │       │   ├── pricing.rs        # 定价查看/写入
│   │       │   ├── pricing_models.rs # 自定义模型库管理
│   │       │   ├── query.rs          # 并行查询与 watch
│   │       │   ├── remove.rs         # 确认删除
│   │       │   ├── script.rs         # 脚本试查
│   │       │   ├── setkey.rs         # 写入 API key
│   │       │   ├── template.rs       # 模板试查
│   │       │   ├── update.rs         # 更新检测/下载命令
│   │       │   └── vault.rs          # vault 健康检查
│   │       ├── ctx.rs         # CLI 上下文
│   │       ├── exit.rs        # 退出码三分约定
│   │       ├── idgen.rs       # 随机 id 生成
│   │       ├── io.rs          # 交互 IO 薄层
│   │       ├── lang.rs        # 语言三态与检测
│   │       ├── main.rs        # clap 定义与 dispatch
│   │       ├── render.rs      # 表格与 JSON 渲染
│   │       ├── settings_io.rs # settings.json 读取
│   │       └── texts.rs       # 双语文案表
│   └── quota-desktop/ # 桌面端（M3 完成）
│       ├── eslint.config.js    # ESLint 扁平配置
│       ├── index.html          # Vite HTML 入口
│       ├── package.json        # pnpm前端清单
│       ├── pnpm-lock.yaml      # 前端依赖锁文件
│       ├── pnpm-workspace.yaml # pnpm 构建许可
│       ├── scripts/            # 构建辅助脚本目录
│       │   ├── android-post-init.contract.mjs # Android初始化契约测试
│       │   ├── android-post-init.mjs          # Android工程安全初始化
│       │   ├── android-tauri.contract.mjs     # Android构建入口测试
│       │   ├── android-tauri.mjs              # Android构建环境入口
│       │   ├── build-hook.contract.mjs        # 构建钩子契约测试
│       │   ├── build-hook.mjs                 # 跨目标Tauri构建钩子
│       │   ├── dev.contract.mjs               # dev端口探测避让契约测试
│       │   ├── dev.mjs                        # dev端口探测避让入口
│       │   └── mobile-style.contract.mjs      # 移动样式契约测试
│       ├── src/                # React 前端源码
│       │   ├── api.ts                  # invoke 封装
│       │   ├── App.tsx                 # 跨端主界面壳层
│       │   ├── assets/                 # 静态资源
│       │   │   ├── brand-mark.png # 透明品牌主图
│       │   │   └── providers/     # Provider SVG 图标集
│       │   ├── components/             # 前端组件
│       │   │   ├── aiAssistPack.test.ts         # AI 求助包测试
│       │   │   ├── aiAssistPack.ts              # AI 求助包纯逻辑
│       │   │   ├── AiAssistPanel.tsx            # AI 调试求助面板
│       │   │   ├── BrandMark.tsx                # 品牌标志薄组件
│       │   │   ├── ClearConfigDialog.tsx        # 清空配置二级确认弹窗
│       │   │   ├── clearConfigView.test.ts      # 清空确认逻辑测试
│       │   │   ├── clearConfigView.ts           # 清空确认纯逻辑
│       │   │   ├── configTransferView.test.ts   # 迁移视图测试
│       │   │   ├── configTransferView.ts        # 迁移视图纯逻辑
│       │   │   ├── dragSortView.test.ts         # 拖拽排序逻辑测试
│       │   │   ├── dragSortView.ts              # 拖拽排序几何纯逻辑
│       │   │   ├── EditDialog.tsx               # 跨端添加编辑页
│       │   │   ├── HoverPanel.tsx               # 托盘悬停浮窗
│       │   │   ├── hoverPanelView.test.ts       # 悬停面板测试
│       │   │   ├── hoverPanelView.ts            # 悬停面板纯逻辑
│       │   │   ├── inlineMd.test.ts             # 行内 Markdown 解析测试
│       │   │   ├── inlineMd.ts                  # 行内 Markdown 解析纯函数
│       │   │   ├── MainPanelTabs.tsx            # 页签与鼠标聚光
│       │   │   ├── mainPanelTabsView.test.ts    # 聚光视图测试
│       │   │   ├── mainPanelTabsView.ts         # 聚光视图纯逻辑
│       │   │   ├── MessageCenter.tsx            # 标题栏铃铛消息中心
│       │   │   ├── messageCenterView.test.ts    # 消息中心逻辑测试
│       │   │   ├── messageCenterView.ts         # 消息中心纯逻辑
│       │   │   ├── MobileChrome.tsx             # 移动端应用壳组件
│       │   │   ├── nativeProviderGroups.test.ts # 平台分组测试
│       │   │   ├── nativeProviderGroups.ts      # 平台分组纯逻辑
│       │   │   ├── NativeProviderPicker.tsx     # 跨端平台聚合选择器
│       │   │   ├── PortableInitGate.tsx         # 便携首启确认页
│       │   │   ├── presetTemplates.test.ts      # 预设库测试
│       │   │   ├── presetTemplates.ts           # 模板预设库
│       │   │   ├── pricingDraft.test.ts         # 定价草稿测试
│       │   │   ├── pricingDraft.ts              # 定价草稿纯逻辑
│       │   │   ├── PricingSection.tsx           # 峰谷编辑区块
│       │   │   ├── ProviderCard.tsx             # 余额卡片
│       │   │   ├── providerCardView.test.ts     # 卡片视图测试
│       │   │   ├── providerCardView.ts          # 卡片视图纯逻辑
│       │   │   ├── providerIcon.test.ts         # 图标映射测试
│       │   │   ├── providerIcon.ts              # Provider 图标映射
│       │   │   ├── providerPricing.test.ts      # 定价镜像测试
│       │   │   ├── providerPricing.ts           # 前端定价解析镜像
│       │   │   ├── SettingsDialog.tsx           # 跨端设置页
│       │   │   ├── settingsView.test.ts         # 设置视图测试
│       │   │   ├── settingsView.ts              # 设置视图纯逻辑
│       │   │   ├── TemplateHelpCard.tsx         # 模板说明折叠卡
│       │   │   ├── TitleBar.tsx                 # 自定义标题栏
│       │   │   ├── ui.tsx                       # 跨端共享基础组件
│       │   │   ├── usageChartView.test.ts       # 统计图表逻辑测试
│       │   │   ├── usageChartView.ts            # 统计图表纯逻辑
│       │   │   └── UsageStatsPage.tsx           # 跨端使用统计页
│       │   ├── display.test.ts         # display 文案测试
│       │   ├── display.ts              # 时间与百分比文案
│       │   ├── i18n/                   # 轻量自写 i18n
│       │   │   ├── en.ts     # 英文字典（编译锁键）
│       │   │   ├── index.tsx # LangProvider 与 t()
│       │   │   └── zh.ts     # 中文字典（类型基准）
│       │   ├── index.css               # 跨端令牌与全局样式
│       │   ├── main.tsx                # React 入口
│       │   ├── mainPanelView.test.ts   # 面板切换测试
│       │   ├── mainPanelView.ts        # 面板切换状态机
│       │   ├── queries.test.ts         # queries hooks 测试
│       │   ├── queries.ts              # React Query hooks
│       │   ├── runtimeView.test.ts     # 跨端界面策略测试
│       │   ├── runtimeView.ts          # 跨端界面能力策略
│       │   ├── theme.tsx               # ThemeProvider 三态
│       │   ├── themeTransition.test.ts # 扩散动效测试
│       │   ├── themeTransition.ts      # 主题扩散动效
│       │   ├── types.ts                # 跨端IPC类型镜像
│       │   ├── useCardDragSort.ts      # 卡片拖拽排序状态机
│       │   └── vite-env.d.ts           # Vite 资源类型声明
│       ├── src-tauri/          # Tauri Rust 后端
│       │   ├── build.rs                # Tauri构建脚本
│       │   ├── build_support.rs        # CLI产物路径纯函数
│       │   ├── capabilities/           # 权限 ACL
│       │   │   ├── default.json     # 主窗 ACL
│       │   │   ├── hover-panel.json # 悬停窗 ACL
│       │   │   └── mobile.json      # Android主窗ACL
│       │   ├── Cargo.toml              # 桌面端 crate 清单
│       │   ├── examples/               # 示例注入器
│       │   │   └── smoke_setup.rs # GUI 冒烟注入器
│       │   ├── icons/                  # 应用图标集
│       │   ├── src/                    # 后端源码
│       │   │   ├── apk_install.rs          # APK安装JNI桥
│       │   │   ├── background.rs           # Android 后台刷新编排核
│       │   │   ├── commands.rs             # 跨端IPC命令集
│       │   │   ├── hover_panel.rs          # 悬停窗口状态机
│       │   │   ├── hover_panel_mobile.rs   # 移动悬停面板空实现
│       │   │   ├── i18n.rs                 # 托盘/命令双语文案
│       │   │   ├── lib.rs                  # 跨端Tauri装配
│       │   │   ├── main.rs                 # 薄壳入口
│       │   │   ├── notification_android.rs # 通知设置页JNI桥
│       │   │   ├── ring.rs                 # 托盘圆环渲染
│       │   │   ├── settings.rs             # settings.json 读写
│       │   │   ├── snapshot.rs             # cache.json 快照
│       │   │   ├── state.rs                # AppState
│       │   │   ├── tray.rs                 # 托盘菜单与图标
│       │   │   ├── tray_mobile.rs          # 移动托盘空实现
│       │   │   └── update_ctl.rs           # 更新检测控制
│       │   ├── tauri.android.conf.json # Android Tauri配置
│       │   ├── tauri.conf.json         # Tauri配置
│       │   ├── tauri.windows.conf.json # Windows Tauri 覆盖配置
│       │   └── tests/                  # 构建逻辑测试目录
│       │       └── build_support.rs # CLI路径契约测试
│       ├── tsconfig.json       # TS 编译配置
│       └── vite.config.ts      # Vite 配置
├── Cargo.lock              # 依赖锁文件
├── Cargo.toml              # workspace 根配置
├── CHANGELOG.md            # 版本变更记录
├── CLAUDE.md               # AGENTS 导入+专属补充
├── clean.cmd               # 开发目录清理入口
├── crates/                 # workspace crates 根
│   └── quota-core/ # 业务核心库（无 UI）
│       ├── Cargo.toml # core crate 清单
│       └── src/       # core 源码
│           ├── config/    # 配置层
│           │   ├── mod.rs      # AppConfig 原子读写
│           │   ├── provider.rs # 凭据与条目类型
│           │   └── transfer.rs # 配置跨机器迁移
│           ├── history/   # 历史数据存储（M5）
│           │   └── mod.rs # HistoryStore（SQLite）
│           ├── http/      # HTTP 抽象
│           │   ├── mod.rs     # HttpClient trait 与错误
│           │   ├── redact.rs  # 错误详情脱敏
│           │   └── reqwest.rs # reqwest 生产实现
│           ├── lib.rs     # 模块声明与 re-export
│           ├── model.rs   # 用量模型与错误分类
│           ├── pricing.rs # 峰谷定价纯函数
│           ├── provider/  # 预置平台查询
│           │   ├── aliyun_bss.rs    # 阿里云余额查询 provider
│           │   ├── claude.rs        # Claude 订阅查询
│           │   ├── codex.rs         # Codex 订阅查询
│           │   ├── deepseek.rs      # /user/balance 单站双币
│           │   ├── gemini.rs        # Gemini Code Assist
│           │   ├── grok.rs          # Grok 订阅 credits 查询
│           │   ├── kimi.rs          # Kimi 开放平台余额
│           │   ├── kimi_coding.rs   # Kimi Code 用量
│           │   ├── minimax.rs       # MiniMax Coding Plan
│           │   ├── mod.rs           # trait 与注册表（20 项）
│           │   ├── novita.rs        # /v3/user/balance
│           │   ├── openrouter.rs    # /api/v1/credits
│           │   ├── siliconflow.rs   # 硅基流动国内/国际
│           │   ├── stepfun.rs       # /v1/accounts（CNY）
│           │   ├── zhipu.rs         # GLM Coding Plan 用量
│           │   └── zhipu_metered.rs # 智谱按量余额
│           ├── query/     # 查询引擎
│           │   └── mod.rs # QueryEngine 路由
│           ├── runtime.rs # 运行模式纯函数（安装/便携）
│           ├── script/    # 脚本查询（M4）
│           │   └── mod.rs # QuickJS 沙箱脚本查询
│           ├── template/  # 声明式模板 DSL（M2a）
│           │   ├── mod.rs  # DSL 结构与执行器
│           │   └── path.rs # JSONPath 子集
│           ├── update.rs  # 更新检测下载与清理判定
│           └── vault/     # 凭据保险库
│               ├── cipher.rs # AES-256-GCM 密文格式
│               ├── mod.rs    # Vault 门面
│               └── store.rs  # 跨平台主密钥存储
├── docs/                   # 文档
│   ├── Android端预览版说明.md # Android预览端说明
│   ├── design/          # 设计文档
│   │   └── tray-ring-demo.html # 圆环视觉规格
│   ├── guide/           # 用户配置指引（GUI 可渲染）
│   │   └── 阿里云余额监控配置指引.md # 阿里云余额监控用户配置指引
│   ├── specs/           # 规格文档
│   │   ├── CLI-spec.md          # CLI 规格（M2b）
│   │   ├── console-link-spec.md # 控制台直达规格（#59）
│   │   ├── GUI-spec.md          # GUI 规格（M3）
│   │   └── history-spec.md      # 历史存储规格（M5）
│   ├── 测试单/             # 真机端测执行清单目录
│   │   └── 2026-08-29 安卓端端测清单.md # 安卓真机端测清单（更新链/升级/通用）
│   ├── 移动端能力缺口追踪.md     # Android 能力缺口活追踪
│   └── 预研文档/            # 立项前调研与预研报告
│       ├── 2026-08-22 项目方案预研.md         # 项目方案预研
│       ├── 2026-08-23 CC-Switch调研报告.md  # cc-switch 调研
│       ├── 2026-08-25 预置Provider缺口预研.md # 预置缺口预研
│       ├── 2026-08-27 WoA与便携版预研报告.md    # WoA 与便携版预研
│       ├── 2026-08-28 自动更新预研报告.md       # 自动更新静默与双目录预研
│       ├── 2026-08-29 安卓更新与下载预研报告.md    # 安卓更新下载预研
│       ├── 2026-08-29 安卓缺口调研报告.md       # 安卓缺口八项现状盘点（移动端计划底稿）
│       └── 2026-08-30 百炼余额查询预研.md       # 百炼余额查询预研报告
├── examples/               # 可运行示例
│   ├── scripts/   # 脚本查询示例
│   │   ├── basic.js        # 最小闭环脚本示例
│   │   ├── multi-window.js # 多窗口脚本示例
│   │   └── README.md       # 脚本示例说明
│   └── templates/ # 模板示例（5 形态）
│       ├── deepseek.json     # 单对象余额示例
│       ├── multi-window.json # 多窗口示例
│       ├── newapi.json       # NewAPI 中转示例
│       ├── openrouter.json   # 总额已用示例
│       ├── README.md         # 模板示例说明
│       └── siliconflow.json  # 双站 baseUrl 示例
├── LICENSE                 # MIT 许可证全文
├── package.cmd             # 一键打包入口包装器
├── README.en.md            # 英文自述，互链中文
├── README.md               # 中文项目自述
├── rust-toolchain.toml     # 锁定开发与CI工具链
├── scripts/                # 维护脚本
│   ├── clean.ps1         # 分级清理器
│   ├── clean.tests.ps1   # 清理器契约测试
│   ├── package.ps1       # 一键发布资产打包脚本
│   └── package.tests.ps1 # 打包脚本契约测试
└── setup-hooks.cmd         # git hooks 配置入口（幂等）
<!-- file-tree:tree:end -->
```

## 文件树标签词表

<!-- file-tree:tags:begin 由脚本渲染，禁止手改 -->
| 标签 | 说明 |
| --- | --- |
| `deprecated` | 已废弃，计划移除 |
| `generated` | 构建生成物，不手工编辑 |
| `pure` | 纯函数/纯逻辑模块，可直接单测 |
| `security` | 涉及凭据安全红线，改动需对照安全章节 |
| `test` | 测试文件 |
<!-- file-tree:tags:end -->
