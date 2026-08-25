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
├── README.md                  # 项目自述（中文，默认展示）：定位/功能/预置平台/
│                              #   模板示例/安全设计/构建安装，顶部链英文版
├── README.en.md               # 英文版自述，与中文版结构对齐，顶部互链
├── CHANGELOG.md               # 版本变更记录（Keep a CHANGELOG，按 PR 归纳）
├── LICENSE                    # MIT 许可证全文（参考项目 cc-switch 同为 MIT）
├── clean.cmd                  # Windows 开发目录清理入口：交互或 Level 1/2/3
├── scripts/
│   ├── clean.ps1             # 固定白名单分级清理器（仓库边界校验 + WhatIf）
│   └── clean.tests.ps1       # 隔离沙箱契约测试：三级目标/预览/受保护文件
├── Cargo.toml                 # workspace 根：成员、共享依赖版本、release 配置
├── rust-toolchain.toml        # 锁定 stable 工具链
├── .gitignore                 # 含 .DevApiKey.json / 前端与 gen/schemas 生成物
├── .DevApiKey.json.example    # 本地密钥文件模板（真实文件被 ignore）
├── .github/workflows/
│   └── ci.yml                 # CI：双矩阵 fmt+clippy+test（Ubuntu 含 Tauri 依赖）+ 前端 lint/build
├── crates/
│   └── quota-core/            # 业务核心库（无 UI 依赖）
│       └── src/
│           ├── lib.rs         # 模块声明与 re-export
│           ├── model.rs       # UsageData（含窗口重置时刻 reset_at）/ QueryError 双轨分类
│           │                  #   （可选 detail 排查详情，Display 不输出），附契约单测
│           ├── pricing.rs     # 峰谷定价：周几+时间段判定与下次翻转（epoch ms 纯函数、
│           │                  #   UTC 偏移/本地时区）、三档价格（缓存命中/未命中/输出，
│           │                  #   每 MTokens）、计费模式（按量三档价/订阅积分项）、
│           │                  #   预置：DeepSeek 单站双币 + Kimi 开放平台国内/国际 +
│           │                  #   Kimi Code 国内/国际订阅额度 + 智谱/Z.ai 通用 API
│           │                  #   国内/国际按量模型 + Coding Plan
│           │                  #   （按量模型 + Coding Plan 订阅项，模型级窗口）、
│           │                  #   自定义校验与预置/自定义模型库字段级合并
│           │                  #   （resolve/resolve_with、preset/preset_with_currency）
│           ├── vault/         # 凭据保险库
│           │   ├── mod.rs     # Vault 门面：open（取/建主密钥）+ encrypt/decrypt
│           │   ├── cipher.rs  # AES-256-GCM 与 v1: 密文格式（nonce 随机、AAD 绑定）
│           │   └── store.rs   # SecretStore trait + KeyringStore（生产）/ InMemoryStore（测试）
│           ├── config/        # 配置层
│           │   ├── mod.rs     # AppConfig（providers + custom_models 自定义模型库；
│           │   │              #   原子写、密文落盘、旧文件兼容）+ ProviderEntry
│           │   │              #   （含 use_proxy 条目级查询代理开关，默认 false）
│           │   ├── provider.rs# Credentials / ProviderKind（serde tag 分派：native/
│           │   │              #   template/script，script 为 M4 新增——旧版二进制
│           │   │              #   读到会报未知 tag，升级单向）、PlanVariant
│           │   │              #   订阅套餐变体（auto/no_weekly/weekly，智谱 v1 无周限）
│           │   └── transfer.rs# 完整配置跨机器迁移：一次性迁移密钥转写、私有认证
│           │                  #   二进制容器、字节 API 与原子文件导入导出；
│           │                  #   容器 v2（M5）信封 {config, history} 携带历史
│           │                  #   数据（v1 仍可导入，单向升级）、TransferBundle
│           │                  #   返回 config + 可选历史行
│           ├── history/       # 历史数据存储（M5，SQLite/rusqlite bundled）
│           │   └── mod.rs     # HistoryStore：成功查询结果时序落库（一条 UsageData
│           │                  #   一行，PK provider_id+window_key+sampled_at，
│           │                  #   同毫秒重放幂等）；window_key 纯函数（plan_name
│           │                  #   优先、序数 w0/w1 回退）；PRAGMA user_version +
│           │                  #   MIGRATIONS 数组逐版本事务迁移（降级运行拒绝
│           │                  #   打开）；30 天滚动清理（写入时节流 ≥1h 一次）；
│           │                  #   WAL + synchronous NORMAL；export_rows/merge_rows
│           │                  #   供迁移容器导入导出；非关键数据——调用方写失败
│           │                  #   静默告警不阻断查询主链路
│           ├── http/          # HTTP 抽象
│           │   ├── mod.rs     # HttpClient trait + 请求/响应/错误类型（Debug 打码，
│           │   │              #   敏感头判断统一走 redact；HttpResponse.raw 为
│           │   │              #   二进制协议字节保真通道，gRPC-web 必走）
│           │   ├── redact.rs  # 错误详情脱敏（参考 opencode）：结构化正则 + 本次请求
│           │   │              #   密钥字面量两遍清洗，先清洗后截断（2048 字符）
│           │   └── reqwest.rs # 生产实现（rustls；错误去 URL 防凭据泄漏；
│           │              #   new_with_proxy 供更新通道注入可选代理）
│           ├── provider/      # 预置平台
│           │   ├── mod.rs     # NativeProvider trait（query 收套餐变体）+ 注册表（20 项）+
│           │   │              #   supports_plan_variant/uses_cli_credentials 标记（订阅
│           │   │              #   四家 CLI 凭据型：查询时读本机官方 CLI 登录文件）+
│           │   │              #   解析工具（parse_success_json/
│           │   │              #   status_error_with_body 错误附脱敏响应体 detail——
│           │   │              #   error_detail 嵌套 error.message 后回退平铺字符串，
│           │   │              #   非字符串形态不误提取）+ MockHttp
│           │   ├── claude.rs         # Claude 订阅（Pro/Max）：CLI 凭据复用——只读
│           │   │                     #   ~/.claude/.credentials.json（claudeAiOauth.
│           │   │                     #   accessToken，双拼写兼容）→ GET oauth/usage
│           │   │                     #   （anthropic-beta 头）；已知四窗口固定顺序 +
│           │   │                     #   未知窗口自动兼容、extra_usage 透传 extra；
│           │   │                     #   401/403 → 重登引导（fetch_json_relogin）
│           │   ├── codex.rs          # Codex（ChatGPT 订阅）：CLI 凭据复用——只读
│           │   │                     #   ~/.codex/auth.json（auth_mode==chatgpt 门禁，
│           │   │                     #   API key 模式确定性引导）→ GET wham/usage
│           │   │                     #   （UA codex-cli 防拦 + ChatGPT-Account-Id 头）；
│           │   │                     #   primary/secondary 双窗口按秒数标注、reset 秒→毫秒
│           │   ├── gemini.rs         # Gemini Code Assist：CLI 凭据复用——只读
│           │   │                     #   ~/.gemini/oauth_creds.json，access_token
│           │   │                     #   过期用 refresh_token + gemini-cli 公开
│           │   │                     #   client 凭据刷新（不写回文件，失败回退
│           │   │                     #   旧 token）；两步 RPC loadCodeAssist →
│           │   │                     #   retrieveUserQuota，buckets 按模型分组
│           │   │                     #   （flash-lite 先判）聚合取最小剩余比例
│           │   ├── grok.rs           # Grok 订阅 credits：CLI 凭据复用——只读
│           │   │                     #   ~/.grok/auth.json（scope map，OIDC 优先/
│           │   │                     #   legacy 兜底）；gRPC-web 空帧请求 + 整组
│           │   │                     #   伪装头；响应无 .proto——帧拆分 + 通用
│           │   │                     #   protobuf 递归扫描 + 字段路径启发提取
│           │   │                     #   （fixed32 百分比/varint 重置秒/零用量
│           │   │                     #   特判），必须走 HttpResponse.raw 字节
│           │   │                     #   保真通道；gRPC 16/7→重登、4/14→瞬时
│           │   ├── deepseek.rs       # /user/balance（单站双币，余额 API 返回币种）
│           │   ├── siliconflow.rs    # /v1/user/info（国内/国际双站参数化，CNY/USD）
│           │   ├── openrouter.rs     # /api/v1/credits（remaining = credits − usage）
│           │   ├── kimi.rs           # /v1/users/me/balance（国内/国际双站，
│           │   │                     #   余额+代金券/现金拆分进 extra）
│           │   ├── kimi_coding.rs    # Kimi Code 国内/国际双站 /coding/v1/usages：
│           │   │                     #   5h+周额度、RFC3339 重置时间、remaining 本地推导
│           │   ├── minimax.rs       # MiniMax Coding Plan 国内/国际双站 coding_plan/
│           │   │                     #   remains：仅 general 条目、5h+周剩余% 归一已用%、
│           │   │                     #   周桶仅 weekly_status==1 展示（==3 无周限防假窗口）
│           │   ├── novita.rs        # /v3/user/balance（availableBalance÷10000=USD）
│           │   ├── stepfun.rs       # /v1/accounts（顶层 balance，CNY）
│           │   ├── zhipu_metered.rs  # 智谱/Z.ai 通用 API 按量余额：Bearer、credit grants
│           │   │                     #   优先，404/405 回退 balance；两站 CNY/USD
│           │   └── zhipu.rs          # GLM Coding Plan 用量（智谱/Z.ai 双站，非文档
│           │                         #   端点、裸 key、已用百分比多窗口：type
│           │                         #   过滤 + unit 归类 5h/周 + 未知条目仅填
│           │                         #   5h 空槽宁缺毋错；TIME_LIMIT 独立成
│           │                         #   MCP 行（不受变体过滤）；PlanVariant
│           │                         #   声明过滤——NoWeekly 只留 5h、Weekly 放宽；
│           │                         #   各窗口 nextResetTime 透传 reset_at 供倒计时）
│           ├── template/      # 声明式模板 DSL（M2a）
│           │   ├── mod.rs     # DSL 结构/静态校验/执行器（变量替换、URL 安全、
│           │   │              #   多窗口、uses_api_key；错误文案不含明文凭据；
│           │   │              #   substitute/check_url_safety 与 script 查询共用）
│           │   └── path.rs    # JSONPath 子集（$.a.b[0]，拒绝过滤器/通配符）
│           ├── script/
│           │   └── mod.rs     # QuickJS 沙箱脚本查询（M4）：{request, extractor}
│           │                  #   两阶段协议（rquickjs 默认内嵌编译 QuickJS-NG）；
│           │                  #   六环节——变量替换（代码字符串层面，脚本可
│           │                  #   分享）→ 沙箱 eval request 产物（JSON 桥出入箱）
│           │                  #   → URL 安全校验 → 宿主发请求（fetch_json 收口
│           │                  #   错误分类与脱敏、apiKey 从根登记）→ 沙箱 eval
│           │                  #   extract → 产物转 UsageData（parse_num 语义）；
│           │                  #   沙箱限制：内存 16MiB/栈 256KiB/单次 eval 5s
│           │                  #   CPU 中断器（eval 在 spawn_blocking 线程）；
│           │                  #   validate 干跑（假变量+形状校验）；JS 异常
│           │                  #   消息可能回显注入 key——全错误路径 redact 收口
│           ├── query/
│           │   └── mod.rs     # QueryEngine：双通道路由（条目 use_proxy 走代理
│           │                  #   通道，未配全局端口 → 确定性引导）→解密→分派
│           │                  #   （native/template/script；CLI 凭据型 native 跳过
│           │                  #   解密前置，api_key_enc 可为 None）→超时（15s）
│           └── update.rs      # 更新检测（M4-b）：版本三段比较、GitHub release 解析
│                              #   与资产选择、节流/每日到点纯函数、AssetDownloader
│                              #   独立下载通道（10min 总超时 + 15s 连接超时快速失败、
│                              #   256MB 上限、分块进度/速率回调且兼容旧实现、可选代理
│                              #   构造——显式代理不叠加环境变量；proxy_url_of 端口→
│                              #   URL 单一拼接口径）、字节原子落盘
├── apps/
│   ├── quota-cli/             # CLI 前端（bin 名 quota，M2b 完成；i18n 三态 + 更新检测）
│   │   └── src/
│   │       ├── main.rs        # clap 定义子命令（含 pricing model / config 迁移组）+ dispatch
│   │       │                  #   + --lang 全局参数（两阶段解析）+ 启动更新提示钩子
│   │       │                  #   （stderr、节流、--json 与 update 子命令自身豁免）
│   │       ├── ctx.rs         # Ctx：配置路径 + SecretStore 注入 + lang 字段 +
│   │       │                  #   history_path 推导（config 同目录 history.db，M5）
│   │       ├── exit.rs        # 退出码三分约定（0 全成功 / 1 确定性 / 2 仅瞬时）
│   │       ├── idgen.rs       # 6 位 Crockford base32 随机 id（无偏映射）
│   │       ├── io.rs          # 交互薄层：掩码读 key（星号回显、Ctrl+V 剪贴板粘贴、管道分流）、
│   │       │                  #   多行读取双约定——模板 JSON 空行结束，脚本代码
│   │       │                  #   单独一行 . 结束（空行照常保留），管道均读到 EOF
│   │       ├── lang.rs        # Lang 三态（zh/en/system）+ sys-locale 检测 +
│   │       │                  #   settings.json language 读取（mini struct，容错回退 System）
│   │       ├── settings_io.rs # settings.json 的 update 字段读取（mini struct：开关/时刻/
│   │       │                  #   时间戳/代理端口）+ last_check 写回（Value 读改写保留
│   │       │                  #   未知字段 + 原子写）
│   │       ├── render.rs      # comfy-table 表格 + query --json 输出结构（纯函数可测、文案双语；
│   │       │                  #   error 含 detail 排查详情 additive 透出）+
│   │       │                  #   重置倒计时列（fmt_reset_countdown，now 注入）+
│   │       │                  #   pricing 价格对照表/星期连续段聚合/UTC 偏移描述 +
│   │       │                  #   history 时间桶聚合/分页切片/走势表（M5，纯函数）
│   │       ├── texts.rs       # 双语文案表（TextKey exhaustive，漏译即编译错误）+
│   │       │                  #   带参文案函数 + clap about/help 运行时翻译
│   │       └── cmd/           # 子命令实现（每命令一模块，handler 收 Ctx；文案走 texts.rs）
│   │           ├── mod.rs     # 子模块声明（devsmoke 仅 debug 编入）
│   │           ├── list.rs    # 条目列表（表格 / --json providers 数组）
│   │           ├── query.rs   # 并行查询 + watch 轮询 + 退出码聚合（RouteHttp 全链测试）；
│   │           │              #   成功结果写历史库（M5，打开/写失败仅告警不改退出码）
│   │           ├── add.rs     # 交互向导（订阅型平台问套餐变体；template 粘贴/
│   │           │              #   script 代码粘贴 + allowInsecure 确认双形态）/
│   │           │              #   --json stdin（拒收 api_key_enc，script 干跑校验）
│   │           ├── config.rs  # config export/import：高敏感确认、完整配置迁移、
│   │           │              #   目标机器 Vault 重加密与失败不覆盖契约测试；
│   │           │              #   迁移包携带查询历史（M5）——导出全量带出、
│   │           │              #   导入按主键幂等合并，读失败降级不带不阻断
│   │           ├── history.rs # history show/clear（M5）：三档范围（24h=15m 桶/
│   │           │              #   7d=1h 桶/30d=6h 桶，桶内取最后点）+ 窗口时间线
│   │           │              #   过滤 + 分页（默认 20 行；终端交互翻页，--page N
│   │           │              #   非交互打印单页，管道整表；--json 原始点）；
│   │           │              #   clear 无 id 清全部（确认默认否）
│   │           ├── edit.rs    # 向导（回车保持，套餐变体可改；template/script
│   │           │              #   各自的 baseUrl、内容重粘贴与 script 的
│   │           │              #   allowInsecure 修改）+ --enable/--disable 快捷路径
│   │           ├── remove.rs  # 确认删除（--yes 跳过）
│   │           ├── setkey.rs  # 隐藏读 key → vault 加密写配置
│   │           ├── natives.rs # 预置平台表（含峰谷预置标记列）
│   │           ├── pricing.rs  # pricing show/set/clear：生效定价展示（判定/
│   │           │              #   价格对照表/时段聚合/下次翻转，now 注入纯函数；
│   │           │              #   show 接线自定义模型库 + 条目 currency 币种
│   │           │              #   hint 选 DeepSeek 双币套 + plan 透出/订阅说明行）、
│   │           │              #   stdin JSON 校验写入、清除回退预置
│   │           ├── pricing_models.rs # pricing model list/add/remove：自定义模型库
│   │           │              #   管理（表格价格对照/同 id 覆盖/删空移键，纯函数可测）
│   │           ├── template.rs# template test：静态校验 + 真实试查
│   │           ├── script.rs  # script test：干跑校验先行 + 真实试查（--json
│   │           │              #   stdin 双形态：config JSON 或纯 JS 文本，{ 开头
│   │           │              #   解析失败提示疑似误输；key 在校验通过后按需
│   │           │              #   交互收集，空 key 按终端/重定向分派提示）
│   │           ├── update.rs  # update：检测 GitHub release + 可选下载（--check/--yes/
│   │           │              #   --output；交互终端实时进度/速率；http 与 downloader
│   │           │              #   可注入测试；退出码三分；更新代理端口读自 settings.json
│   │           │              #   ——GUI 设置页写入，检测/下载共用并打印提示行）
│   │           ├── vault.rs   # vault status：主密钥健康检查
│   │           └── devsmoke.rs# 仅 debug：读 .DevApiKey.json 走完整链路（CLI 凭据型
│   │           │              #   平台条目仅作开关、跳过 set_api_key；--proxy 让
│   │           │              #   条目走 settings.json 代理端口，真机验证代理通道；
│   │           │              #   OK 行带 extra= 原始窗口 JSON，便于核对响应结构）
│   └── quota-desktop/         # 桌面端（M3 完成）：Tauri 2 + React，GUI 为薄层
│       ├── package.json       # pnpm：React 18/Vite/Tailwind 4/React Query 5/CodeMirror/
│       │                      #   （lang-json + lang-javascript）/Lucide/Vitest/opener +
│       │                      #   dialog（系统文件选择器）
│       ├── pnpm-workspace.yaml# pnpm 11 构建脚本许可（esbuild）
│       ├── vite.config.ts     # 端口 1420 固定、chrome110 目标、Tailwind 插件
│       ├── tsconfig.json / eslint.config.js / index.html
│       ├── src/               # React 前端（zh/en 双语 + 明暗主题三态）
│       │   ├── main.tsx / App.tsx        # 入口按 URL 分派主窗/悬停窗 + Calm Native 主布局
│       │   │                              #   （标题栏/账户摘要/列表），编辑时传递查询币种
│       │   ├── vite-env.d.ts   # Vite 静态资源（品牌 PNG 等）模块类型声明
│       │   ├── index.css       # 明暗设计令牌（暗色 #161616 中性系；color-scheme
│       │   │                    #   随主题；全局 webkit 滚动条暗色 #2e2e2e）、
│       │   │                    #   Mica-like 基底、主窗响应式系统 + 悬停面板样式
│       │   ├── assets/
│       │   │   ├── brand-mark.png # 透明品牌主图：四段额度环 + 右下 Q 形拖尾
│       │   │   └── providers/     # 十四张官方 Provider SVG（六组复用品牌 + stepfun/novita/
│       │   │                    #   minimax + 订阅四家 claude/openai(codex 用)/gemini/grok；
│       │   │                    #   minimax + newapi 供模板条目启发匹配；图标容器固定浅底
│       │   │                    #   不随主题——单色深色 logo 明暗主题均可见，浅色底图形
│       │   │                    #   （StepFun 白色圆盘）走 is-light-logo 深底变体）
│       │   ├── types.ts        # core serde 形状的 TS 镜像（模型级 plan/windows、
│       │   │                    #   PlanVariant、reset_at、自定义模型库/按币种预置 DTO、
│       │   │                    #   更新下载进度/已下载路径、KEEP_LAST_GOOD_MS）
│       │   ├── api.ts          # invoke 封装 + 短 id 生成 + 主题/更新/配置迁移命令
│       │   ├── queries.ts / queries.test.ts # React Query hooks：轮询/快照/refresh-now/
│       │   │                    #   自动更新状态事件 + Provider 变更/配置导入跨窗口失效契约 +
│       │   │                    #   CLI 可改 native/custom model 短缓存 +
│       │   │                    #   usePeakFlipTick 峰谷翻转事件锚点（#15，常驻视图重算标签）
│       │   ├── display.ts / display.test.ts
│       │   │                    # 相对/精确时间、已用百分比、数据文案（双语，与 tray.rs 成对）、
│       │   │                    #   重置倒计时/多窗口短标签（与 CLI fmt_reset_countdown 成对）、
│       │   │                    #   条目类型标签 kindLabel（与 CLI kind_label 成对）
│       │   ├── theme.tsx       # ThemeProvider：三态解析、system 实时跟随、setTheme 联动
│       │   ├── i18n/           # 轻量自写 i18n（Context + t(key, params) 插值）
│       │   │   ├── index.tsx   # LangProvider + resolveUiLang + TextKey re-export
│       │   │   ├── zh.ts       # 中文字典（as const 类型基准）
│       │   │   └── en.ts       # 英文字典（Record<TextKey,string> 编译期锁键完整）
│       │   └── components/
│       │       ├── ui.tsx              # 按钮/菜单/徽标/开关/Tooltip/Dialog 等共享基础组件
│       │       │                       #   （Dialog 以 body Portal 保证主窗居中，含 Escape、
│       │       │                       #   焦点圈定与关闭后焦点恢复）
│       │       ├── TitleBar.tsx        # 自定义标题栏：拖动/双击最大化、GitHub 仓库链接
│       │       │                       #   （opener 插件，scope 锁定仓库主页）、语言与主题
│       │       │                       #   图标下拉三选（即时保存）、窗口控制按钮
│       │       ├── BrandMark.tsx       # 标题栏/悬停面板共用的静态品牌标志薄组件
│       │       ├── ProviderCard.tsx    # 余额优先卡片：悬停/窄屏展开、按币种峰谷三价、
│       │       │                       #   条目级代理开关（Globe 按钮）、CLI 凭据型
│       │       │                       #   脚注文案（「凭据来自本机 CLI」），
│       │       │                       #   订阅积分语义、预置/库模型即时切换、多窗口
│       │       │                       #   逐窗主数值（短标签）+重置倒计时小字+
│       │       │                       #   短时反馈、启停/编辑/删除确认；错误行带
│       │       │                       #   「复制报错信息」图标（Tooltip，复制脱敏详情）
│       │       ├── HoverPanel.tsx       # 托盘悬停浮窗 A 方案：余额/额度/峰谷详情 +
│       │       │                       #   圆环数据源账户与计价模型即时切换、
│       │       │                       #   头部刷新/关闭按钮、低垂直空间压缩布局、
│       │       │                       #   hero 多窗口标签与用量行重置倒计时
│       │       ├── hoverPanelView.ts / hoverPanelView.test.ts
│       │       │                       # 悬停条目回退、前端圆环镜像与压缩视口
│       │       │                       # 判定（联动后端缩窗）纯逻辑
│       │       ├── providerCardView.ts / providerCardView.test.ts
│       │       │                       # 卡片正常/错误/keep-last-good/快照/多窗口视图纯逻辑
│       │       │                       #   （errorDetail 排查详情随查询错误态透传）
│       │       ├── providerIcon.ts / providerIcon.test.ts
│       │       │                       # 预置 Provider id → 官方 SVG 映射与未知项回退契约 +
│       │       │                       #   浅色 logo 判定（isLightLogo → 容器深底变体）+
│       │       │                       #   模板/脚本条目按名启发（templateProviderIconUrl，
│       │       │                       #   条目名含 newapi → NewAPI 品牌图）
│       │       ├── NativeProviderPicker.tsx # 添加/编辑平台聚合选择器：SVG 一级菜单+
│       │       │                       #   悬停/键盘展开二级 Provider 选单；菜单为 fixed
│       │       │                       #   浮层不占文档流，高度自适应主窗口（min(剩余
│       │       │                       #   空间, 460px)），左右双栏 minmax(0,1fr)+min-height:0
│       │       │                       #   各自独立滚动（滚动条走全局主题变量）
│       │       ├── nativeProviderGroups.ts / nativeProviderGroups.test.ts
│       │       │                       # native id 的平台分组、稳定顺序与未知项兜底
│       │       ├── providerPricing.ts / providerPricing.test.ts
│       │       │                       # 镜像 resolve_with：模型级窗口/订阅/币种套/
│       │       │                       #   自定义模型库解析、峰谷判定与模型切换纯逻辑
│       │       ├── EditDialog.tsx      # Modal：native 下拉（订阅型带套餐变体选择）/
│       │       │                       #   template 分支二级子页（运营商与模型＝名称/
│       │       │                       #   baseUrl/key/峰谷计价；设置模板＝预设模板+
│       │       │                       #   JSON 编辑器+校验/试查+编写说明卡。子页 A 用
│       │       │                       #   CSS 隐藏切换保 PricingSection 草稿态不丢）/
│       │       │                       #   script JS 编辑器（M4：校验/试查镜像 template，
│       │       │                       #   allowInsecure 开关带警告 + 默认最小闭环示例）、
│       │       │                       #   分组表单、独立凭据区与固定页脚
│       │       ├── presetTemplates.ts / presetTemplates.test.ts
│       │       │                       # 模板编辑器预设库（6 形态：通用/单对象余额/
│       │       │                       #   站点可变/总额已用/多窗口/NewAPI 系中转
│       │       │                       #   ——New-Api-User 头写占位值用户手改、
│       │       │                       #   quota÷500000 换算 USD，与 examples/templates
│       │       │                       #   同构）+ matchedPresetId 语义等价高亮判定
│       │       │                       #   （serde 往返补全键不算改动）纯函数
│       │       ├── TemplateHelpCard.tsx # 模板编写说明折叠卡：变量、字段速查、
│       │       │                       #   最小示例（设置模板子页底部，默认收起）
│       │       ├── PricingSection.tsx  # 峰谷区块：预置/库模型、模型级窗口、订阅说明、
│       │       │                       #   时区与带说明的三档价格编辑（空字段按契约回退）
│       │       ├── pricingDraft.ts / pricingDraft.test.ts
│       │       │                       # 编辑草稿转换、撞名模型显式选择、小额价格精度与
│       │       │                       #   完整自定义判定纯逻辑
│       │       ├── SettingsDialog.tsx  # 常规/更新/数据迁移导航：更新下载进度 +
│       │       │                       #   下载完成后「立即安装」（确认后运行安装包
│       │       │                       #   并退出应用）+ 更新代理端口输入（空=直连）+
│       │       │                       #   系统文件选择器导入导出与双重风险确认
│       │       ├── settingsView.ts / settingsView.test.ts
│       │       │                       # 更新错误优先级、状态结论、进度格式化、
│       │       │                       #   主按钮动作分派（下载中/安装/下载/检查）纯逻辑
│       │       └── configTransferView.ts / configTransferView.test.ts
│       │                               # 迁移默认文件名、扩展名与错误归一化纯逻辑
│       └── src-tauri/          # Rust 后端（crate quota-desktop，入 workspace）
│           ├── tauri.conf.json # 版本经 crate 继承 workspace；CSP 基线；decorations:false；NSIS 安装包
│           ├── capabilities/
│           │   ├── default.json        # 主窗口事件/主题/无装饰窗口控制 + opener +
│           │   │                       #   dialog 打开/保存/确认 ACL
│           │   └── hover-panel.json    # 悬停窗口事件与主题最小 ACL
│           ├── icons/          # 品牌主图导出的 PNG/ICO/ICNS/Windows 尺寸集 + manifest；
│           │                   #   运行时托盘圆环仍由 ring.rs 动态渲染
│           ├── examples/
│           │   └── smoke_setup.rs # GUI 冒烟注入器（沙箱 config.json，手动跑）
│           └── src/
│               ├── main.rs     # 薄壳（release 隐藏控制台）
│               ├── lib.rs      # Builder：单实例/自启/dialog/托盘/窗口隐藏/更新调度/命令注册
│               ├── state.rs    # AppState：引擎+保险库+结果表+resolved_theme+update_ctl
│               │               #   +last_peak 峰谷翻转缓存+--data-dir 覆盖+ErrorInfo
│               │               #   （IPC 错误形状，含脱敏 detail 排查详情）+
│               │               #   history 历史库句柄（M5，Mutex<HistoryStore>，打开
│               │               #   失败降级内存库不阻断启动；DataPaths.history()）
│               ├── commands.rs # 主业务 IPC 20 命令：key 写入策略（空=保持不变）、试查经引擎、
│               │               #   upsert 清结果后即时补查（refetch_and_store 共用，
│               │               #   消除悬停面板等只读视图的无数据空窗；成功分支顺写
│               │               #   历史库 M5，结果表锁外执行、失败仅告警）+ Provider
│               │               #   增删改跨 WebView 失效事件、
│               │               #   快照落盘过滤、设置顺序（磁盘权威）、set_resolved_theme、
│               │               #   更新四命令（检测/下载/install_update 运行安装包后
│               │               #   退出应用）+ 配置导入导出（导入清结果/快照并广播；
│               │               #   M5 起随迁移包携带历史——导出全量带出、导入幂等
│               │               #   合并；remove_provider 同步清条目历史）+
│               │               #   script 双命令（validate_script 干跑/test_script 全链路，
│               │               #   镜像 template 对，key 缺省语义同）；
│               │               #   validate_entry 统一校验（含峰谷配置）、
│               │               #   list_native_metas 携带模型级 plan/windows 与
│               │               #   supports_plan_variant、DeepSeek
│               │               #   CNY/USD 预置套与按 native id 聚类的自定义模型 DTO
│               ├── i18n.rs     # Lang 三态 + sys-locale + 托盘/命令双语文案表（Texts，
│               │               #   含峰谷行/定价错误带参方法）
│               ├── ring.rs     # 托盘圆环渲染纯函数：分层叠弧/阈值色/预设色循环/溢出/
│               │               #   4x6 字模中心文字（tiny-skia 32×32，像素级契约测试）
│               ├── hover_panel.rs # 悬停窗口创建/四边定位/延迟收起状态机 + IPC 命令；
│               │               #   隐藏托盘兜底：光标严格在工作区内（=悬停 flyout
│               │               #   图标，任务栏图标必在工作区外）时改以光标锚定
│               │               #   （面板出现在图标上方、垂直让开整个图标高度）；
│               │               #   垂直空间不足时窗口缩至压缩高度（260，前端
│               │               #   联动裁剪区块）；show 后 SetWindowPos 重插
│               │               #   topmost 压过 flyout（不激活）；真实光标看门狗
│               │               #   兜底上游漏发 Leave，Move 可恢复失步后的首次悬停。
│               │               #   架构备忘：悬停自动刷新链依赖主窗「关闭=隐藏不销毁」
│               │               #   （refresh-now 由主窗响应查询后经 provider-state-changed
│               │               #   回流面板）——若未来改主窗为销毁式，需改为后端直查
│               │               #   引擎并广播结果；面板手动刷新按钮不依赖此链
│               ├── tray.rs     # 托盘：菜单文本（双语参数化）/圆环图标（数据源门控、
│               │               #   「图标显示」子菜单、any_alert 红点、新版本信息行）
│               │               #   /keep-last-good 窗口/峰谷两行（数据行只挂当前展示
│               │               #   条目，其余条目不进菜单防信息行膨胀；峰谷行
│               │               #   resolve_in_currency 接线自定义模型库/查询币种/订阅说明，
│               │               #   pricing_lines 纯函数）+rebuild_on_peak_flip 每分钟翻转检测
│               │               #   （peak_map 全启用条目快照比对，非仅图标条目；翻转时
│               │               #   重建并向 WebView 广播 peak-flip 事件，前端锚点重算 #15）
│               ├── settings.rs # settings.json 读写（原子写、损坏回退；主题/语言三态、
│               │               #   每圈单位、图标数据源、更新检测字段组——开关/时刻/
│               │               #   时间戳/网络代理端口（更新与 use_proxy 条目查询
│               │               #   共用，变更热重建引擎），CLI 共读同一文件）
│               ├── update_ctl.rs # 更新检测控制：状态表（含已下载安装包记录，
│               │               #   资产名随版本变化自动失效；检测失败不丢记录）+
│               │               #   手动/自动检测 + 下载到 %TEMP%/QuotaTray/Downloads
│               │               #   并向前端推送进度/速率（检测与下载共用设置
│               │               #   中的更新代理端口；写入侧校验资产名为纯
│               │               #   文件名 .exe、下载目录 symlink/junction 防御）+
│               │               #   run_installer 运行安装包（运行侧路径防御校验：
│               │               #   限下载目录内 .exe）+ 每分钟调度
│               │               #   （due_check，完成后推送状态事件；设置变更自然
│               │               #   生效；同 tick 顺带峰谷翻转检测）
│               └── snapshot.rs # cache.json 快照（{id:{data,at}}，原子写、容错）
├── examples/
│   ├── templates/             # 声明式模板可运行示例（5 形态：字符串数字单对象/
│   │                          #   双站 baseUrl/总额已用/多窗口/NewAPI 系中转，
│   │                          #   含 README 使用说明与已知边界；前四者经 quota
│   │                          #   template test 实测，newapi 需自备站点）
│   └── scripts/               # JS 脚本查询可运行示例（basic 最小闭环/多窗口
│                              #   字段间运算+reset_at，含 README 协议说明与
│                              #   已知边界，loopback 端到端实测）
└── docs/
    ├── CC-Switch调研报告.md    # cc-switch 代码级调研（技术栈/密钥安全/余额查询）
    ├── 预置Provider缺口预研.md  # 预置平台缺口对照 cc-switch（A/B/C 分层、批次标记与
