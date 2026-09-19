# QuotaTray

托盘常驻的多平台 AI 账户余额监视器：预置官方平台查询 + 声明式模板/JS 脚本自定义查询，GUI 为薄层，业务核心与 CLI 平级共享。

- 调研基础：cc-switch v3.20.0（见 [docs/预研文档/2026-08-23 CC-Switch调研报告.md](docs/预研文档/2026-08-23 CC-Switch调研报告.md)）
- 设计方案：[docs/预研文档/2026-08-22 项目方案预研.md](docs/预研文档/2026-08-22 项目方案预研.md)

## 并行开发约定（2026-08-23 起）

core 的 M2 API 面已冻结（M2a 完成）。
CLI（M2b）与 GUI（M3）双工作树并行开发，共享文件仅 workspace
`Cargo.toml`、CI 与本文件树——先合的 PR 为准，后合的 rebase 更新文件树即可。
core 若需变更公开 API，先单独提 PR 合入再同步两端。
M3 期间 core 的 template/http 曾随桌面端 PR 做错误文案安全修复
（只增 `uses_api_key` 公开函数与测试，不改既有签名）。
M4-a（CLI i18n #4 / GUI 主题+圆环+i18n+标题栏 #5）沿用此约定：
CLI 先合，GUI rebase 后合并同步本文件树；Lang 枚举两端各自实现（core 不动）。

## 移动端能力缺口追踪（Android Preview）

活追踪独立建档：[docs/移动端能力缺口追踪.md](docs/移动端能力缺口追踪.md)——
凡合入影响任一条目的移动端变更必须同 PR 更新该文档（能力部分就绪即改写口径，
彻底闭环即移出条目；全部补齐后该文档删除）。现状底稿见
[2026-08-29 安卓缺口调研报告.md](docs/预研文档/2026-08-29 安卓缺口调研报告.md)。

## Agent 工程技能配置

- **Issue tracker**：任务包（spec + 工单）以 GitHub Issues 为唯一载体——规格发布为父 issue、工单挂 sub-issue、依赖用原生 blocked-by 关系；2026-09-10 起不再落 `.scratch/` 本地任务包（旧包 model-pricing-catalog 保留为历史存档）。详见 [docs/agents/issue-tracker.md](docs/agents/issue-tracker.md)。
- **分诊标签**：五个规范角色用默认字符串（needs-triage / needs-info / ready-for-agent / ready-for-human / wontfix）。详见 [docs/agents/triage-labels.md](docs/agents/triage-labels.md)。
- **领域文档**：单一上下文布局，词汇基准是根 `CONTEXT.md`（Language 词条），设计决策记录在 `docs/adr/`。详见 [docs/agents/domain.md](docs/agents/domain.md)。

## 工程规范

- 通用行为准则、提交规范（中文、`类型: 简述` + 正文）、发布规范遵循用户全局 AGENTS.md，此处不重复。
- **TDD**：实现功能、修复 BUG 前先添加契约测试；网络相关测试一律 mock（不依赖真实平台 API）。
- **最低 Rust 版本**：workspace MSRV 为 1.88；CLI、桌面、WoA 与 Android 工作树需同步
  使用满足该版本的 stable 工具链，依赖升级不得使实际要求高于 workspace 声明。
