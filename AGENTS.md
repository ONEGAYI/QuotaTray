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

<!-- 文件树由 .agents/skills/file-tree 技能维护：增删改查一律走其 scripts/tree_tool.py，本树块为渲染产物禁止手改 -->

```
<!-- file-tree:full:begin 由脚本渲染，禁止手改 -->
QuotaTray/
├── .agents/                # Agent 技能库（项目级）
│   └── skills/ # 技能目录
│       └── file-tree/ # 项目文件树唯一数据源与维护入口
│                      #   tree.json 为源，SKILL.md/AGENTS.md 树块为渲染产物
│                      #   脚本保证字典序与序列化确定性，git diff 稳定
│           ├── agents/   # Codex 元数据目录
│           │   └── openai.yaml # display_name / short_description / default_prompt
│           ├── scripts/  # 技能脚本
│           │   ├── tree_tool.py      # 子命令 add/rm/get/query/tag-add/tag-rm/check/render
│           │   │                     #   写操作后自动字典序规范化并重渲染产物
│           │   │                     #   check 校验规范形态/词表/rel/磁盘对照/产物一致
│           │   └── tree_tool_test.py # unittest 沙箱契约：排序/渲染快照/check 不变量/自举
│           ├── SKILL.md  # 用法说明 + 字段说明 + 多树冲突规则；不承载渲染数据
│           └── tree.json # 顶层 {tags, tree}；嵌套 = 目录；条目字段 desc/detail/rel/tags/children
│                         #   禁止手改——增删改查一律走 scripts/tree_tool.py
├── .DevApiKey.json.example # 本地密钥文件模板（真实文件被 ignore）
├── .gitattributes          # file-tree 技能数据与产物固定 LF，跨机器渲染字节一致
├── .github/                # GitHub 配置
│   └── workflows/ # CI 工作流
│       └── ci.yml # CI：双矩阵 fmt+clippy+test（Ubuntu 含 Tauri 依赖）+ 前端 lint/build
├── .gitignore              # 含 .DevApiKey.json / .DevApiKey.json 被忽略、前端与 gen/schemas 生成物、
│                           #   Python 字节码（file-tree 技能脚本）
├── AGENTS.md               # 本文件：项目规则单一事实源（设计决策快照/工程规范/安全红线/术语表）
│                           #   文件树章节由 file-tree 技能脚本渲染，禁止手改树块
├── apps/                   # 应用层（CLI 与桌面端）
│   ├── quota-cli/     # CLI 前端（bin 名 quota，M2b 完成；i18n 三态 + 更新检测）
│   │   ├── Cargo.toml # CLI crate 依赖与 bin（quota）声明
│   │   └── src/       # CLI 源码
│   │       ├── cmd/           # 子命令实现（每命令一模块，handler 收 Ctx；文案走 texts.rs）
│   │       │   ├── add.rs            # 交互向导（订阅型平台问套餐变体；template 粘贴/
│   │       │   │                     #   script 代码粘贴 + allowInsecure 确认双形态）/
│   │       │   │                     #   --json stdin（拒收 api_key_enc，script 干跑校验）
│   │       │   ├── config.rs         # config export/import：高敏感确认、完整配置迁移、
│   │       │   │                     #   目标机器 Vault 重加密与失败不覆盖契约测试；
│   │       │   │                     #   迁移包携带查询历史（M5）——导出全量带出、
│   │       │   │                     #   导入按主键幂等合并，读失败降级不带不阻断
│   │       │   ├── devsmoke.rs       # 仅 debug：读 .DevApiKey.json 走完整链路（CLI 凭据型
│   │       │   │                     #   平台条目仅作开关、跳过 set_api_key；--proxy 让条目走
│   │       │   │                     #   settings.json 代理端口，真机验证代理通道；OK 行带 extra=
│   │       │   │                     #   原始窗口 JSON，便于核对响应结构）
│   │       │   ├── edit.rs           # 向导（回车保持，套餐变体可改；template/script
│   │       │   │                     #   各自的 baseUrl、内容重粘贴与 script 的 allowInsecure 修改）+
│   │       │   │                     #   --enable/--disable 快捷路径
│   │       │   ├── history.rs        # history show/clear（M5）：三档范围（24h=15m 桶/7d=1h 桶/30d=6h 桶，桶内取最后点）
│   │       │   │                     #   + 窗口语义过滤（--window 三态：all/类别别名/精确键优先，
│   │       │   │                     #   缺省按范围选粒度 24h→5h、7d/30d→周，缺失回退全部并条件提示）
│   │       │   │                     #   + 分页（默认 20 行；终端交互翻页，--page N 非交互打印单页，
│   │       │   │                     #   管道整表，跨类别视图分段表头；--json 原始点含 window 生效口径）；
│   │       │   │                     #   clear 无 id 清全部（确认默认否）
│   │       │   ├── list.rs           # 条目列表（表格 / --json providers 数组）
│   │       │   ├── mod.rs            # devsmoke 仅 debug 编入
│   │       │   ├── natives.rs        # 预置平台表（含峰谷预置标记列）
│   │       │   ├── pricing.rs        # pricing show/set/clear：生效定价展示（判定/价格对照表/
│   │       │   │                     #   时段聚合/下次翻转，now 注入纯函数；show 接线自定义模型库 +
│   │       │   │                     #   条目 currency 币种 hint 选 DeepSeek 双币套 + plan 透出/订阅说明行）、
│   │       │   │                     #   stdin JSON 校验写入、清除回退预置
│   │       │   ├── pricing_models.rs # pricing model list/add/remove：自定义模型库管理
│   │       │   │                     #   （表格价格对照/同 id 覆盖/删空移键，纯函数可测）
│   │       │   ├── query.rs          # 并行查询 + watch 轮询 + 退出码聚合（RouteHttp 全链测试）；
│   │       │   │                     #   成功结果写历史库（M5，打开/写失败仅告警不改退出码）
│   │       │   ├── remove.rs         # 确认删除（--yes 跳过；M5 起删除后同步清条目历史，
│   │       │   │                     #   失败仅告警不影响删除结果）
│   │       │   ├── script.rs         # script test：干跑校验先行 + 真实试查（--json
│   │       │   │                     #   stdin 双形态：config JSON 或纯 JS 文本，{ 开头解析失败提示
│   │       │   │                     #   疑似误输；key 在校验通过后按需交互收集，空 key 按终端/重定向
│   │       │   │                     #   分派提示）
│   │       │   ├── setkey.rs         # 隐藏读 key → vault 加密写配置
│   │       │   ├── template.rs       # template test：静态校验 + 真实试查
│   │       │   ├── update.rs         # update：检测 GitHub release + 可选下载（--check/--yes/
│   │       │   │                     #   --output；交互终端实时进度/速率；http 与 downloader
│   │       │   │                     #   可注入测试；退出码三分；更新代理端口读自 settings.json
│   │       │   │                     #   ——GUI 设置页写入，检测/下载共用并打印提示行）
│   │       │   └── vault.rs          # vault status：主密钥健康检查
│   │       ├── ctx.rs         # Ctx：配置路径 + SecretStore 注入 + lang 字段 +
│   │       │                  #   history_path 推导（config 同目录 history.db，M5）
│   │       ├── exit.rs        # 退出码三分约定（0 全成功 / 1 确定性 / 2 仅瞬时）
│   │       ├── idgen.rs       # 6 位 Crockford base32 随机 id（无偏映射）
│   │       ├── io.rs          # 交互薄层：掩码读 key（星号回显、Ctrl+V 剪贴板粘贴、管道分流）、
│   │       │                  #   多行读取双约定——模板 JSON 空行结束，脚本代码单独一行 . 结束
│   │       │                  #   （空行照常保留），管道均读到 EOF
│   │       ├── lang.rs        # Lang 三态（zh/en/system）+ sys-locale 检测 +
│   │       │                  #   settings.json language 读取（mini struct，容错回退 System）
│   │       ├── main.rs        # clap 定义子命令（含 pricing model / config 迁移组）+ dispatch
│   │       │                  #   + --lang 全局参数（两阶段解析）+ 启动更新提示钩子
│   │       │                  #   （stderr、节流、--json 与 update 子命令自身豁免）
│   │       ├── render.rs      # comfy-table 表格 + query --json 输出结构（纯函数可测、文案双语；
│   │       │                  #   error 含 detail 排查详情 additive 透出）+
│   │       │                  #   重置倒计时列（fmt_reset_countdown，now 注入）+
│   │       │                  #   pricing 价格对照表/星期连续段聚合/UTC 偏移描述 +
│   │       │                  #   history 时间桶聚合/分页切片/走势表 + 类别排序分组与分段表头渲染
│   │       │                  #   （M5，纯函数）
│   │       ├── settings_io.rs # settings.json 的 update 字段读取（mini struct：开关/时刻/
│   │       │                  #   时间戳/代理端口）+ last_check 写回（Value 读改写保留
│   │       │                  #   未知字段 + 原子写）
│   │       └── texts.rs       # 双语文案表（TextKey exhaustive，漏译即编译错误）+
│   │                          #   带参文案函数 + clap about/help 运行时翻译
│   └── quota-desktop/ # 桌面端（M3 完成）：Tauri 2 + React，GUI 为薄层
│       ├── eslint.config.js    # ESLint 扁平配置（React hooks/TS 规则，pnpm lint 入口）
│       ├── index.html          # Vite HTML 入口（挂载 #root）
│       ├── package.json        # pnpm：React 18/Vite/Tailwind 4/React Query 5/CodeMirror/
│       │                       #   （lang-json + lang-javascript）/Lucide/Vitest/opener +
│       │                       #   dialog（系统文件选择器）
│       ├── pnpm-lock.yaml      # pnpm 依赖锁定（入库保证可复现安装）
│       ├── pnpm-workspace.yaml # pnpm 11 构建脚本许可（esbuild）
│       ├── src/                # React 前端（zh/en 双语 + 明暗主题三态）
│       │   ├── api.ts                  # invoke 封装 + 短 id 生成 + 主题/更新/配置迁移命令
│       │   ├── App.tsx                 # Calm Native 主布局（标题栏/账户与使用统计页签/列表/历史占位），
│       │   │                           #   编辑时传递查询币种；双面板常挂载、hidden 切换——换页不卸载
│       │   │                           #   卡片，不触发重查也不丢展开态
│       │   ├── assets/                 # 静态资源
│       │   │   ├── brand-mark.png # 透明品牌主图：四段额度环 + 右下 Q 形拖尾
│       │   │   └── providers/     # 十四张官方 Provider SVG（六组复用品牌 + stepfun/novita/
│       │   │                      #   minimax + 订阅四家 claude/openai(codex 用)/gemini/grok；
│       │   │                      #   minimax + newapi 供模板条目启发匹配；图标容器固定浅底
│       │   │                      #   不随主题——单色深色 logo 明暗主题均可见，浅色底图形
│       │   │                      #   （StepFun 白色圆盘）走 is-light-logo 深底变体）
│       │   ├── components/             # 前端组件
│       │   │   ├── BrandMark.tsx                # 标题栏/悬停面板共用的静态品牌标志薄组件
│       │   │   ├── configTransferView.test.ts   # 迁移默认文件名/扩展名/错误归一化的 Vitest 契约测试
│       │   │   ├── configTransferView.ts        # 迁移默认文件名、扩展名与错误归一化纯逻辑
│       │   │   ├── EditDialog.tsx               # Modal：native 下拉（订阅型带套餐变体选择）/
│       │   │   │                                #   template 分支二级子页（运营商与模型＝名称/baseUrl/key/
│       │   │   │                                #   峰谷计价；设置模板＝预设模板+JSON 编辑器+校验/试查+
│       │   │   │                                #   编写说明卡。子页 A 用 CSS 隐藏切换保草稿态不丢）/
│       │   │   │                                #   script JS 编辑器（M4：校验/试查镜像 template，
│       │   │   │                                #   allowInsecure 开关带警告 + 默认最小闭环示例）、
│       │   │   │                                #   分组表单、独立凭据区与固定页脚
│       │   │   ├── HoverPanel.tsx               # 托盘悬停浮窗 A 方案：余额/额度/峰谷详情 +
│       │   │   │                                #   圆环数据源账户与计价模型即时切换、头部刷新/关闭按钮、
│       │   │   │                                #   低垂直空间压缩布局、hero 多窗口标签与用量行重置倒计时
│       │   │   ├── hoverPanelView.test.ts       # 悬停条目回退/圆环镜像/压缩视口判定的 Vitest 契约测试
│       │   │   ├── hoverPanelView.ts            # 悬停条目回退、前端圆环镜像与压缩视口判定
│       │   │   │                                #   （联动后端缩窗）纯逻辑
│       │   │   ├── MainPanelTabs.tsx            # 账户/使用统计页签鼠标聚光：按鼠标到文字中心距离
│       │   │   │                                #   独立显现径向柔光，离开区域完全隐藏
│       │   │   ├── mainPanelTabsView.test.ts    # 页签聚光视图逻辑的 Vitest 契约测试
│       │   │   ├── mainPanelTabsView.ts         # 鼠标聚光视图纯逻辑：距离计算与柔光显现参数
│       │   │   ├── nativeProviderGroups.test.ts # 平台分组/顺序/兜底的 Vitest 契约测试
│       │   │   ├── nativeProviderGroups.ts      # native id 的平台分组、稳定顺序与未知项兜底
│       │   │   ├── NativeProviderPicker.tsx     # 添加/编辑平台聚合选择器：SVG 一级菜单+悬停/键盘展开
│       │   │   │                                #   二级 Provider 选单；菜单为 fixed 浮层不占文档流，
│       │   │   │                                #   高度自适应主窗口（min(剩余空间, 460px)），
│       │   │   │                                #   左右双栏 minmax(0,1fr)+min-height:0 各自独立滚动
│       │   │   │                                #   （滚动条走全局主题变量）
│       │   │   ├── presetTemplates.test.ts      # 预设库形态与等价高亮判定的 Vitest 契约测试
│       │   │   ├── presetTemplates.ts           # 模板编辑器预设库（6 形态：通用/单对象余额/站点可变/
│       │   │   │                                #   总额已用/多窗口/NewAPI 系中转——New-Api-User 头写占位值
│       │   │   │                                #   用户手改、quota÷500000 换算 USD，与 examples/templates
│       │   │   │                                #   同构）+ matchedPresetId 语义等价高亮判定
│       │   │   │                                #   （serde 往返补全键不算改动）纯函数
│       │   │   ├── pricingDraft.test.ts         # 草稿转换/撞名/精度判定的 Vitest 契约测试
│       │   │   ├── pricingDraft.ts              # 编辑草稿转换、撞名模型显式选择、小额价格精度与
│       │   │   │                                #   完整自定义判定纯逻辑
│       │   │   ├── PricingSection.tsx           # 峰谷区块：预置/库模型、模型级窗口、订阅说明、
│       │   │   │                                #   时区与带说明的三档价格编辑（空字段按契约回退）
│       │   │   ├── ProviderCard.tsx             # 余额优先卡片：悬停/窄屏展开、按币种峰谷三价、
│       │   │   │                                #   条目级代理开关（Globe 按钮）、CLI 凭据型脚注文案
│       │   │   │                                #   （「凭据来自本机 CLI」），订阅积分语义、预置/库模型
│       │   │   │                                #   即时切换、多窗口逐窗主数值（短标签）+重置倒计时小字+
│       │   │   │                                #   短时反馈、启停/编辑/删除确认；错误行带「复制报错信息」
│       │   │   │                                #   图标（Tooltip，复制脱敏详情）
│       │   │   ├── providerCardView.test.ts     # 卡片各视图状态机的 Vitest 契约测试
│       │   │   ├── providerCardView.ts          # 卡片正常/错误/keep-last-good/快照/多窗口视图纯逻辑
│       │   │   │                                #   （errorDetail 排查详情随查询错误态透传）
│       │   │   ├── providerIcon.test.ts         # 图标映射与浅色 logo 判定的 Vitest 契约测试
│       │   │   ├── providerIcon.ts              # 预置 Provider id → 官方 SVG 映射与未知项回退契约 +
│       │   │   │                                #   浅色 logo 判定（isLightLogo → 容器深底变体）+
│       │   │   │                                #   模板/脚本条目按名启发（templateProviderIconUrl，
│       │   │   │                                #   条目名含 newapi → NewAPI 品牌图）
│       │   │   ├── providerPricing.test.ts      # resolve_with 镜像解析的 Vitest 契约测试
│       │   │   ├── providerPricing.ts           # 镜像 resolve_with：模型级窗口/订阅/币种套/
│       │   │   │                                #   自定义模型库解析、峰谷判定与模型切换纯逻辑
│       │   │   ├── SettingsDialog.tsx           # 常规/更新/数据迁移导航：更新下载进度 +
│       │   │   │                                #   下载完成后「立即安装」（确认后运行安装包并退出应用）+
│       │   │   │                                #   更新代理端口输入（空=直连）+系统文件选择器导入导出
│       │   │   │                                #   与双重风险确认
│       │   │   ├── settingsView.test.ts         # 设置视图状态分派的 Vitest 契约测试
│       │   │   ├── settingsView.ts              # 更新错误优先级、状态结论、进度格式化、
│       │   │   │                                #   主按钮动作分派（下载中/安装/下载/检查）纯逻辑
│       │   │   ├── TemplateHelpCard.tsx         # 模板编写说明折叠卡：变量、字段速查、
│       │   │   │                                #   最小示例（设置模板子页底部，默认收起）
│       │   │   ├── TitleBar.tsx                 # 自定义标题栏：拖动/双击最大化、GitHub 仓库链接
│       │   │   │                                #   （opener 插件，scope 锁定仓库主页）、语言与主题图标
│       │   │   │                                #   下拉三选（即时保存）、窗口控制按钮
│       │   │   └── ui.tsx                       # 按钮/菜单/徽标/开关/Tooltip/Dialog 等共享基础组件
│       │   │                                    #   （Dialog 以 body Portal 保证主窗居中，含 Escape、
│       │   │                                    #   焦点圈定与关闭后焦点恢复）
│       │   ├── display.test.ts         # 时间/百分比/文案格式化的 Vitest 契约测试
│       │   ├── display.ts              # 相对/精确时间、已用百分比、数据文案（双语，与 tray.rs 成对）、
│       │   │                           #   重置倒计时/多窗口短标签（与 CLI fmt_reset_countdown 成对）、
│       │   │                           #   条目类型标签 kindLabel（与 CLI kind_label 成对）
│       │   ├── i18n/                   # 轻量自写 i18n
│       │   │   ├── en.ts     # 英文字典（Record<TextKey,string> 编译期锁键完整）
│       │   │   ├── index.tsx # LangProvider + resolveUiLang + TextKey re-export
│       │   │   └── zh.ts     # 中文字典（as const 类型基准）
│       │   ├── index.css               # 明暗设计令牌（暗色 #161616 中性系；color-scheme 随主题；
│       │   │                           #   全局 webkit 滚动条暗色 #2e2e2e）、Mica-like 基底、
│       │   │                           #   主窗响应式系统 + 悬停面板样式
│       │   ├── main.tsx                # 入口按 URL 分派主窗/悬停窗
│       │   ├── mainPanelView.test.ts   # 面板切换状态机的 Vitest 契约测试
│       │   ├── mainPanelView.ts        # 主窗口面板切换状态机：下沉模糊峰值换内容后上浮清晰，
│       │   │                           #   连续点击以最后选择为准
│       │   ├── queries.test.ts         # 轮询/失效/快照 hooks 的 Vitest 契约测试
│       │   ├── queries.ts              # React Query hooks：轮询（staleTime 对齐轮询周期，重挂载/
│       │   │                           #   恢复聚焦不追加查询）/快照/refresh-now/自动更新状态事件 +
│       │   │                           #   Provider 变更按条目 id 失效（其余条目不陪查）/配置导入
│       │   │                           #   全量失效跨窗口契约 + CLI 可改 native/custom model 短缓存 +
│       │   │                           #   usePeakFlipTick 峰谷翻转事件锚点（#15，常驻视图重算标签）
│       │   ├── theme.tsx               # ThemeProvider：三态解析、system 实时跟随、setTheme 联动
│       │   │                           #   （跟随变色同样走扩散动效，含 setTheme(null) 翻转
│       │   │                           #   prefers-color-scheme 的回声防御——比对 DOM class）
│       │   ├── themeTransition.test.ts # 扩散动效纯函数与时序契约的 Vitest 测试
│       │   ├── themeTransition.ts      # 主题切换圆形扩散动效（View Transitions）：CSS 变量传参 +
│       │   │                           #   样式表关键帧（首帧即 circle(0px) 裁剪）、圆心取主题按钮锚点
│       │   │                           #   （data-theme-trigger）、reduce-motion/不支持退化瞬时切换；
│       │   │                           #   纯函数与更新回调时序契约测试
│       │   ├── types.ts                # core serde 形状的 TS 镜像（模型级 plan/windows、PlanVariant、
│       │   │                           #   reset_at、自定义模型库/按币种预置 DTO、更新下载进度/
│       │   │                           #   已下载路径、KEEP_LAST_GOOD_MS）
│       │   └── vite-env.d.ts           # Vite 静态资源（品牌 PNG 等）模块类型声明
│       ├── src-tauri/          # Rust 后端（crate quota-desktop，入 workspace）
│       │   ├── build.rs        # tauri-build：能力清单/图标/资源嵌入构建期生成
│       │   ├── capabilities/   # 权限 ACL
│       │   │   ├── default.json     # 主窗口事件/主题/无装饰窗口控制 + opener +
│       │   │   │                    #   dialog 打开/保存/确认 ACL
│       │   │   └── hover-panel.json # 悬停窗口事件与主题最小 ACL
│       │   ├── Cargo.toml      # 版本号经 workspace 继承
│       │   ├── examples/       # 示例注入器
│       │   │   └── smoke_setup.rs # GUI 冒烟注入器（沙箱 config.json，手动跑）
│       │   ├── icons/          # 品牌主图导出的 PNG/ICO/ICNS/Windows 尺寸集 + manifest；
│       │   │                   #   运行时托盘圆环仍由 ring.rs 动态渲染
│       │   ├── src/            # 后端源码
│       │   │   ├── commands.rs    # 主业务 IPC 20 命令：key 写入策略（空=保持不变）、
│       │   │   │                  #   试查经引擎、upsert 清结果后携条目 id 广播 providers-changed，
│       │   │   │                  #   由主窗按条目失效驱动卡片重查（卡片常挂载保证观察者在；
│       │   │   │                  #   后端不补查——与前端失效驱动会并发双查同一平台 API；
│       │   │   │                  #   成功分支顺写历史库 M5，结果表锁外执行、失败仅告警）+
│       │   │   │                  #   Provider 增删改跨 WebView 失效事件（payload 条目 id）、
│       │   │   │                  #   快照落盘过滤、设置顺序（磁盘权威）、set_resolved_theme、
│       │   │   │                  #   更新四命令（检测/下载/install_update 运行安装包后
│       │   │   │                  #   退出应用）+ 配置导入导出（导入清结果/快照并广播；
│       │   │   │                  #   M5 起随迁移包携带历史；remove_provider 同步清条目历史）+
│       │   │   │                  #   script 双命令（validate_script 干跑/test_script 全链路，
│       │   │   │                  #   镜像 template 对，key 缺省语义同）；
│       │   │   │                  #   validate_entry 统一校验（含峰谷配置）、
│       │   │   │                  #   list_native_metas 携带模型级 plan/windows 与
│       │   │   │                  #   supports_plan_variant、DeepSeek CNY/USD 预置套与
│       │   │   │                  #   按 native id 聚类的自定义模型 DTO
│       │   │   ├── hover_panel.rs # 悬停窗口创建/四边定位/延迟收起状态机 + IPC 命令；
│       │   │   │                  #   隐藏托盘兜底：光标严格在工作区内（=悬停 flyout 图标，
│       │   │   │                  #   任务栏图标必在工作区外）时改以光标锚定（面板出现在图标
│       │   │   │                  #   上方、垂直让开整个图标高度）；垂直空间不足时窗口缩至
│       │   │   │                  #   压缩高度（260，前端联动裁剪区块）；show 后 SetWindowPos
│       │   │   │                  #   重插 topmost 压过 flyout（不激活）；真实光标看门狗
│       │   │   │                  #   兜底上游漏发 Leave，Move 可恢复失步后的首次悬停。
│       │   │   │                  #   悬停只管浮层不触发查询（纯显示操作）；数据新鲜度
│       │   │   │                  #   由后台轮询与面板手动刷新按钮（单条 queryProvider）兜底
│       │   │   ├── i18n.rs        # Lang 三态 + sys-locale + 托盘/命令双语文案表（Texts，
│       │   │   │                  #   含峰谷行/定价错误带参方法）
│       │   │   ├── lib.rs         # Builder：单实例/自启/dialog/托盘/窗口隐藏/
│       │   │   │                  #   更新调度/命令注册
│       │   │   ├── main.rs        # 薄壳（release 隐藏控制台）
│       │   │   ├── ring.rs        # 托盘圆环渲染纯函数：分层叠弧/阈值色/预设色循环/溢出/
│       │   │   │                  #   4x6 字模中心文字（tiny-skia 32×32，像素级契约测试）
│       │   │   ├── settings.rs    # settings.json 读写（原子写、损坏回退；主题/语言三态、
│       │   │   │                  #   每圈单位、图标数据源、更新检测字段组——开关/时刻/
│       │   │   │                  #   时间戳/网络代理端口（更新与 use_proxy 条目查询共用，
│       │   │   │                  #   变更热重建引擎），CLI 共读同一文件）
│       │   │   ├── snapshot.rs    # cache.json 快照（{id:{data,at}}，原子写、容错）
│       │   │   ├── state.rs       # AppState：引擎+保险库+结果表+resolved_theme+update_ctl
│       │   │   │                  #   +last_peak 峰谷翻转缓存+--data-dir 覆盖+ErrorInfo
│       │   │   │                  #   （IPC 错误形状，含脱敏 detail 排查详情）+
│       │   │   │                  #   history 历史库句柄（M5，Mutex<HistoryStore>，打开
│       │   │   │                  #   失败降级内存库不阻断启动；DataPaths.history()）
│       │   │   ├── tray.rs        # 托盘：菜单文本（双语参数化）/圆环图标（数据源门控、
│       │   │   │                  #   「图标显示」子菜单、any_alert 红点、新版本信息行）
│       │   │   │                  #   /keep-last-good 窗口/峰谷两行（数据行只挂当前展示条目，
│       │   │   │                  #   其余条目不进菜单防信息行膨胀；峰谷行 resolve_in_currency
│       │   │   │                  #   接线自定义模型库/查询币种/订阅说明，pricing_lines 纯函数）
│       │   │   │                  #   +rebuild_on_peak_flip 每分钟翻转检测（peak_map 全启用
│       │   │   │                  #   条目快照比对，非仅图标条目；翻转时重建并向 WebView 广播
│       │   │   │                  #   peak-flip 事件，前端锚点重算 #15）
│       │   │   └── update_ctl.rs  # 更新检测控制：状态表（含已下载安装包记录，资产名随
│       │   │                      #   版本变化自动失效；检测失败不丢记录）+手动/自动检测 +
│       │   │                      #   下载到 %TEMP%/QuotaTray/Downloads 并向前端推送进度/速率
│       │   │                      #   （检测与下载共用设置中的更新代理端口；写入侧校验资产名
│       │   │                      #   为纯文件名 .exe、下载目录 symlink/junction 防御）+
│       │   │                      #   run_installer 运行安装包（运行侧路径防御校验：
│       │   │                      #   限下载目录内 .exe）+ 每分钟调度（due_check，完成后推送
│       │   │                      #   状态事件；设置变更自然生效；同 tick 顺带峰谷翻转检测）
│       │   └── tauri.conf.json # 版本经 crate 继承 workspace；CSP 基线；decorations:false；
│       │                       #   NSIS 安装包
│       ├── tsconfig.json       # TypeScript 编译选项（React JSX、严格模式）
│       └── vite.config.ts      # 端口 1420 固定、chrome110 目标、Tailwind 插件
├── Cargo.lock              # workspace 依赖锁定版本（含 Tauri 链路）
├── Cargo.toml              # workspace 根：成员、共享依赖版本、release 配置
├── CHANGELOG.md            # 版本变更记录（Keep a CHANGELOG，按 PR 归纳）
├── CLAUDE.md               # @AGENTS.md 导入主文件，仅附加 Claude 专属补充规则
├── clean.cmd               # Windows 开发目录清理入口：交互或 Level 1/2/3
├── crates/                 # workspace crates 根
│   └── quota-core/ # 业务核心库（无 UI 依赖）
│       ├── Cargo.toml # core crate 依赖声明（serde/reqwest/rusqlite/rquickjs 等）
│       └── src/       # core 源码
│           ├── config/    # 配置层
│           │   ├── mod.rs      # AppConfig（providers + custom_models 自定义模型库；
│           │   │               #   原子写、密文落盘、旧文件兼容）+ ProviderEntry
│           │   │               #   （含 use_proxy 条目级查询代理开关，默认 false）
│           │   ├── provider.rs # Credentials / ProviderKind（serde tag 分派：native/
│           │   │               #   template/script，script 为 M4 新增——旧版二进制
│           │   │               #   读到会报未知 tag，升级单向）、PlanVariant
│           │   │               #   订阅套餐变体（auto/no_weekly/weekly，智谱 v1 无周限）
│           │   └── transfer.rs # 完整配置跨机器迁移：一次性迁移密钥转写、私有认证
│           │                   #   二进制容器、字节 API 与原子文件导入导出；
│           │                   #   容器 v2（M5）信封 {config, history} 携带历史
│           │                   #   数据（v1 仍可导入，单向升级）、TransferBundle
│           │                   #   返回 config + 可选历史行
│           ├── history/   # 历史数据存储（M5）
│           │   └── mod.rs # HistoryStore：成功查询结果时序落库（一条 UsageData 一行，
│           │              #   PK provider_id+window_key+sampled_at，同毫秒重放幂等，
│           │              #   重复窗口键按出现顺序 #2/#3 消歧、is_valid=false 跳过）；
│           │              #   window_key 纯函数（plan_name 优先、序数 w0/w1 回退，
│           │              #   native 文案冻结为键）+ window_kind 键文本→语义类别
│           │              #   （5h/周/其他，冻结键括号短标注启发式，CLI 分组过滤用、
│           │              #   M5-b GUI 复用）；PRAGMA user_version + MIGRATIONS 数组
│           │              #   逐版本事务迁移（降级运行/负版本拒绝打开）；
│           │              #   busy_timeout 3s 防并发写 database is locked；
│           │              #   30 天滚动清理（写入时节流 ≥1h 一次）；WAL + synchronous
│           │              #   NORMAL；export_rows/merge_rows 供迁移容器导入导出；
│           │              #   非关键数据——调用方写失败静默告警不阻断查询主链路
│           ├── http/      # HTTP 抽象
│           │   ├── mod.rs     # HttpClient trait + 请求/响应/错误类型（Debug 打码，
│           │   │              #   敏感头判断统一走 redact；HttpResponse.raw 为
│           │   │              #   二进制协议字节保真通道，gRPC-web 必走）
│           │   ├── redact.rs  # 错误详情脱敏（参考 opencode）：结构化正则 + 本次请求
│           │   │              #   密钥字面量两遍清洗，先清洗后截断（2048 字符）
│           │   └── reqwest.rs # 生产实现（rustls；错误去 URL 防凭据泄漏；
│           │                  #   new_with_proxy 供更新通道注入可选代理）
│           ├── lib.rs     # 模块声明与 re-export（对外 API 面单一出口）
│           ├── model.rs   # UsageData（含窗口重置时刻 reset_at）/ QueryError 双轨分类
│           │              #   （可选 detail 排查详情，Display 不输出），附契约单测
│           ├── pricing.rs # 峰谷定价：周几+时间段判定与下次翻转（epoch ms 纯函数、
│           │              #   UTC 偏移/本地时区）、三档价格（缓存命中/未命中/输出，
│           │              #   每 MTokens）、计费模式（按量三档价/订阅积分项）、
│           │              #   预置：DeepSeek 单站双币 + Kimi 开放平台国内/国际 +
│           │              #   Kimi Code 国内/国际订阅额度 + 智谱/Z.ai 通用 API
│           │              #   国内/国际按量模型 + Coding Plan（订阅项，模型级窗口）、
│           │              #   自定义校验与预置/自定义模型库字段级合并
│           │              #   （resolve/resolve_with、preset/preset_with_currency）
│           ├── provider/  # 预置平台查询
│           │   ├── claude.rs        # Claude 订阅（Pro/Max）：CLI 凭据复用——只读
│           │   │                    #   ~/.claude/.credentials.json（claudeAiOauth.accessToken，
│           │   │                    #   双拼写兼容）→ GET oauth/usage（anthropic-beta 头）；
│           │   │                    #   已知四窗口固定顺序 + 未知窗口自动兼容、extra_usage 透传
│           │   │                    #   extra；401/403 → 重登引导（fetch_json_relogin）
│           │   ├── codex.rs         # Codex（ChatGPT 订阅）：CLI 凭据复用——只读
│           │   │                    #   ~/.codex/auth.json（auth_mode==chatgpt 门禁，API key
│           │   │                    #   模式确定性引导）→ GET wham/usage（UA codex-cli 防拦 +
│           │   │                    #   ChatGPT-Account-Id 头）；primary/secondary 双窗口按秒数
│           │   │                    #   标注、reset 秒→毫秒
│           │   ├── deepseek.rs      # /user/balance（单站双币，余额 API 返回币种）
│           │   ├── gemini.rs        # Gemini Code Assist：CLI 凭据复用——只读
│           │   │                    #   ~/.gemini/oauth_creds.json，access_token 过期用
│           │   │                    #   refresh_token + gemini-cli 公开 client 凭据刷新
│           │   │                    #   （不写回文件，失败回退旧 token）；两步 RPC
│           │   │                    #   loadCodeAssist → retrieveUserQuota，buckets 按模型分组
│           │   │                    #   （flash-lite 先判）聚合取最小剩余比例
│           │   ├── grok.rs          # Grok 订阅 credits：CLI 凭据复用——只读 ~/.grok/auth.json
│           │   │                    #   （scope map，OIDC 优先/legacy 兜底）；gRPC-web 空帧请求 +
│           │   │                    #   整组伪装头；响应无 .proto——帧拆分 + 通用 protobuf 递归
│           │   │                    #   扫描 + 字段路径启发提取（fixed32 百分比/varint 重置秒/
│           │   │                    #   零用量特判），必须走 HttpResponse.raw 字节保真通道；
│           │   │                    #   gRPC 16/7→重登、4/14→瞬时
│           │   ├── kimi.rs          # /v1/users/me/balance（国内/国际双站，
│           │   │                    #   余额+代金券/现金拆分进 extra）
│           │   ├── kimi_coding.rs   # Kimi Code 国内/国际双站 /coding/v1/usages：
│           │   │                    #   5h+周额度、RFC3339 重置时间、remaining 本地推导
│           │   ├── minimax.rs       # MiniMax Coding Plan 国内/国际双站 coding_plan/remains：
│           │   │                    #   仅 general 条目、5h+周剩余% 归一已用%、
│           │   │                    #   周桶仅 weekly_status==1 展示（==3 无周限防假窗口）
│           │   ├── mod.rs           # NativeProvider trait（query 收套餐变体）+ 注册表（20 项）+
│           │   │                    #   supports_plan_variant/uses_cli_credentials 标记（订阅
│           │   │                    #   四家 CLI 凭据型：查询时读本机官方 CLI 登录文件）+
│           │   │                    #   解析工具（parse_success_json/status_error_with_body 错误
│           │   │                    #   附脱敏响应体 detail——error_detail 嵌套 error.message 后
│           │   │                    #   回退平铺字符串，非字符串形态不误提取）+ MockHttp
│           │   ├── novita.rs        # /v3/user/balance（availableBalance÷10000=USD）
│           │   ├── openrouter.rs    # /api/v1/credits（remaining = credits − usage）
│           │   ├── siliconflow.rs   # /v1/user/info（国内/国际双站参数化，CNY/USD）
│           │   ├── stepfun.rs       # /v1/accounts（顶层 balance，CNY）
│           │   ├── zhipu.rs         # GLM Coding Plan 用量（智谱/Z.ai 双站，非文档端点、裸 key、
│           │   │                    #   已用百分比多窗口：type 过滤 + unit 归类 5h/周 + 未知条目
│           │   │                    #   仅填 5h 空槽宁缺毋错；TIME_LIMIT 独立成 MCP 行（不受变体
│           │   │                    #   过滤）；PlanVariant 声明过滤——NoWeekly 只留 5h、Weekly
│           │   │                    #   放宽；各窗口 nextResetTime 透传 reset_at 供倒计时）
│           │   └── zhipu_metered.rs # 智谱/Z.ai 通用 API 按量余额：Bearer、credit grants 优先，
│           │                        #   404/405 回退 balance；两站 CNY/USD
│           ├── query/     # 查询引擎
│           │   └── mod.rs # QueryEngine：双通道路由（条目 use_proxy 走代理通道，
│           │              #   未配全局端口 → 确定性引导）→解密→分派（native/
│           │              #   template/script；CLI 凭据型 native 跳过解密前置，
│           │              #   api_key_enc 可为 None）→超时（15s）
│           ├── script/    # 脚本查询（M4）
│           │   └── mod.rs # QuickJS 沙箱脚本查询（M4）：{request, extractor} 两阶段协议
│           │              #   （rquickjs 默认内嵌编译 QuickJS-NG）；
│           │              #   六环节——变量替换（代码字符串层面，脚本可分享）→ 沙箱
│           │              #   eval request 产物（JSON 桥出入箱）→ URL 安全校验 →
│           │              #   宿主发请求（fetch_json 收口错误分类与脱敏、apiKey 从根
│           │              #   登记）→ 沙箱 eval extract → 产物转 UsageData（parse_num
│           │              #   语义）；沙箱限制：内存 16MiB/栈 256KiB/单次 eval 5s CPU
│           │              #   中断器（eval 在 spawn_blocking 线程）；validate 干跑
│           │              #   （假变量+形状校验）；JS 异常消息可能回显注入 key——
│           │              #   全错误路径 redact 收口
│           ├── template/  # 声明式模板 DSL（M2a）
│           │   ├── mod.rs  # DSL 结构/静态校验/执行器（变量替换、URL 安全、
│           │   │           #   多窗口、uses_api_key；错误文案不含明文凭据；
│           │   │           #   substitute/check_url_safety 与 script 查询共用）
│           │   └── path.rs # JSONPath 子集（$.a.b[0]，拒绝过滤器/通配符）
│           ├── update.rs  # 更新检测（M4-b）：版本三段比较、GitHub release 解析与资产
│           │              #   选择、节流/每日到点纯函数、AssetDownloader 独立下载通道
│           │              #   （10min 总超时 + 15s 连接超时快速失败、256MB 上限、分块
│           │              #   进度/速率回调且兼容旧实现、可选代理构造——显式代理不叠加
│           │              #   环境变量；proxy_url_of 端口→URL 单一拼接口径）、字节原子落盘
│           └── vault/     # 凭据保险库（主密钥存系统凭据库 + AES-GCM 密文）
│               ├── cipher.rs # AES-256-GCM 与 v1: 密文格式（nonce 随机、AAD 绑定）
│               ├── mod.rs    # open（取/建主密钥）+ encrypt/decrypt
│               └── store.rs  # SecretStore trait + KeyringStore（生产）/ InMemoryStore（测试）
├── docs/                   # 文档
│   ├── CC-Switch调研报告.md  # cc-switch 代码级调研（技术栈/密钥安全/余额查询）
│   ├── design/           # 设计文档
│   │   └── tray-ring-demo.html # 托盘圆环视觉规格（层结构/颜色/溢出/红点定案）
│   ├── specs/            # 规格文档
│   │   ├── CLI-spec.md     # quota-cli 规格（M2b）：子命令/退出码/dev-smoke
│   │   ├── GUI-spec.md     # quota-desktop 规格（M3）：窗口托盘/IPC/快照持久化
│   │   └── history-spec.md # 历史存储规格（M5）：数据模型/滚动清理/schema 迁移/
│   │                       #   迁移容器 v2/CLI 命令（含 SQLite 决策边界）
│   ├── 项目方案预研.md         # 架构、凭据安全、查询体系、CLI/GUI 设计与里程碑
│   └── 预置Provider缺口预研.md # 预置平台缺口对照 cc-switch（A/B/C 分层、批次标记与
│                         #   StepFun/Novita/MiniMax/NewAPI 模板移植参考）
├── examples/               # 可运行示例
│   ├── scripts/   # JS 脚本查询可运行示例（basic 最小闭环/多窗口字段间运算+reset_at，
│   │              #   含 README 协议说明与已知边界，loopback 端到端实测）
│   │   ├── basic.js        # 最小闭环示例：单 request+extractor 拿余额
│   │   ├── multi-window.js # 字段间运算 + reset_at
│   │   └── README.md       # 协议说明与已知边界
│   └── templates/ # 声明式模板可运行示例（5 形态：字符串数字单对象/双站 baseUrl/
│                  #   总额已用/多窗口/NewAPI 系中转；前四者经 quota template test
│                  #   实测，newapi 需自备站点）
│       ├── deepseek.json     # 字符串数字单对象形态示例
│       ├── multi-window.json # 多窗口字段映射形态示例
│       ├── newapi.json       # NewAPI 系中转示例（New-Api-User 头）
│       ├── openrouter.json   # 总额-已用差值形态示例
│       ├── README.md         # 使用说明与已知边界
│       └── siliconflow.json  # 双站 baseUrl 参数化形态示例
├── LICENSE                 # MIT 许可证全文（参考项目 cc-switch 同为 MIT）
├── README.en.md            # 英文版自述，与中文版结构对齐，顶部互链
├── README.md               # 项目自述（中文，默认展示）：定位/功能/预置平台/
│                           #   模板示例/安全设计/构建安装，顶部链英文版
├── rust-toolchain.toml     # 锁定 stable 工具链
└── scripts/                # 维护脚本
    ├── clean.ps1       # 固定白名单分级清理器（仓库边界校验 + WhatIf）
    └── clean.tests.ps1 # 隔离沙箱契约测试：三级目标/预览/受保护文件
<!-- file-tree:full:end -->
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

## 文件树（简版速览）

```
<!-- file-tree:tree:begin 由脚本渲染，禁止手改 -->
QuotaTray/
├── .agents/                # Agent 技能库（项目级）
│   └── skills/ # 技能目录
│       └── file-tree/ # 文件树技能
│           ├── agents/   # Codex 元数据目录
│           │   └── openai.yaml # Codex 技能元数据
│           ├── scripts/  # 技能脚本
│           │   ├── tree_tool.py      # 文件树唯一维护脚本
│           │   └── tree_tool_test.py # 脚本契约测试
│           ├── SKILL.md  # 技能主入口
│           └── tree.json # 文件树唯一数据源
├── .DevApiKey.json.example # 本地密钥文件模板
├── .gitattributes          # 行尾规则（技能 LF）
├── .github/                # GitHub 配置
│   └── workflows/ # CI 工作流
│       └── ci.yml # CI 双矩阵流水线
├── .gitignore              # 忽略清单（密钥/生成物）
├── AGENTS.md               # 项目规则单一事实源
├── apps/                   # 应用层（CLI 与桌面端）
│   ├── quota-cli/     # CLI 前端（bin 名 quota）
│   │   ├── Cargo.toml # CLI crate 清单
│   │   └── src/       # CLI 源码
│   │       ├── cmd/           # 子命令实现（每命令一模块）
│   │       │   ├── add.rs            # 交互添加向导
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
│       ├── package.json        # pnpm 前端清单
│       ├── pnpm-lock.yaml      # 前端依赖锁文件
│       ├── pnpm-workspace.yaml # pnpm 构建许可
│       ├── src/                # React 前端源码
│       │   ├── api.ts                  # invoke 封装
│       │   ├── App.tsx                 # 主窗布局与页签
│       │   ├── assets/                 # 静态资源
│       │   │   ├── brand-mark.png # 透明品牌主图
│       │   │   └── providers/     # Provider SVG 图标集
│       │   ├── components/             # 前端组件
│       │   │   ├── BrandMark.tsx                # 品牌标志薄组件
│       │   │   ├── configTransferView.test.ts   # 迁移视图测试
│       │   │   ├── configTransferView.ts        # 迁移视图纯逻辑
│       │   │   ├── EditDialog.tsx               # 添加/编辑弹窗
│       │   │   ├── HoverPanel.tsx               # 托盘悬停浮窗
│       │   │   ├── hoverPanelView.test.ts       # 悬停面板测试
│       │   │   ├── hoverPanelView.ts            # 悬停面板纯逻辑
│       │   │   ├── MainPanelTabs.tsx            # 页签与鼠标聚光
│       │   │   ├── mainPanelTabsView.test.ts    # 聚光视图测试
│       │   │   ├── mainPanelTabsView.ts         # 聚光视图纯逻辑
│       │   │   ├── nativeProviderGroups.test.ts # 平台分组测试
│       │   │   ├── nativeProviderGroups.ts      # 平台分组纯逻辑
│       │   │   ├── NativeProviderPicker.tsx     # 平台聚合选择器
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
│       │   │   ├── SettingsDialog.tsx           # 设置弹窗
│       │   │   ├── settingsView.test.ts         # 设置视图测试
│       │   │   ├── settingsView.ts              # 设置视图纯逻辑
│       │   │   ├── TemplateHelpCard.tsx         # 模板说明折叠卡
│       │   │   ├── TitleBar.tsx                 # 自定义标题栏
│       │   │   └── ui.tsx                       # 共享基础组件
│       │   ├── display.test.ts         # display 文案测试
│       │   ├── display.ts              # 时间与百分比文案
│       │   ├── i18n/                   # 轻量自写 i18n
│       │   │   ├── en.ts     # 英文字典（编译锁键）
│       │   │   ├── index.tsx # LangProvider 与 t()
│       │   │   └── zh.ts     # 中文字典（类型基准）
│       │   ├── index.css               # 设计令牌与全局样式
│       │   ├── main.tsx                # React 入口
│       │   ├── mainPanelView.test.ts   # 面板切换测试
│       │   ├── mainPanelView.ts        # 面板切换状态机
│       │   ├── queries.test.ts         # queries hooks 测试
│       │   ├── queries.ts              # React Query hooks
│       │   ├── theme.tsx               # ThemeProvider 三态
│       │   ├── themeTransition.test.ts # 扩散动效测试
│       │   ├── themeTransition.ts      # 主题扩散动效
│       │   ├── types.ts                # core serde 的 TS 镜像
│       │   └── vite-env.d.ts           # Vite 资源类型声明
│       ├── src-tauri/          # Tauri Rust 后端
│       │   ├── build.rs        # Tauri 构建脚本
│       │   ├── capabilities/   # 权限 ACL
│       │   │   ├── default.json     # 主窗 ACL
│       │   │   └── hover-panel.json # 悬停窗 ACL
│       │   ├── Cargo.toml      # 桌面端 crate 清单
│       │   ├── examples/       # 示例注入器
│       │   │   └── smoke_setup.rs # GUI 冒烟注入器
│       │   ├── icons/          # 应用图标集
│       │   ├── src/            # 后端源码
│       │   │   ├── commands.rs    # IPC 命令集
│       │   │   ├── hover_panel.rs # 悬停窗口状态机
│       │   │   ├── i18n.rs        # 托盘/命令双语文案
│       │   │   ├── lib.rs         # Tauri Builder 装配
│       │   │   ├── main.rs        # 薄壳入口
│       │   │   ├── ring.rs        # 托盘圆环渲染
│       │   │   ├── settings.rs    # settings.json 读写
│       │   │   ├── snapshot.rs    # cache.json 快照
│       │   │   ├── state.rs       # AppState
│       │   │   ├── tray.rs        # 托盘菜单与图标
│       │   │   └── update_ctl.rs  # 更新检测控制
│       │   └── tauri.conf.json # Tauri 配置
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
│           ├── script/    # 脚本查询（M4）
│           │   └── mod.rs # QuickJS 沙箱脚本查询
│           ├── template/  # 声明式模板 DSL（M2a）
│           │   ├── mod.rs  # DSL 结构与执行器
│           │   └── path.rs # JSONPath 子集
│           ├── update.rs  # 更新检测与下载
│           └── vault/     # 凭据保险库
│               ├── cipher.rs # AES-256-GCM 密文格式
│               ├── mod.rs    # Vault 门面
│               └── store.rs  # 密钥存储抽象
├── docs/                   # 文档
│   ├── CC-Switch调研报告.md  # cc-switch 调研
│   ├── design/           # 设计文档
│   │   └── tray-ring-demo.html # 圆环视觉规格
│   ├── specs/            # 规格文档
│   │   ├── CLI-spec.md     # CLI 规格（M2b）
│   │   ├── GUI-spec.md     # GUI 规格（M3）
│   │   └── history-spec.md # 历史存储规格（M5）
│   ├── 项目方案预研.md         # 项目方案预研
│   └── 预置Provider缺口预研.md # 预置缺口预研
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
├── README.en.md            # 英文自述，互链中文
├── README.md               # 中文项目自述
├── rust-toolchain.toml     # 锁定 stable 工具链
└── scripts/                # 维护脚本
    ├── clean.ps1       # 分级清理器
    └── clean.tests.ps1 # 清理器契约测试
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