│                              #   StepFun/Novita/MiniMax/NewAPI 模板移植参考）
    ├── 项目方案预研.md         # 架构、凭据安全、查询体系、CLI/GUI 设计与里程碑
    ├── design/
    │   └── tray-ring-demo.html # 托盘圆环视觉规格（层结构/颜色/溢出/红点定案）
    └── specs/
        ├── CLI-spec.md        # quota-cli 规格（M2b）：子命令/退出码/dev-smoke
        ├── GUI-spec.md        # quota-desktop 规格（M3）：窗口托盘/IPC/快照持久化
        └── history-spec.md    # 历史存储规格（M5）：数据模型/滚动清理/schema
                               #   迁移/迁移容器 v2/CLI 命令（含 SQLite 决策边界）
```

（`src-tauri/gen/schemas` 为构建生成物，被 gitignore）

**并行开发约定**（2026-08-23 起）：core 的 M2 API 面已冻结（M2a 完成）。
CLI（M2b）与 GUI（M3）双工作树并行开发，共享文件仅 workspace
`Cargo.toml`、CI 与本文件树——先合的 PR 为准，后合的 rebase 更新文件树即可。
core 若需变更公开 API，先单独提 PR 合入再同步两端。
M3 期间 core 的 template/http 曾随桌面端 PR 做错误文案安全修复
（只增 `uses_api_key` 公开函数与测试，不改既有签名）。
M4-a（CLI i18n #4 / GUI 主题+圆环+i18n+标题栏 #5）沿用此约定：
CLI 先合，GUI rebase 后合并同步本文件树；Lang 枚举两端各自实现（core 不动）。

## 工程规范

- 通用行为准则、提交规范（中文、`类型: 简述` + 正文）、发布规范遵循用户全局 AGENTS.md，此处不重复。
- **TDD**：实现功能、修复 BUG 前先添加契约测试；网络相关测试一律 mock（不依赖真实平台 API）。
- **提交前格式化与静态检查（硬门禁）**：Rust 改动先 `cargo fmt --all`，再 `cargo clippy --workspace --all-targets -- -D warnings`（`--all-targets` 含 examples/测试，CI 同口径——漏跑会让 main 编译债拖垮后续所有 PR 的 CI）；前端改动先 `pnpm lint --fix`。CI 的 `cargo fmt --all --check` 作用于全 workspace（2026-08-24 v0.3.2 遗留三处未格式化、2026-08-25 v0.4.2 后三处 clippy 失败即为此例）。
- **构建与测试**：
  - 全量构建检查：`cargo build --workspace`（仅编译校验）
  - 测试：`cargo test --workspace`
  - 前端：`pnpm lint` / `pnpm build`（于 `apps/quota-desktop`，build 含 tsc 检查）
  - 桌面端开发：`pnpm tauri dev`（于 `apps/quota-desktop`）
  - 桌面端产物：`pnpm tauri build --no-bundle`（出裸 exe；完整打包 M4）
  - ⚠️ 裸 `cargo build`（含 --release）的桌面端产物指向 devUrl（1420），
    无 vite dev server 时窗口空白——运行/分发一律走 tauri CLI
  - GUI 冒烟：`cargo run -p quota-desktop --example smoke_setup -- --data-dir <沙箱>
    --key-file <.DevApiKey.json>` 注入后以 `--data-dir` 启动 exe 验证
  - 开发目录清理：仓库根执行 `.\clean 1|2|3`；先预览用 `.\clean 3 -WhatIf`
  - 清理器契约测试：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/clean.tests.ps1`