- **提交前格式化与静态检查（硬门禁）**：Rust 改动先 `cargo fmt --all`，再 `cargo clippy --workspace --all-targets -- -D warnings`（`--all-targets` 含 examples/测试，CI 同口径——漏跑会让 main 编译债拖垮后续所有 PR 的 CI）；前端改动先 `pnpm lint --fix`。CI 的 `cargo fmt --all --check` 作用于全 workspace（2026-08-24 v0.3.2 遗留三处未格式化、2026-08-25 v0.4.2 后三处 clippy 失败即为此例）。
  交叉 lint（2026-08-29 审查轮闭环）：host clippy 不编译 android/
  桌面 cfg 分叉的另一半，CI android-preview job 已加
  `cargo clippy -p quota-desktop --all-targets --target aarch64-linux-android -- -D warnings`（NDK CC/AR/sysroot env 就地配置，
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
  - GUI 冒烟：`cargo run -p quota-desktop --example smoke_setup -- --data-dir <沙箱> --key-file <.DevApiKey.json>` 注入后以 `--data-dir` 启动 exe 验证
  - 开发目录清理：仓库根执行 `.\clean 1|2|3`；先预览用 `.\clean 3 -WhatIf`
  - 清理器契约测试：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/clean.tests.ps1`
- 文档用中文编写。

## 发布惯例

本章正文已外迁技能 `.agents/skills/release/SKILL.md`（渐进式披露），原位留指针：

- **必读触发**：凡 bump workspace 版本、打包发布资产、推发布 tag、编写版本
  CHANGELOG / Release notes、更新 README 下载说明或便携包内说明，动笔前必须
  先读上述技能——Portable 固定安全提示、ARM64 / Android Preview 声明等逐字
  固定文本在技能正文，**不得缩写、改写、凭记忆复述或仅以链接代替**。
- 既有引用（安全红线首启确认页例外口径、core `update.rs` Preview 口径注释）
  继续以本章名为锚点，正文内容以技能为准。

## 安全红线（凭据处理）

本项目以"凭据不落明文"为差异化设计，以下为硬性红线，违反即 bug：

1. **主密钥与凭据的允许位置**：安装版主密钥只存系统凭据库与内存；已确认的
   Portable 例外允许便携主密钥常驻 `Data/portable.key`；经项目所有者 2026-08-28
   确认，Android 系统凭据库具体为 Keystore 加密的应用私有 SharedPreferences，且关闭
   系统自动备份；凭据明文只允许短暂存在于
   内存，**受控例外（2026-09-05 所有者确认）：CLI 凭据型 provider 的进程内快照
   缓存——快照对象为外部 CLI 自管的磁盘明文凭据文件（`~/.codex/auth.json` 等），
   仅驻内存、永不落盘/入日志/入错误信息，用于拦截窗口期回退旧 token 继续查询；
   磁盘原文件本就明文，快照不降低既有安全等级**，
   持久化配置中必须是 AES-GCM 密文。任何日志、错误信息、调试输出不得包含
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

## 文件树（简版速览）

```
<!-- file-tree:tree:begin 由脚本渲染，禁止手改 -->
QuotaTray/
├── .agents/                # Agent 技能库（项目级）
│   └── skills/… # 技能目录
├── .DevApiKey.json.example # 本地密钥文件模板
├── .gitattributes          # 行尾规则（技能 LF）
├── .githooks/              # Git hooks 本地门禁
│   ├── pre-commit # 提交级轻量门禁（fmt+前端lint）
│   └── pre-push   # 推送级重门禁（clippy+tsc）
├── .github/                # GitHub 配置
│   └── workflows/ # CI 工作流
│       ├── android-release.yml # Android签名发布链
│       ├── catalog-data.yml    # 定价目录数据校验工作流（T-07）
│       └── ci.yml              # 桌面与Android CI
├── .gitignore              # 忽略清单（密钥/生成物）
├── AGENTS.md               # 项目规则单一事实源
├── apps/…                  # 应用层子树见 file-subtrees
├── assets/                 # 仓库静态资产目录
│   └── pics/ # README 界面截图与动图
│       ├── 主题切换动效.webp   # 主题切换扩散动效演示
│       ├── 使用统计多曲线比较.png # 使用统计多曲线比较截图
│       └── 托盘悬停浮窗.png    # 托盘悬停浮窗界面截图
├── Cargo.lock              # 依赖锁文件
├── Cargo.toml              # workspace 根配置
├── CHANGELOG.md            # 版本变更记录
├── CLAUDE.md               # AGENTS 导入+专属补充
├── clean.cmd               # 开发目录清理入口
├── CONTEXT.md              # 项目词汇基准（Language 词条）
├── crates/                 # workspace crates 根
│   └── quota-core/ # 业务核心库（无 UI）
│       ├── Cargo.toml # core crate 清单
│       ├── src/       # core 源码
│       │   ├── config/          # 配置层
│       │   │   ├── mod.rs      # AppConfig 原子读写
│       │   │   ├── provider.rs # 凭据与条目类型
│       │   │   └── transfer.rs # 配置迁移容器
│       │   ├── history/         # 历史数据存储（M5）
│       │   │   └── mod.rs # HistoryStore（SQLite）
│       │   ├── http/            # HTTP 抽象
│       │   │   ├── mod.rs     # HttpClient trait 与错误
│       │   │   ├── redact.rs  # 错误详情脱敏
│       │   │   └── reqwest.rs # reqwest 生产实现
│       │   ├── lib.rs           # 模块声明与 re-export
│       │   ├── logging.rs       # 结构化事件打点与滚动日志装配
│       │   ├── model.rs         # 用量模型与错误分类
│       │   ├── pricing.rs       # 峰谷定价纯函数
│       │   ├── pricing_catalog/ # 定价目录模块（T-01）
│       │   │   ├── mod.rs  # 目录类型校验与种子装载
│       │   │   └── sync.rs # 目录同步与持久缓存
│       │   ├── provider/        # 预置平台查询
│       │   │   ├── aliyun_bss.rs    # 阿里云余额查询 provider
│       │   │   ├── claude.rs        # Claude 订阅查询
│       │   │   ├── codex.rs         # Codex 订阅查询
│       │   │   ├── deepseek.rs      # /user/balance 单站双币
│       │   │   ├── gemini.rs        # Gemini Code Assist
│       │   │   ├── grok.rs          # Grok 订阅 credits 查询
│       │   │   ├── kimi.rs          # Kimi 开放平台余额
│       │   │   ├── kimi_coding.rs   # Kimi Code 用量
│       │   │   ├── minimax.rs       # MiniMax Coding Plan
│       │   │   ├── mod.rs           # trait、注册表与共用工具
│       │   │   ├── novita.rs        # /v3/user/balance
│       │   │   ├── openrouter.rs    # /api/v1/credits
│       │   │   ├── siliconflow.rs   # 硅基流动国内/国际
│       │   │   ├── stepfun.rs       # /v1/accounts（CNY）
│       │   │   ├── zhipu.rs         # GLM Coding Plan 用量
│       │   │   └── zhipu_metered.rs # 智谱按量余额
│       │   ├── query/           # 查询引擎
│       │   │   └── mod.rs # QueryEngine 路由
│       │   ├── runtime.rs       # 运行模式纯函数（安装/便携）
│       │   ├── script/          # 脚本查询（M4）
│       │   │   └── mod.rs # QuickJS 沙箱脚本查询
│       │   ├── template/        # 声明式模板 DSL（M2a）
│       │   │   ├── mod.rs  # DSL 结构与执行器
│       │   │   └── path.rs # JSONPath 子集
│       │   ├── update.rs        # 更新检测下载与清理判定
│       │   └── vault/           # 凭据保险库
│       │       ├── cipher.rs # AES-256-GCM 密文格式
│       │       ├── mod.rs    # Vault 门面
│       │       └── store.rs  # 跨平台主密钥存储
│       └── tests/     # core 集成测试目录
│           └── logging_smoke.rs # 滚动日志装配端到端冒烟
├── data/                   # 正式数据文件根
│   └── pricing/ # 定价数据目录
│       └── v1/ # 定价目录 v1 格式
│           └── catalog.json # 预置定价目录数据源
├── docs/                   # 文档
│   ├── adr/…            # 设计决策记录（ADR 一行一档）
│   ├── agents/          # 工程技能配置文档
│   │   ├── domain.md        # 工程技能领域文档消费规则
│   │   ├── issue-tracker.md # 工程技能 issue 约定
│   │   └── triage-labels.md # 分诊标签角色映射表
│   ├── Android端预览版说明.md # Android预览端说明
│   ├── assets/          # 指引打包资产根目录
│   │   └── bundle/ # 随应用打包的指引图片
│   │       └── README.md # 图片资产目录说明
│   ├── design/          # 设计文档
│   │   └── tray-ring-demo.html # 托盘圆环交互演示
│   ├── file-subtrees/   # 文件树子树视图文档目录
│   │   └── apps/ # apps 子树视图
│   │       ├── quota-cli.md     # quota-cli 子树视图
│   │       └── quota-desktop.md # quota-desktop 子树视图
│   ├── guide/           # 平台配置指引（zh/en 语言子目录）
│   │   ├── en/ # 英文版配置指引
│   │   │   └── aliyun-balance-setup-guide.md # 阿里云余额监控配置指引（英文版）
│   │   └── zh/ # 中文版配置指引
│   │       └── aliyun-balance-setup-guide.md # 阿里云余额监控配置指引（中文版）
│   ├── specs/           # 规格文档
│   │   ├── CLI-spec.md          # CLI 规格（M2b）
│   │   ├── console-link-spec.md # 控制台直达规格（#59）
│   │   ├── GUI-spec.md          # GUI 规格（M3）
│   │   └── history-spec.md      # 历史存储规格（M5）
│   ├── 定价目录维护指南.md      # 定价目录数据维护指南
│   ├── 测试单/             # 真机端测执行清单目录
│   │   ├── 2026-08-29 安卓端端测清单.md     # 安卓真机端测清单（更新链/升级/通用）
│   │   └── 2026-09-16 聚焦组合模态窗端测清单.md # 聚焦组合模态窗真机端测清单
│   ├── 移动端能力缺口追踪.md     # Android 能力缺口活追踪
│   └── 预研文档/…           # 立项前调研与预研报告
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
│   ├── fetch_pricing/    # 官网定价确定性抓取脚本集
│   │   ├── fetch_pricing.py # 抓取主入口：路由与一键全抓
│   │   ├── providers/       # 平台抓取组件目录
│   │   │   ├── __init__.py # Provider 协议声明与通用件导出
│   │   │   ├── _http.py    # 共用 HTTP GET 助手
│   │   │   ├── deepseek.py # DeepSeek 中文定价页抓取组件
│   │   │   ├── model.py    # 通用数据结构与异常
│   │   │   ├── zai.py      # Z.ai 国际站定价页抓取组件
│   │   │   └── zhipu.py    # 智谱国内站 app.js 价格解析组件
│   │   └── tests/           # 契约测试与快照
│   │       ├── fetch_pricing.tests.py # 抓取契约测试
│   │       └── fixtures/…             # 官网页 HTML 固化快照
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
