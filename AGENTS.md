# QuotaTray

托盘常驻的多平台 AI 账户余额监视器：预置官方平台查询 + 声明式模板/JS 脚本自定义查询，GUI 为薄层，业务核心与 CLI 平级共享。

- 调研基础：cc-switch v3.20.0（见 [docs/CC-Switch调研报告.md](docs/CC-Switch调研报告.md)）
- 设计方案：[docs/项目方案预研.md](docs/项目方案预研.md)

## 设计决策快照

以下决策已由项目所有者确认（2026-08-22），修改需重新确认：

| 决策项 | 结论 | 备选（未采纳） |
|---|---|---|
| 技术栈 | Rust workspace 三端共享：`crates/core` + `apps/cli`（clap）+ `apps/desktop`（Tauri 2 + 托盘） | 异构 GUI sidecar；Go/Node 栈 |
| 凭据加密 | 随机 32 字节主密钥存系统凭据库（keyring crate），凭据字段 AES-GCM 加密后存配置文件 | AES-SIV 确定性加密；凭据直存系统库；DPAPI 整体加密 |
| 自定义查询 | 声明式模板优先（零代码），QuickJS 沙箱脚本兜底复杂场景 | 全 JS 脚本；纯声明式 |
| 目标平台 | Windows 优先，全程使用跨平台库，不为未支持平台花工作量 | 仅 Windows（锁死）；三平台同步支持 |

## 文件树

<!-- 每次提交前核对：文件增减与摘要是否与实际一致 -->

```
QuotaTray/
├── AGENTS.md                  # 本文件：项目规则单一事实源
├── CLAUDE.md                  # @AGENTS.md 导入 + Claude 专属补充
├── Cargo.toml                 # workspace 根：成员、共享依赖版本、release 配置
├── rust-toolchain.toml        # 锁定 stable 工具链
├── .gitignore                 # 含 .DevApiKey.json（本地开发密钥，不入库）
├── .DevApiKey.json.example    # 本地密钥文件模板（真实文件被 ignore）
├── .github/workflows/
│   └── ci.yml                 # CI：Windows/Ubuntu 双矩阵 fmt + clippy + test
├── crates/
│   └── quota-core/            # 业务核心库（无 UI 依赖）
│       └── src/
│           ├── lib.rs         # 模块声明与 re-export
│           ├── model.rs       # UsageData / QueryError 双轨分类，附契约单测
│           ├── vault/         # 凭据保险库
│           │   ├── mod.rs     # Vault 门面：open（取/建主密钥）+ encrypt/decrypt
│           │   ├── cipher.rs  # AES-256-GCM 与 v1: 密文格式（nonce 随机、AAD 绑定）
│           │   └── store.rs   # SecretStore trait + KeyringStore（生产）/ InMemoryStore（测试）
│           ├── config/        # 配置层
│           │   ├── mod.rs     # AppConfig + ProviderEntry（原子写、密文落盘）
│           │   └── provider.rs# Credentials / ProviderKind（serde tag 分派）
│           ├── http/          # HTTP 抽象
│           │   ├── mod.rs     # HttpClient trait + 请求/响应/错误类型（Debug 打码）
│           │   └── reqwest.rs # 生产实现（rustls，超时映射 Network/Timeout/InvalidRequest）
│           ├── provider/      # 预置平台
│           │   ├── mod.rs     # NativeProvider trait + 注册表 + 解析工具 + MockHttp
│           │   ├── deepseek.rs       # /user/balance
│           │   ├── siliconflow.rs    # /v1/user/info（CNY）
│           │   └── openrouter.rs     # /api/v1/credits（remaining = credits − usage）
│           ├── template/      # 声明式模板 DSL（M2a）
│           │   ├── mod.rs     # DSL 结构/静态校验/执行器（变量替换、URL 安全、多窗口）
│           │   └── path.rs    # JSONPath 子集（$.a.b[0]，拒绝过滤器/通配符）
│           └── query/
│               └── mod.rs     # QueryEngine：解密→分派（native/template）→超时（15s）
├── apps/
│   └── quota-cli/             # CLI 前端（bin 名 quota，M2b 完成）
│       └── src/
│           ├── main.rs        # clap 定义 10 子命令 + dispatch（dev-smoke 仅 debug）
│           ├── ctx.rs         # Ctx：配置路径 + SecretStore 注入（生产 keyring / 测试内存）
│           ├── exit.rs        # 退出码三分约定（0 全成功 / 1 确定性 / 2 仅瞬时）
│           ├── idgen.rs       # 6 位 Crockford base32 随机 id（无偏映射）
│           ├── io.rs          # 交互薄层：掩码读 key（星号回显、Ctrl+V 剪贴板粘贴、管道分流）、多行 JSON 粘贴
│           ├── render.rs      # comfy-table 表格 + query --json 输出结构（纯函数可测）
│           └── cmd/           # 子命令实现（每命令一模块，handler 收 Ctx）
│               ├── mod.rs     # 子模块声明（devsmoke 仅 debug 编入）
│               ├── list.rs    # 条目列表（表格 / --json providers 数组）
│               ├── query.rs   # 并行查询 + watch 轮询 + 退出码聚合（RouteHttp 全链测试）
│               ├── add.rs     # 交互向导 / --json stdin（拒收 api_key_enc）
│               ├── edit.rs    # 向导（回车保持）+ --enable/--disable 快捷路径
│               ├── remove.rs  # 确认删除（--yes 跳过）
│               ├── setkey.rs  # 隐藏读 key → vault 加密写配置
│               ├── natives.rs # 预置平台表
│               ├── template.rs# template test：静态校验 + 真实试查
│               ├── vault.rs   # vault status：主密钥健康检查
│               └── devsmoke.rs# 仅 debug：读 .DevApiKey.json 走完整链路（原 example 迁入）
└── docs/
    ├── CC-Switch调研报告.md    # cc-switch 代码级调研（技术栈/密钥安全/余额查询）
    ├── 项目方案预研.md         # 架构、凭据安全、查询体系、CLI/GUI 设计与里程碑
    └── specs/
        ├── CLI-spec.md        # quota-cli 规格（M2b）：子命令/退出码/dev-smoke
        └── GUI-spec.md        # quota-desktop 规格（M3）：窗口托盘/IPC/快照持久化
```