- **发布惯例**：每个 Release 必须附带桌面端安装包——先把 workspace `Cargo.toml`
  版本号改为目标版本，再于 `apps/quota-desktop` 跑 `pnpm tauri build`，
  产物取 NSIS `target/release/bundle/nsis/*-setup.exe` 随 `gh release create`
  上传（安装包版本随 crate 继承 workspace，先改版本再构建）。
- 文档用中文编写。

## 安全红线（凭据处理）

本项目以"凭据不落明文"为差异化设计，以下为硬性红线，违反即 bug：

1. **主密钥与凭据**只允许存在于：系统凭据库（运行时取用）、内存、AES-GCM 密文。任何日志、错误信息、调试输出不得包含明文或密钥材料。
2. **源码零密钥**：不得硬编码任何密钥、盐、派生参数；配置文件中凭据字段必须是密文（`v1:<base64>` 格式，含版本号以便未来算法升级）。
3. **前端/GUI 永不接收明文凭据**：查询由 core 在后端完成，GUI 只展示结果；编辑凭据时走"写入专用"通道（空值 = 保持不变，不回显）。
4. **机器主密钥永不导出**。普通 `config.json` 不含任何解密能力，离开本机不可解；显式生成的 `.qtray-export` 迁移包例外携带每次导出新生成的一次性迁移密钥，敏感级别等同明文凭据。CLI/GUI 接入导出时必须在写文件前显式警告并建议用户迁移后删除。

