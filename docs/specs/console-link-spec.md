# 控制台直达（Console Link）规格 · #59

状态：已定案（样式经临时预览页确认，2026-08-28）。桌面端 2026-08-28 实现；Android
端 2026-08-29 启用（`consoleLink` 翻位 + 44px 触摸热区同 PR 落地）。剩余验证事项以
AGENTS.md「移动端能力缺口追踪」为活追踪。

## 1. 背景与目标

余额卡片上增加「控制台直达」图标按钮：点击后用系统默认浏览器打开对应平台控制台
（余额/充值/账单页），省去用户手动找网址。来源 issue #59。

不做的事：

- 不在应用内嵌网页；
- 未解析到 URL 的条目不渲染图标（不出无效链接）。

## 2. 样式定案（2026-08-28 所有者确认）

| 项 | 定案 |
|---|---|
| 图标 | Material `open_in_new`（实心 fill 变体，24 网格），内联 SVG，16px |
| 按钮载体 | 迷你档图标钮 26px / xs 圆角（button.md T-004 档位表内） |
| 位置 | 名称行内、状态徽章右侧（预览页「位置 A」） |
| 颜色 | 默认 `--qt-text-faint` 弱化色；hover/focus 提升到 accent（T-004 hover 规则的登记例外） |
| 提示 | data-tooltip 悬停气泡，文案「访问控制台」/ "Open console" |
| 无障碍 | `aria-label` 同提示文案；仅当 URL 存在才渲染 |

图标 SVG（Apache-2.0，Google Material Symbols）以 `viewBox="0 -960 960 960"` path
内联于组件，与 TitleBar 的 GithubMark 内联先例同模式。

**移动端补充（2026-08-29 启用）**：Android 下按钮命中区扩为 44×44px（透明无边框，
视觉仍为 16px 图标，T-010）；伪元素 tooltip 气泡不渲染（T-001/T-010 分流），
`aria-label` 保留同文案；按压反馈走全局 `:active` 规则，hover 定案不适用于触摸端。

## 3. 数据模型（core）

### 3.1 NativeMeta 预置默认值

`provider::NativeMeta` 增加字段 `console_url: Option<&'static str>`。20 个注册表
项逐一填写（双站为独立注册项，天然按站点区分域名）；值以联网核验结果为准（§4）。

语义：平台固有属性、随版本分发，等价于查询端点一类的「预置知识」。CLI 未来可零成本
复用（如 `quota open`）。

### 3.2 ProviderEntry 自定义覆盖

`config::ProviderEntry` 增加字段：

```rust
/// 控制台直达 URL 覆盖（明文、非敏感，不进 vault）。None = 回退预置默认；
/// 模板/脚本条目仅此字段生效。旧配置缺省天然兼容。
#[serde(default, skip_serializing_if = "Option::is_none")]
pub console_url: Option<String>,
```

URL 不是凭据，明文落 config.json，与 `base_url` 同级待遇（安全红线不涉及）。

### 3.3 解析函数（两端共用语义）

```rust
/// 条目的控制台直达 URL：自定义覆盖优先；native 条目回退注册表预置值；
/// 模板/脚本条目仅取自定义值；均无则 None（UI 不渲染图标）。
pub fn resolve_console_url(entry: &ProviderEntry) -> Option<String>
```

实现于 `provider/mod.rs`：`Kind::Native { provider }` 时 `find(provider)` 取
`meta().console_url`；`Kind::Template/Script` 忽略注册表。

## 4. 预置平台 URL 表（联网核验，2026-08-29）

> 值由调研代理逐平台经官方文档/官网/FAQ 交叉验证；深链仅在官方文档明示时使用，
> 否则取控制台稳定根路径。与源码 API 端点域名逐一比对，确保双站条目不串域名。

| id | 平台 / 站点 | 控制台 URL | 依据 |
|---|---|---|---|
| deepseek | DeepSeek 开放平台 | `https://platform.deepseek.com/` | 官方 API 文档，登录后首页即余额 |
| siliconflow | 硅基流动 · 国内站 | `https://cloud.siliconflow.cn/` | 官方登录页即控制台入口 |
| siliconflow_global | 硅基流动 · 国际站 | `https://cloud.siliconflow.com/` | 官方国际站登录页（与 .cn 分立） |
| openrouter | OpenRouter | `https://openrouter.ai/settings/credits` | 官方 FAQ 内嵌 credits 管理页链接 |
| kimi_cn | Kimi 开放平台 · 国内 | `https://platform.kimi.com/` | 旧域 platform.moonshot.cn 301 至此（实测） |
| kimi_global | Kimi 开放平台 · 国际 | `https://platform.kimi.ai/` | 旧域 platform.moonshot.ai 301 至此（实测） |
| stepfun | 阶跃星辰 | `https://platform.stepfun.com/` | 官方开放平台即控制台 |
| novita | Novita AI | `https://novita.ai/billing` | 官方 Quickstart 明示 Billing 链接 |
| zhipu_api | 智谱按量（bigmodel） | `https://open.bigmodel.cn/finance/overview` | 官方费用 FAQ「财务总览」 |
| zai_api | Z.ai 按量 | `https://z.ai/manage-apikey/billing` | 官方 Quick Start Billing Page |
| minimax | MiniMax · 国内 | `https://platform.minimaxi.com/user-center/payment/balance` | 官方账户 FAQ 余额/充值页 |
| minimax_global | MiniMax · 国际 | `https://platform.minimax.io/user-center/payment/balance` | 官方国际站账户 FAQ |
| kimi_code_cn | Kimi Code 订阅 · 国内 | `https://www.kimi.com/code/console` | 官方会员文档「控制台查看剩余额度」 |
| kimi_code_global | Kimi Code 订阅 · 国际 | `https://kimi.ai/code/console` | 与国内站对称推断（返回 200 SPA；内容需登录渲染，未完全验证） |
| zhipu | GLM Coding Plan（bigmodel） | `https://www.bigmodel.cn/coding-plan/personal/usage` | 官方使用须知「用量统计」链接 |
| zai | GLM Coding Plan（z.ai） | `https://z.ai/manage-apikey/billing` | 官方 devpack 文档（各计费类型用量） |
| claude | Claude Pro/Max（订阅） | `https://claude.ai/settings/billing` | Anthropic 官方帮助 Settings > Billing |
| codex | ChatGPT 订阅（Codex） | `https://chatgpt.com/#settings/Subscription` | 官方帮助菜单路径；hash 深链为 SPA 路由，未获官方文档化（改版可能失效，保守可退 `chatgpt.com/`） |
| gemini | Gemini（Code Assist） | `https://gemini.google.com` | 所有者定案（2026-08-29）取「打开 Gemini 本体」语义；Google 无 Code Assist 个人用量网页 |
| grok | Grok 订阅 | `https://grok.com/` | xAI 官方 FAQ「grok.com → Settings → Billing」；站内深链有反爬无法外部验证 |

