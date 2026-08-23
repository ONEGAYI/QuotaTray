# quota-desktop 规格（M3）

> 状态：待开发（并行窗口已开放，见 [AGENTS.md](../../AGENTS.md) 并行开发约定）
> 依赖：仅使用 quota-core 已冻结的公开 API；本 spec 是桌面端唯一需求来源

## 1. 定位与边界

Tauri 2 桌面应用：主窗口做配置管理，托盘做余额常驻展示。**GUI 是薄层**——
全部业务在 core，前端事件只消费结果；快照持久化在本端实现（core 冻结期不动 core）。

**不做**：CLI 已覆盖的批量管理功能不再重复设计；用量趋势图表等留待后续版本。

## 2. 技术选型

| 项 | 选定 | 理由 |
|---|---|---|
| 框架 | Tauri 2（Rust 主进程即后端） | D1 决策；无额外 sidecar 进程 |
| 前端 | React 18 + TypeScript + Vite | 生态最成熟，表单/列表场景资料多；Svelte 备选但无决定性优势 |
| 样式 | Tailwind CSS | 小规模 UI，避免组件库全家桶 |
| 状态 | React Query（或等价轻量方案） | 查询结果的缓存/失效直接对齐 core 错误双轨 |

`tauri.conf.json` 的 `version` 必须与 workspace 版本一致（发布脚本校验，
版本号统一计算的决策见方案预研 §2 讨论记录）。

## 3. 窗口与托盘行为

### 主窗口（启动显示，关闭收托盘）

三块区域：

1. **供应商列表**：每条显示 名称 / 类型徽标（native id 或 template）/ 余额或已用百分比 /
   相对时间（"3 分钟前"）/ 手动刷新按钮。查询失败的条目显示错误徽标
   （瞬时=灰、确定性=红），多窗口条目分行。
2. **添加/编辑表单**（对话框或侧栏，三种形态）：
   - native：平台下拉（`natives` 列表）+ key 输入（password 型，空=保持不变）
   - template：模板 JSON 编辑器（CodeMirror 或等价）+ base_url + key +
     「校验」按钮（调 validate）与「试查」按钮（调 template test）
   - script（M4 预留 tab，禁用态）
3. **设置**：常规页含自动刷新间隔、低额度提醒阈值、开机自启；更新页管理
   自动检测与下载安装包；数据迁移页通过系统文件选择器导出/导入完整配置，
   写文件或整体替换前必须显示高敏感风险确认。

### 托盘

- **菜单**：按条目分组的余额列表（`名称 · 剩余 62.97 CNY` 或
  `名称 · 已用 42% · 3 分钟前`）→ 分隔线 → 「立即刷新」「打开主窗口」「退出」。
- **峰谷信息行**：当前展示条目（圆环数据源）的余额行后追加两行 disabled 项——
  `⚡ 高峰 · V4 Flash`（判定 + 模型标签）与当前档三价
  `命中 0.1 · 未命中 3 · 输出 9 CNY/Mtok`（缺价字段跳过）；未配置峰谷的
  条目不追加。判定在菜单重建时进行；更新调度每分钟比对峰/谷状态，
  翻转才重建托盘（轮询间隔长时的过期兜底）。
- **悬停刷新**：指针进入托盘触发全量查询，10 秒节流（cc-switch 验证过的节奏）。
- **悬停详情面板**：指针进入托盘同时在图标相邻位置显示无边框浮窗，余额为第一视觉重心；
  展示当前账户、模型、更新时间、额度窗口与峰谷价格。账户下拉切换托盘圆环数据源，
  模型下拉持久更新该条目的计价模型。离开托盘与面板后延迟收起，面板内操作期间保持显示。
- **低额度提醒**：任一条目已用百分比 ≥ 阈值时，该菜单项标红；
  全部条目低于阈值时托盘图标常态，任一超限换警示色图标。