## 术语表

| 术语 | 含义 |
|---|---|
| 主密钥（KEK） | 首次运行随机生成的 32 字节密钥，存系统凭据库，仅用于加解密配置中的凭据字段 |
| 一次性迁移密钥 | 每次导出随机生成的 32 字节密钥；源凭据先转写到该密钥，密钥随 `.qtray-export` 包携带，导入后再转写到目标机器主密钥 |
| 配置迁移包 | QuotaTray 私有、带版本和认证校验的二进制配置导出；虽然不可直接阅读，但因携带迁移密钥，保密级别等同明文凭据 |
| 预置平台（native provider） | core 内置 Rust 实现的官方查询（如 DeepSeek、SiliconFlow），随版本发布 |
| 声明式模板（template provider） | JSON 描述的查询配置（URL/头/字段映射/算术），零代码 |
| 脚本查询（script provider） | QuickJS 沙箱内运行的 `{request, extractor}` 脚本，兜底复杂平台 |
| 瞬时失败 / 确定性失败 | 网络抖动类错误（可重试、保留旧值）vs 认证/解析类错误（立即透出） |
| keep-last-good | 查询失败时在时限内继续展示上次成功结果的策略 |
| 峰谷定价 | 按「周几+时间段」划分高峰/空闲时段并配两档三价（缓存命中/未命中/输出，每 MTokens）的展示配置：预置随版本内置（DeepSeek），条目可字段级自定义（空=回退预置） |
| 历史库 | `~/.quotatray/history.db`（SQLite）：每次成功查询的余额/额度快照时序表，滚动保留 30 天，schema 走 user_version 版本化迁移 |
| 窗口键（window_key） | 历史行的窗口标识：`plan_name` 非空取之，否则回退序数 `w0/w1…`；同一多窗口条目每窗口一条时间线 |
| 迁移容器 v2 | `.qtray-export` 第 2 版信封 `{config, history}`：随配置携带历史数值行（幂等合并进目标机历史库）；v1 旧包仍可导入，旧二进制读 v2 拒绝 |
