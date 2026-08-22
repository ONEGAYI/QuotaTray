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
├── .gitignore
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
│           │   ├── mod.rs     # HttpClient trait + 请求/响应/错误类型
│           │   └── reqwest.rs # 生产实现（rustls，超时映射 Network/Timeout）
│           ├── provider/      # 预置平台
│           │   ├── mod.rs     # NativeProvider trait + 注册表 + 解析工具 + MockHttp
│           │   ├── deepseek.rs       # /user/balance
│           │   ├── siliconflow.rs    # /v1/user/info（CNY）
│           │   └── openrouter.rs     # /api/v1/credits（remaining = credits − usage）
│           └── query/
│               └── mod.rs     # QueryEngine：解密→分派→超时（默认 15s）
├── apps/
│   └── quota-cli/             # CLI 前端（bin 名 quota，M0 骨架）
│       └── src/
│           └── main.rs        # clap 空壳：--version + 占位输出
└── docs/
    ├── CC-Switch调研报告.md    # cc-switch 代码级调研（技术栈/密钥安全/余额查询）
    └── 项目方案预研.md         # 架构、凭据安全、查询体系、CLI/GUI 设计与里程碑
```

（`apps/quota-desktop/` 为规划目录，M3 建立；core 的 template（M2）/ script（M4）模块随里程碑建立）

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