- **单实例**：启动时抢 named mutex（`Global\QuotaTray`），已在运行则激活
  已有窗口并退出——同时根治 vault 首次初始化竞态（审查第 1 轮遗留项）。
- **退出**：仅托盘菜单「退出」真正退出（清托盘图标）；窗口关闭按钮 = 隐藏。

### 查询调度

- React Query 轮询（默认 5 分钟，`refetchIntervalInBackground` 保证托盘态继续刷新）；
  瞬时失败保留上次成功值展示（keep-last-good，10 分钟窗口），确定性失败立即透出。
- 调度策略在前端实现（core 冻结期不加调度 API）。

## 4. IPC 契约（tauri commands）

全部为 core API 薄封装，参数/返回不含明文 key：

| command | 入 → 出 | 对应 core |
|---|---|---|
| `list_providers` | → `ProviderEntry[]`（含密文字段，供编辑回显结构） | `AppConfig::load` |
| `upsert_provider` | `ProviderEntry` → ()（含 `pricing` 字段校验） | `AppConfig::save` + `pricing::validate` |
| `remove_provider` | `id` → () | 同上 |
| `list_native_metas` | → `NativeMeta[]`（含峰谷预置 pricing 字段） | `provider::metas` + `pricing::preset` |
| `validate_template` | `TemplateConfig` → `Ok / 字段定位错误` | `template::validate` |
| `test_template` | `TemplateConfig + key输入 + baseUrl` → `UsageData[] / QueryError` | `template::execute`（经引擎） |
| `query_provider` | `id` → `UsageData[] / { kind, message }` | `QueryEngine::query` |
| `get_settings / save_settings` | 设置对象 ↔ | desktop 自有存储 |
| `export_configuration` | `path` → () | `export_config_to_path` |
| `import_configuration` | `path` → `provider_count` | `import_config_to_path`；清空旧结果/快照并广播刷新 |

**红线 3 落实**：key 写入走「空值 = 保持不变」约定，前端永不回显明文、
永不接收明文（编辑表单的 key 框初始为空，占位符显示"已配置"/"未配置"）。

## 5. 快照持久化（desktop 侧）

core 冻结期内由本端实现：查询成功后把 `{ id: { data, at } }` 写入
`~/.quotatray/cache.json`（tauri-plugin-store 或直接 fs，原子写）；
启动时先渲染快照（标注"上次于 N 分钟前"）再异步刷新——消除重启空窗
（cc-switch 的已知短板，方案预研 §5.3 改良项）。

## 6. 打包与分发

- Windows 优先：NSIS 或 MSI（tauri bundler 默认），目标 `x86_64-pc-windows-msvc`。
- 自动更新：接入 tauri-plugin-updater（端点待定，发布前完成签名与密钥对）。
- CI：push 到 main 跑 fmt/clippy/test（已有）+ 前端 lint/build；release tag 触发打包。

## 7. 验收标准

- [ ] 托盘常驻展示全部 enabled 条目的余额，悬停刷新有节流
- [ ] 悬停详情面板可查看余额/额度/峰谷价格，并快速切换圆环数据源与计价模型
- [ ] 关闭窗口收托盘，托盘「退出」正常退出且图标清理
- [ ] 重启应用托盘先显快照再刷新，无空窗
- [ ] native/template 两种条目的增删改查全程可用，模板校验错误带字段定位展示
- [ ] 峰谷定价：DeepSeek 预置三模型可选（默认 flash），托盘显示当前峰/谷
  与三档价；自定义时段/价格可保存，跨日等非法配置被拒并提示字段定位
- [ ] key 在 UI 任何位置不回显明文；`--json` 类调试输出亦无泄漏
- [ ] 单实例：第二个实例启动只激活已有窗口
- [ ] 低额度提醒按阈值生效
- [ ] 数据迁移可通过系统文件对话框完成；导入后账户、托盘与悬停窗同步刷新，旧快照清空
- [ ] `cargo clippy/test --workspace` 全绿；前端构建无错误
