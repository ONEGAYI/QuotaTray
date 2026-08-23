# CC Switch 调研报告

> **调研对象**：[farion1231/cc-switch](https://github.com/farion1231/cc-switch) v3.20.0（MIT 协议，浅克隆快照）
> **调研目的**：为 QuotaTray 提供技术参考——技术栈选型、凭据存储安全性、余额查询与自定义脚本的实现机制
> **引用约定**：`文件路径:行号` 均基于 v3.20.0 克隆快照，随上游演进可能失效；本地克隆位于 `D:\CODE\Project\_ForExplore\cc-switch`

---

## 1. 项目概览

cc-switch 定位为 Claude Code、Codex、Gemini CLI 等 8 个 AI 编程工具的"全方位管理工具"：供应商切换、本地代理与故障转移、MCP/Prompts/Skills 管理、用量仪表盘、云同步。余额查询只是其功能矩阵中的一个子集。

对 QuotaTray 而言，值得研究的是三块：整体技术栈、凭据存储方式、以及它已经生产验证过的余额查询双轨机制。

## 2. 技术栈与架构

| 层 | 选型 |
|---|---|
| 前端 | React 18 + TypeScript + Vite 7，shadcn/ui（Radix + Tailwind 3.4），TanStack Query 管理服务端状态，无 Redux/Zustand |
| 后端 | Tauri 2（Rust，edition 2021），分层 `commands/`（Tauri command 层，37 文件）→ `services/`（业务层，40+ 文件）→ `database/`（rusqlite/SQLite） |
| 网络 | reqwest 0.12（余额查询）；axum + hyper（内置本地代理，与余额无关） |
| 脚本引擎 | **rquickjs**（QuickJS 绑定，执行用户自定义用量脚本） |
| 插件 | updater（自动更新）、deep-link、window-state、single-instance |

前后端通信：前端 `src/lib/api/*.ts` 统一封装 `invoke`（约 260 个命令）；反向由 Rust `app.emit()` 推事件（如 `usage-cache-updated`），前端 hook 监听。

### 托盘实现（与 QuotaTray 直接相关）

- 悬停/点击托盘触发 `refresh_all_usage_in_tray`，**10 秒节流**（`src-tauri/src/tray.rs:1055`），并行查询各应用当前供应商的用量。
- 查询结果写入进程内缓存 `UsageCache`（RwLock HashMap，写穿式，不持久化），再格式化为托盘菜单项后缀，如 `· 😊 five_hour 42% weekly 12%`（`tray.rs:332-370`）。
- 关闭窗口仅隐藏进托盘，`ExitRequested` 被拦截以保持后台常驻（`src-tauri/src/lib.rs:1703-1720`）。

## 3. 凭据存储与安全性

### 3.1 存储：全程明文，无 OS 凭据库

这是 cc-switch 明确的设计决定（其 SECURITY.md 声明威胁模型为"本地单用户桌面应用"）。核心事实：

- 所有供应商 API key 以**明文 JSON 存于 `~/.cc-switch/cc-switch.db`（SQLite）的 `providers.settings_config` 列**（写入见 `src-tauri/src/database/dao/providers.rs:241-250`）；`Cargo.toml` 中无任何加密库。
- **不使用** Windows Credential Manager / DPAPI、macOS Keychain、Linux Secret Service 存自己的凭据。唯一涉及 Keychain 的代码是*读取* Claude Code 等 CLI 自己存的凭据用于订阅展示（`src-tauri/src/services/subscription.rs:129-154`）。
- 明文有其功能必然性：cc-switch 的核心职责是把明文密钥**写回**各 CLI 的明文配置文件（`~/.claude/settings.json`、`~/.codex/auth.json` 等，`src-tauri/src/services/provider/live.rs:1242-1406`）——CLI 只认明文，管理者也就无法只存密文。

### 3.2 权限控制

- Unix 侧部分文件强制 0600（Gemini `.env`、`settings.json`、OAuth token 文件）。
- **Windows 侧全部无 ACL 强化**：`atomic_write_with_unix_mode` 的非 Unix 分支直接忽略 mode（`src-tauri/src/config.rs:341-342`），含密钥文件仅靠用户 profile 目录默认 ACL。
- 数据库文件本身两平台均无权限处理。

### 3.3 出口脱敏（工程投入所在，值得学习）

cc-switch 把安全预算花在"凭据离开存储之后的每一条出口"上：

- 后端日志统一清洗：`redact_known_secrets`、`redact_url_for_log` 剥 URL userinfo/query/fragment 与密钥形状字符串（`src-tauri/src/lib.rs:134-229`）；请求/响应体不落日志。
- 前端日志三层脱敏：按属性名 + 值形状（`sk-`/`ghp_`/JWT 等）+ 文本正则；超大 JSON 整体丢弃而非截断（`src/lib/frontendLogger.ts:36-46, 95-201`）。
- API Key 输入框默认 `type="password"`；Deep Link 导入确认框对凭据掩码并检测风险（`src/utils/deeplinkRisk.ts`）。

### 3.4 确认的明文外流点

1. 导出功能产出**完整 SQL dump**（含全部密钥），无加密无提示（`src-tauri/src/database/backup.rs:118-137`）。
2. WebDAV/S3 云同步**上传明文库**，且允许 `http://` 端点（`src-tauri/src/services/webdav_sync.rs:66-72`、`webdav.rs:49-56`）。
3. 编辑供应商的 JSON 模式下密钥界面明文可见（`src/components/providers/ProviderForm.tsx:442`）。

### 3.5 对 QuotaTray 的启示

> **QuotaTray 没有 cc-switch 的"被迫明文"约束**：余额查询只需把 key 放进 HTTP 头，不存在"写回 CLI 明文配置"的需求。因此密钥可以全程待在系统凭据库或以 AES-GCM 密文形式落盘，这是本项目能在安全性上直接超越参考对象的地方。同时，cc-switch 的出口脱敏体系（日志清洗、UI 掩码、错误信息脱敏）应当完整继承。

## 4. 余额查询体系（本报告重点）

### 4.1 双轨架构：7 种模板类型路由

每个供应商的 `meta.usage_script` 携带查询配置，统一入口为 Tauri 命令 `queryProviderUsage`（`src-tauri/src/commands/provider.rs:457-498`），按 `templateType` 分发：

| templateType | 实现 | 覆盖平台 |
|---|---|---|
| `balance` | Rust 原生（`services/balance.rs`） | DeepSeek、StepFun、SiliconFlow、OpenRouter、Novita |
| `token_plan` | Rust 原生（`services/coding_plan.rs`） | Kimi、智谱（个人/团队）、MiniMax、ZenMux、火山方舟 |
| `official_subscription` | Rust 原生（`services/subscription.rs`） | Claude/Codex/Gemini/Grok 订阅额度（读 CLI 的 OAuth token，非 API key） |
| `github_copilot` | Rust 原生（托管 OAuth） | GitHub Copilot |
| `general` / `newapi` / `custom` | QuickJS 脚本（`usage_script.rs`） | 任意平台，零代码改动扩展 |

平台识别按 `base_url` 子串匹配（`detect_provider`）；**前后端各维护一份检测表**，靠注释约定同步（`src/config/codingPlanProviders.ts:2-8` 明示此债）。

### 4.2 原生平台端点速查

预置官方查询实现时的直接参考（完整解析逻辑见对应源文件）：

| 平台 | 端点 | 认证 | 解析要点 | 源码 |
|---|---|---|---|---|
| DeepSeek | `GET api.deepseek.com/user/balance` | Bearer | `balance_infos[].total_balance` | balance.rs:74-146 |
| StepFun | `GET api.stepfun.com/v1/accounts` | Bearer | `balance`（CNY） | balance.rs:152-204 |
| SiliconFlow | `GET api.siliconflow.cn/v1/user/info` | Bearer | `data.totalBalance` | balance.rs:210-281 |
| OpenRouter | `GET openrouter.ai/api/v1/credits` | Bearer | remaining = `total_credits − total_usage` | balance.rs:287-346 |
| Novita | `GET api.novita.ai/v3/user/balance` | Bearer | `availableBalance` **÷10000** = USD | balance.rs:353-410 |
| Kimi | `GET api.kimi.com/coding/v1/usages` | Bearer | `limits[]`（5h）+ `usage`（周）双窗口 | coding_plan.rs:105-209 |
| 智谱 | `GET {base}/api/monitor/usage/quota/limit` | **裸 key，无 Bearer** | `data.limits[].percentage` 已是已用%；团队版加 `bigmodel-organization/project` 头 | coding_plan.rs:247-411 |
| MiniMax | `GET api.minimaxi.com/v1/api/openplatform/coding_plan/remains` | Bearer | `model_remains[].current_interval_remaining_percent` | coding_plan.rs:415-496 |
| 火山方舟 | `POST open.volcengineapi.com`（GetAFPUsage） | **V4 AK/SK 签名**（非推理 key） | `Result.AFPFiveHour.{Quota,Used,ResetTime}` | coding_plan.rs:702-1175 |
| NewAPI 系中转 | `GET {base}/api/user/self` | Bearer accessToken + `New-Api-User` 头 | `quota / 500000` = USD | UsageScriptModal.tsx:91-117 |

> **Kimi 字段更新（2026-08-23）**：MoonshotAI 官方 `kimi-code` 已将当前响应契约收紧为周窗口 `usage.used/limit/resetTime`，以及 5 小时窗口 `limits[].detail.used/limit/resetTime`（窗口由 `duration=300`、`timeUnit=TIME_UNIT_MINUTE` 识别）。响应没有 `remaining`，需由 `limit - used` 计算；`resetTime` 为 RFC3339 字符串。cc-switch v3.20.0 的 `remaining` 读取方式不应直接移植。

订阅类（Claude/Codex/Gemini）走各 CLI 本地 OAuth 文件取 token，端点分别为 `api.anthropic.com/api/oauth/usage`、`chatgpt.com/backend-api/wham/usage`、`cloudcode-pa.googleapis.com` 两段式（subscription.rs:344-1184）。

### 4.3 JS 脚本协议（QuotaTray 脚本兜底的蓝本）

脚本是一个返回配置对象的 JS 表达式，**分两阶段执行**（`src-tauri/src/usage_script.rs`）：

```js
({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: { "Authorization": "Bearer {{apiKey}}" }
  },
  extractor: function(response) {
    return { remaining: response.balance, unit: "CNY" };
  }
})
```

执行流程的六个环节，每一环都有值得照搬的设计：

1. **变量替换**：`{{apiKey}}` 等凭据在代码字符串层面替换注入——脚本作者看不到真实凭据，脚本文件可安全分享。
2. **第一次 eval**：QuickJS 沙箱（内存 16MiB、栈 256KiB、5 秒 CPU 中断器防死循环）只产出 `request` 描述对象。**沙箱内没有网络与文件系统**。
3. **URL 安全校验**：强制 HTTPS（loopback 除外）；非 `custom` 模板强制与供应商 `base_url` 同源；`custom` 模板放开、用户自担风险。
4. **宿主发 HTTP**：统一客户端与代理配置，超时 clamp(2,30) 秒。
5. **第二次 eval**：响应 JSON 传给 `extractor`，返回统一结果模型。
6. **结果校验**：字段类型检查后转 `UsageData`。

### 4.4 统一数据模型与错误双轨

```rust
pub struct UsageData {
    plan_name: Option<String>,       // 套餐名
    total: Option<f64>, used: Option<f64>, remaining: Option<f64>,
    unit: Option<String>,            // "USD" / "CNY" / "%"
    is_valid: Option<bool>, invalid_message: Option<String>,  // 失效标记
    extra: Option<String>,           // 自由文本或内嵌 JSON（resetTime/planLabel 等）
}
pub struct UsageResult { success: bool, data: Option<Vec<UsageData>>, error: Option<String> }
```

- **归一约定**：百分比统一为"已用百分比"（0-100）；多套餐窗口（如 5h + 周）返回数组。
- **错误双轨**：`Err` = 瞬时失败（网络/超时/5xx/429）→ 前端重试并保留旧值；`Ok(success:false)` = 确定性失败（401/解析失败）→ 立即透出。类型定义于 `src-tauri/src/provider.rs:311-342`。

### 4.5 缓存与刷新策略

- 前端 React Query 轮询（默认 5 分钟，可配，只对**当前启用**的供应商自动查，`refetchIntervalInBackground` 保证托盘态继续刷新）。
- **keep-last-good**：瞬时失败后 10 分钟窗口内继续展示上次成功值（`src/lib/query/queries.ts:142-243`）。
- 后端进程内缓存不持久化（重启即空）——托盘重启后首次悬停需等查询完成，这是 QuotaTray 可以改良的点。

## 5. 对 QuotaTray 的复用清单

按"照搬 / 改良 / 规避"三档归纳：

**照搬**（生产验证过的设计）
- JS 脚本 `{request, extractor}` 两阶段协议、`{{变量}}` 凭据注入、QuickJS 资源限制三件套
- `UsageData` 统一结果模型与"已用百分比"归一约定
- 错误双轨制 + keep-last-good
- 出口脱敏体系（日志清洗、UI 掩码、错误信息脱敏）
- 托盘悬停刷新 + 节流模式

**改良**
- 密钥存储：主密钥入系统凭据库 + AES-GCM 密文（cc-switch 无此层）
- 平台检测表收归单一数据源（cc-switch 前后端双表是明确的技术债）
- 托盘缓存轻量持久化（记录时间戳，重启后先显旧值再刷新）

**规避**
- 供应商切换/代理/MCP 等重功能体系——QuotaTray 保持轻量专注
- 导出明文 SQL、http:// 同步端点等外流通道（红线见 AGENTS.md）

MIT 协议允许直接参考乃至复用其代码（如各平台解析逻辑、脱敏实现）。
