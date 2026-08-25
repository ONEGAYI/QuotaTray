# 预置 Provider 缺口预研（对照 cc-switch v3.20.0）

> 日期：2026-08-25 ｜ 依据：cc-switch 本地源码精读（`D:\CODE\Project\_ForExplore\cc-switch`）与
> [CC-Switch调研报告.md](CC-Switch调研报告.md) §4
>
> **结论**：QuotaTray 现有 12 项预置已覆盖 cc-switch 查询体系的主流平台；
> 缺口分三层——**A 层 3 家可直接移植**（批次 1）、**NewAPI 系通用模板 1 个
> 覆盖约 40 家中转站**（批次 2）、B/C 层需架构决策或逐站调研（**待做**）。

## 1. 口径

cc-switch 的「预设」有两类，性质不同：

- **切换端点预设**（Claude 侧 77 条、Codex 侧 55 条等）：只是 base_url + 鉴权头配置，绝大多数**没有**余额查询能力；
- **余额/用量查询实现**（4 套 Rust 服务）：`balance.rs` 按量余额、`coding_plan.rs` 套餐额度、`subscription.rs` 官方订阅 OAuth、`subscription_grok.rs` Grok 积分。

QuotaTray 是余额监视器，native provider 语义对齐后者，缺口按**查询能力**统计。

## 2. 现状：已覆盖清单

| cc-switch 已验证的查询平台 | QuotaTray 对应 |
|---|---|
| DeepSeek 余额 | `deepseek` |
| SiliconFlow 国内/国际余额 | `siliconflow` / `siliconflow_global` |
| OpenRouter 余额 | `openrouter` |
| Kimi 开放平台余额 / Kimi Code 套餐 | `kimi_cn` / `kimi_global` / `kimi_code_cn` / `kimi_code_global` |
| 智谱/Z.ai Coding Plan | `zhipu` / `zai`（仅 Coding Plan） |

QuotaTray 另有 `zhipu_api` / `zai_api`（按量余额，cc-switch 未实现），合计 12 项。

## 3. 缺口分层与批次标记

| 层 | 内容 | 状态 |
|---|---|---|
| **A 层** | StepFun、Novita AI、MiniMax Coding Plan——cc-switch 端点/字段全有生产验证 | **批次 1** |
| **NewAPI 系** | 约 40 家 one-api 系中转站，`GET {base}/api/user/self` 通用查询 | **批次 2**（预置模板，非逐站 native） |
| **B 层** | 火山方舟（V4 AK/SK 签名）、Claude / ChatGPT(Codex) / Gemini / Grok 四家订阅 OAuth | **待做**——凭据形态/架构决策见 §6.1 |
| **C 层** | 国内大厂（千帆/百炼/混元/Longcat/MiMo/灵光/魔搭等）与聚合站（PPIO/AiHubMix/Nvidia 等） | **待做**——cc-switch 仅有切换预设、无查询实现，需逐站调研公开余额 API |

## 4. 批次 1 移植参考（native provider × 3）

### 4.1 StepFun 阶跃星辰（单行余额型）

- **请求**：`GET https://api.stepfun.com/v1/accounts`，`Authorization: Bearer {key}`（cc-switch balance.rs:148-204）
- **响应**：顶层 `{ balance, total_cash_balance, total_voucher_balance, ... }`；cc-switch 只读 `balance`（兼容数字/字符串），**CNY 硬编码**（响应不携带币种）
- **cc-switch 行为差异**：字段缺失时兜底 `0.0` 不报错——移植遵循 QuotaTray「宁缺毋错」惯例改为**确定性失败**，不产假余额
- **国际站**：cc-switch 域名检测认 `.com`/`.ai` 双域，但余额端点硬编码 `.com`；国际站 `.ai` 的余额端点**未经验证**，首版仅预置国内站
- **测试基线**：cc-switch 无测试，响应字段知识来自源码注释——移植后以 mock 契约测试为准

### 4.2 Novita AI（单行余额型，单位陷阱）

- **请求**：`GET https://api.novita.ai/v3/user/balance`，Bearer（balance.rs:348-410）
- **响应**：顶层 `{ availableBalance, cashBalance, creditLimit, ... }`；**原始单位 0.0001 USD，必须 ÷10000**
- cc-switch 额外在余额 ≤0 时置 `is_valid=false`（"No balance remaining"）——语义可保留；`cashBalance` 等子字段忽略
- 同样把「缺失兜底 0.0」改为确定性失败

### 4.3 MiniMax Coding Plan（百分比多窗口型，双站）

- **请求**：`GET https://{api.minimaxi.com | api.minimax.io}/v1/api/openplatform/coding_plan/remains`，Bearer——双站仅域名不同，**复用 SiliconFlow 双站参数化模式**
- **响应**（有 cc-switch 完整单测与真实样例，coding_plan.rs:636-700, 1570-1738）：