（`apps/quota-desktop/` 为规划目录，M3 建立；core 的 script 模块（M4）随里程碑建立）

**并行开发约定**（2026-08-23 起）：core 的 M2 API 面已冻结（M2a 完成）。
CLI（M2b）与 GUI（M3）可双工作树并行开发，共享文件仅 workspace
`Cargo.toml` 与本文件树——先合的 PR 为准，后合的 rebase 更新文件树即可。
core 若需变更公开 API，先单独提 PR 合入再同步两端。

## 工程规范

- 通用行为准则、提交规范（中文、`类型: 简述` + 正文）、发布规范遵循用户全局 AGENTS.md，此处不重复。
- **TDD**：实现功能、修复 BUG 前先添加契约测试；网络相关测试一律 mock（不依赖真实平台 API）。
- **构建与测试**（代码骨架建立后生效）：
  - 全量构建：`cargo build --workspace`
  - 测试：`cargo test --workspace`
  - 桌面端开发：`cargo tauri dev`（于 `apps/desktop`）
- 文档用中文编写。

## 安全红线（凭据处理）

本项目以"凭据不落明文"为差异化设计，以下为硬性红线，违反即 bug：

1. **主密钥与凭据**只允许存在于：系统凭据库（运行时取用）、内存、AES-GCM 密文。任何日志、错误信息、调试输出不得包含明文或密钥材料。
2. **源码零密钥**：不得硬编码任何密钥、盐、派生参数；配置文件中凭据字段必须是密文（`v1:<base64>` 格式，含版本号以便未来算法升级）。
3. **前端/GUI 永不接收明文凭据**：查询由 core 在后端完成，GUI 只展示结果；编辑凭据时走"写入专用"通道（空值 = 保持不变，不回显）。
4. 导出/同步功能若引入，导出前必须显式警告包含密文且主密钥不随行（密文离开本机不可解，这是特性不是缺陷）。

## 术语表

| 术语 | 含义 |
|---|---|
| 主密钥（KEK） | 首次运行随机生成的 32 字节密钥，存系统凭据库，仅用于加解密配置中的凭据字段 |
| 预置平台（native provider） | core 内置 Rust 实现的官方查询（如 DeepSeek、SiliconFlow），随版本发布 |
| 声明式模板（template provider） | JSON 描述的查询配置（URL/头/字段映射/算术），零代码 |
| 脚本查询（script provider） | QuickJS 沙箱内运行的 `{request, extractor}` 脚本，兜底复杂平台 |
| 瞬时失败 / 确定性失败 | 网络抖动类错误（可重试、保留旧值）vs 认证/解析类错误（立即透出） |
| keep-last-good | 查询失败时在时限内继续展示上次成功结果的策略 |