## 5. IPC（desktop src-tauri）

### 5.1 NativeMetaDto

`list_native_metas` 返回的 DTO 增加 `console_url: Option<String>`，直出注册表值。
前端 `useNativeMetas` 已有管道，ProviderCard 已接收 `nativeMeta` prop，无新增查询。

### 5.2 打开命令

```rust
#[tauri::command]
pub fn open_console_url(app: AppHandle, url: String) -> Result<(), String>
```

- **scheme 白名单**：仅 `http://` / `https://`（拒绝 `file:`、`javascript:` 等），
  校验在 Rust 侧，前端不重复。
- 打开走 `app.opener().open_url(url, None)`（Rust 侧插件 API，同 `open_update_dir`
  先例）。**不走** 前端 `@tauri-apps/plugin-opener` 直调——capability 的
  `opener:allow-open-url` scope 锁定 GitHub 仓库单 URL，且模板条目的自定义 URL 是
  任意域名，无法枚举白名单；自定义 command 不受前端 capability 约束，信任边界由
  Rust 侧 scheme 校验收口。
- capability 文件不改（GitHub 链接 scope 保持原样）。

## 6. 前端（跨端组件 + 能力位）

1. **types.ts**：`NativeMeta` / `ProviderEntry` 镜像加 `console_url?: string`。
2. **runtimeView**：`RuntimeUiPolicy` 加 `consoleLink: boolean`——desktop `true`；
   android 初版 `false`（待验证标记），2026-08-29 翻位为两端 `true`。
3. **providerCardView.ts** 纯逻辑加 `resolveConsoleUrl(entry, nativeMeta)`：
   `entry.console_url ?? nativeMeta?.console_url`；补单测。
4. **ProviderCard**：名称行内 statusBadge 之后渲染图标按钮。渲染条件：
   `runtimeUiPolicy.consoleLink && resolveConsoleUrl(...) != null`。按钮复用
   `IconButton`（label = i18n `card.console`，自动 data-tooltip），内联 Material
   `open_in_new` SVG；点击 `api.openConsoleUrl(url)`，失败走卡片 feedback 通道显示
   `card.consoleOpenFailed`（初版静默吞错，2026-08-29 审查轮改为可见反馈）。
5. **EditDialog**：跨端共享编辑页加可选字段「控制台地址」（校验：空或 http/https
   前缀；空 = 清除覆盖回退默认）。表单字段两端都渲染（纯数据编辑无平台行为）。
6. **i18n**：`card.console` zh「访问控制台」/ en "Open console"；EditDialog 字段
   标签与占位文案双语。

## 7. Android 启用与验证状态

2026-08-29 翻 `runtimeView.consoleLink` 为两端启用，同 PR 落地 44px 触摸热区
（`body.qt-mobile-runtime .qt-icon-btn.qt-console-btn`，由 mobile-style 契约测试
锁定）。opener 拉起系统浏览器与返回栈交互的验证进度、厂商浏览器差异与真机验收
状态，以 AGENTS.md「移动端能力缺口追踪」对应条目为活追踪；验收完成前不宣称
移动稳定支持。

## 8. 测试计划（TDD）

| 层 | 用例 |
|---|---|
| core `provider` | 注册表 20 项 console_url 全非空且为 https；`resolve_console_url`：自定义覆盖 native 默认、native 回退默认、模板条目仅自定义、均无 → None |
| core `config` | 旧 config.json（无 console_url 字段）反序列化兼容；字段 round-trip 与 skip 序列化 |
| src-tauri commands | `open_console_url` scheme 校验：https 通过（opener mock/不实际打开的断言方式实现时定）、`file:`/`javascript:`/无 scheme 拒绝；`NativeMetaDto` 带 console_url |
| 前端 runtimeView | `consoleLink` 位：desktop true / android true（2026-08-29 翻位） |
| 前端移动样式 | mobile-style 契约：Android 下 `qt-console-btn` 命中区 44×44px 存在 |
| 前端 providerCardView | `resolveConsoleUrl` 三态 |
| 前端 providerCardView（供 EditDialog 消费） | URL 校验纯函数：空/合法/缺 scheme/非法 scheme/scheme 大小写 |

## 9. 安全说明

- console_url 为公开导航信息，明文存储，非凭据，不进 vault、不脱敏。
- 打开动作的攻击面是「恶意 URL 借应用拉起本地程序」——由 Rust 侧 scheme 白名单
  （仅 http/https）收口；自定义值来源是用户自己在 EditDialog 填写的字段。