```json
{
  "model_remains": [{
    "model_name": "general",
    "current_interval_remaining_percent": 98.0,
    "current_weekly_remaining_percent": 95.0,
    "current_weekly_status": 1,
    "end_time": 1780329600000,
    "weekly_end_time": 1780848000000
  }],
  "base_resp": { "status_code": 0, "status_msg": "success" }
}
```

- **解析契约**（cc-switch 生产验证）：
  - `model_remains[]` 只取 `model_name == "general"`（位置无关），`video` 等其他条目丢弃；
  - **5h 桶无条件展示**：`current_interval_remaining_percent`（剩余%）；
  - **周桶仅当 `current_weekly_status == 1`**；`==3`（无周限额套餐，remaining 恒 100）与 `==2` 都跳过，防「0% 已用」假窗口；
  - 百分比语义为**剩余**，QuotaTray 归一为已用：`used = 100 − remain`、`total = 100`、`unit = "%"`，不裁剪范围；
  - `end_time` / `weekly_end_time` 为毫秒时间戳——QuotaTray 有专用 `reset_at` 字段（Kimi Code 已有 RFC3339 经验），**不学 cc-switch 塞 extra**；
  - `base_resp.status_code != 0` 为业务错误（透出 `status_msg`）；`base_resp` 整体缺失则跳过检查。
- **已知边界**：`current_weekly_remaining_percent` 缺失只跳过周桶；无 general 条目 → cc-switch 返回空——QuotaTray 语义待 plan 定案（倾向确定性失败，同「宁缺毋错」）。

## 5. 批次 2 移植参考（NewAPI 系通用预置模板）

- **端点**：`GET {base}/api/user/self`（one-api 系惯例，覆盖 PackyCode/88Code 类约 40 家中转站）
- **鉴权双头缺一不可**（严格 NewAPI v0.7+ 校验，缺则 401）：
  - `Authorization: Bearer {accessToken}`——NewAPI「系统访问令牌」（站点个人设置生成），**不是** sk- 推理 key；在 QuotaTray 条目中填入 apiKey 凭据位即可（同为加密存储的敏感串）
  - `New-Api-User: {userId}`——用户数字 ID
- **字段映射**：`remaining = data.quota / 500000`、`used = data.used_quota / 500000`（USD，one-api 惯例比率 500000）、`planName = data.group`、`is_valid = $.success`
- **DSL 能力核对结论**（已对 core `template/mod.rs` 核实）：
  - `{{baseUrl}}` / `{{apiKey}}` 变量、`divide` transform 均支持，模板可完整表达；
  - **`New-Api-User` 头无法用变量表达**（DSL 无 userId 变量）——预置模板中写占位值，用户选中后手改自己的 ID（编辑器「选中预设→填入→可继续手改」的自然流程），不动 core；
  - 比率 500000 为惯例值，个别站自定义比率时用户可手改 divide 的 `by`。

## 6. 待做项（不在本两批次）

### 6.1 B 层：有验证端点但需架构决策

| 平台 | 门槛 |
|---|---|
| 火山方舟 Coding Plan | V4 签名（HMAC-SHA256，service=ark），凭据为 **AK/SK 两段**——`Credentials` 需扩凭据模型 |
| Claude 官方订阅 | OAuth：读 Claude CLI 本地 token 并刷新（`api.anthropic.com/api/oauth/usage`） |
| ChatGPT/Codex 订阅 | OAuth + `ChatGPT-Account-Id` 头，依赖 CLI 本地凭据 |
| Gemini 订阅 | Google OAuth 刷新 + 两段式 RPC |
| Grok 订阅积分 | 依赖 grok.com 网页会话（带 Origin/Referer 内部端点），稳定性存疑 |

订阅 OAuth 四家对 QuotaTray 是架构级变更：从「粘贴 API key」到「读取/管理 CLI OAuth token」，涉及交互与安全红线新边界。

### 6.2 C 层：无查询实现，需逐站调研

- **国内厂商官方**：百度千帆（Coding/Token Plan）、阿里百炼（含 Coding）、腾讯混元（仅 Codex 侧）、Longcat（美团）、小米 MiMo（含 Token Plan）、蚂蚁灵光、ModelScope、KAT-Coder
- **聚合/云**：PPIO、AiHubMix、Nvidia（积分制）、CherryIN、DMXAPI、TheRouter 等
- 其中多数是否有公开余额 API **未经调研**，按需逐站确认后再定 native/模板/script 落点。

## 7. cc-switch 源码位置索引

| 内容 | 位置 |
|---|---|
| StepFun / Novita 余额 | `src-tauri/src/services/balance.rs`（148-204 / 348-410） |
| MiniMax Coding Plan + 单测样例 | `src-tauri/src/services/coding_plan.rs`（413-496, 636-700, 1570-1738） |
| NewAPI 查询脚本模板 | `src/components/UsageScriptModal.tsx`（91-117）+ `usage_script.rs`（424-444 变量替换） |
| 订阅 OAuth 三家 + Grok | `services/subscription.rs` / `subscription_grok.rs` |
| 全量切换预设 | `src/config/*ProviderPresets.ts`（Claude 77 / Codex 55 / Gemini 23 条等） |
