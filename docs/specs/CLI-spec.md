# quota-cli 规格（M2b）

> 状态：已实现（feat/cli-m2b 分支，43af755 起；经两轮子代理审查修复）
> 依赖：仅使用 quota-core 已冻结的公开 API；本 spec 是 CLI 端的唯一需求来源

## 1. 定位与边界

`quota` 是 QuotaTray 的命令行前端，与 GUI 完全平级。业务逻辑（查询、加解密、模板解析）全部在 core，CLI 只做参数解析、结果呈现与配置管理入口。

**不做**：托盘、常驻进程、图表。纯命令进出。

## 2. 命令总览

| 命令 | 用途 |
|---|---|
| `quota list` | 列出全部供应商条目及状态 |
| `quota query [<id>...]` | 查询全部或指定条目 |
| `quota add` | 添加供应商（交互式或 `--json` 传入） |
| `quota edit <id>` | 编辑条目（名称/base_url/模板/启用） |
| `quota remove <id>` | 删除条目 |
| `quota set-key <id>` | 写入/更新 API key（隐藏输入） |
| `quota natives` | 列出预置平台（来自 core 注册表，标注峰谷预置有无） |
| `quota pricing show/set/clear` | 峰谷定价查看 / 自定义 / 清除 |
| `quota template test` | 对模板执行静态校验 + 试查询 |
| `quota vault status` | 主密钥健康检查（系统凭据库可读性） |
| `quota history show/clear` | 查询历史走势查看（三档范围 + 分页）/ 清除 |
| `quota config export/import` | 完整配置跨机器导出 / 整体导入（含查询历史） |
| `quota dev-smoke` | 真机冒烟（仅 debug 构建） |

## 3. 子命令规格

### quota list

- 输出表格：`id / 名称 / 类型（native id 或 template）/ 启用 / 凭据已配?`
- M2b 阶段无历史缓存，不展示余额；`--json` 输出 `AppConfig` 的 providers 数组。

### quota query

```
quota query [<id>...] [--json] [--watch] [--interval <分钟>]
```

- 默认查询全部 `enabled` 条目，按配置文件顺序并行发起（`tokio::join_all`）。
- 表格输出：`名称 / 套餐 / 已用 / 剩余 / 单位 / 状态（OK、失效原因、错误）`。
  多窗口条目（如 5h + 周）输出多行。
- `--watch`：轮询模式，间隔默认取条目配置（M2b 固定 5 分钟，`--interval` 覆盖），
  每轮重绘表格；Ctrl+C 退出。
- `--json`：输出
  `[{ "id", "name", "ok": bool, "data": UsageData[] | null, "error": { "kind": "transient|deterministic", "message", "detail"? } | null }]`，
  供脚本消费。`error.detail` 为可选排查详情（serde 解析位置 + 已脱敏的
  响应体片段，后端保证不含明文凭据），仅存在时输出（additive）。

### quota add / edit / remove / set-key

- `quota add`：向导式依次询问——名称、类型（`natives` 列表选择 / `template` /
  `script`）、平台或粘贴多行内容（模板 JSON：空行结束；脚本 JS 代码：
  单独一行 `.` 结束，空行照常保留）、base_url（模板/脚本条目）、
  脚本条目追加 allowInsecure 确认（默认否）、API key（隐藏输入，直接回车跳过）。
- 高级用法：`quota add --json < entry.json`（entry.json 为 ProviderEntry 的
  JSON，api_key_enc/base_url 由后续 set-key/edit 维护，密文不经手）。
- `quota set-key <id>`：隐藏输入读取 key（终端回显关闭），经 vault 加密后写配置。
  不接受命令行参数形式的 key（避免进入 shell history）；管道 stdin 允许
  （`echo $KEY | quota set-key id` 场景）。
- `quota remove`：确认提示（`--yes` 跳过）。
- id 冲突：`add` 生成短随机 id（如 6 位 base32），`edit/remove` 精确匹配。

### quota history show / clear（M5）

```text
quota history show <id> [--window KEY] [--range 24h|7d|30d] [--page-size N] [--page N] [--json]
quota history clear [id] [--yes]
```

- `quota query`（含 `--watch` 每轮）查询成功后自动写入历史库
  （`config.json` 同目录 `history.db`，30 天滚动保留，见 history-spec）；
  历史打开/写入失败仅 stderr 告警，不影响查询输出与退出码。
- `show` 三档范围（默认 7d）：24h=15 分钟桶 / 7d=1 小时桶 / 30d=6 小时桶，
  桶内取最后一点，按窗口时间线分组展示（列：时间/窗口/已用/剩余/单位）；
  `--window` 只看指定时间线（如 five_hour / weekly）。
- 分页：默认每页 20 行（`--page-size` 1..=500 覆盖）；终端下默认交互翻页
  （空格/回车/→ 下一页、b/← 上一页、q/Esc 退出）；`--page N` 指定页码即非交互
  打印该页（与 `--json` 互斥）；管道（非终端）输出整表，翻页交由调用方。
- `--json`：输出原始点数组（`{id, name, range, points}`，不分页不聚合）。
- 退出码：id 不存在、历史库打开失败、页码超界 → 1；范围内无历史 → 提示并 0。
- `clear`：无 id 清全部、有 id 清单条目（id 须存在）；确认默认否，`--yes` 跳过。
- `config export` 默认携带全量历史行；`config import` 后按主键幂等合并进本机
  历史库（同条目 id 跨机器续线），v1 老包无历史 section、导入不受影响。

### quota config export / import

```text
quota config export <路径> [--yes]
quota config import <路径> [--yes]
```

- `export` 导出完整 `AppConfig`（供应商、凭据、定价、自定义模型库）到
  `.qtray-export` 私有认证容器；默认提示该文件等同明文凭据，`--yes` 表示调用方
  已确认风险。
- `import` 整体替换当前配置；包内凭据先用一次性迁移密钥解密，再以目标机器
  主密钥重加密。任一条目或格式校验失败时不覆盖原配置。
- 两个命令成功返回 0，文件/格式/Vault 错误返回 1；取消交互返回 0 且不写文件。

### quota pricing

```
quota pricing show <id> [--json]
quota pricing set <id>      # stdin 读 PricingConfig JSON
quota pricing clear <id>    # 清除自定义，回退预置
quota pricing model list <provider> [--json]   # 预置 + 自定义模型价格对照
quota pricing model add <provider>             # stdin 读 CustomModelDef JSON（同 id 覆盖）
quota pricing model remove <provider> <id>     # 删除自定义模型
```

- `show`：条目生效峰谷定价——当前峰/谷判定、来源（预置 native·模型 /
  自定义）、三档价格对照表（高峰/空闲两列，单位每 MTokens）、
  时段人类可读聚合（连续星期合并，如「周一至周五 09:00–12:00」）与
  UTC 偏移、下次翻转时刻；`--json` 输出结构化形状（kind/plan/source/
  preset/windows/peak/off_peak/next_change）。
- `show` 的两条生效链：自定义模型库（`config.json` 的 `custom_models`，
  条目 `model` 撞名时自定义优先）；条目 `pricing.currency` 作为币种
  hint——DeepSeek 单站双币时数字与标签一起切 USD 预置套。
- 订阅项（如智谱 Coding Plan）：三档价格为空（表格显示 —）、附加
  「订阅积分制」说明行，JSON 的 `plan` 字段为 `subscription`。
- 条目无定价（无预置且未自定义）→ 提示语 + 退出码 0（查看类非错误）。
- `set`：stdin JSON 经 core `pricing::validate`（字段定位错误）后写入
  `entry.pricing`；字段级回退——只写 `{"model":"pro"}` 即可切换预置模型档。
- `clear`：置空 `entry.pricing`（预置重新生效）。
- `model list`：未知平台 id → 1；预置在前自定义在后，表格列
  模型/id/来源/模式/峰价/闲价（命中/输入/输出紧凑串），`--json` 输出
  `{provider, currency, default_model, models[]}`。
- `model add`：stdin `CustomModelDef` JSON 经 core `validate_custom_model`
  （id/display 非空 + 窗口/时区/价格语义复用 validate）；同 id
  大小写不敏感覆盖（= 更新）。
- `model remove`：大小写不敏感；删空后配置文件移除平台键；不存在 → 1。

### quota template test

```
quota template test [--base-url <url>] [--entry <id> | --json < template.json]
```

- 流程：core `template::validate` 静态校验 → 通过后真实试查一次 → 打印
  静态错误（带字段定位）或 UsageData 结果。
- `--entry` 复用已存条目的 key（vault 解密）；`--json` 模式配合
  `set-key` 前的调试，key 从 stdin 读取。

### quota script test

```
quota script test [--base-url <url>] [--entry <id> | --json < script.js]
```

- 流程与 `template test` 同构：core `script::validate` 干跑校验
  （假变量替换 + `request()` 产物形状，不发 HTTP）→ 真实试查一次。
- `--json` 双形态宽容解析：stdin 先按脚本配置 JSON（`{code, allowInsecure?}`），
  失败则整段视为纯 JS 代码（examples/scripts/ 的 .js 文件可直接重定向）；
  输入以 `{` 开头却解析失败时提示疑似配置误输入（回退仍生效）。
- 代码引用 `{{apiKey}}` 时经 tty 交互收 key（仅本次不落盘）；stdin 被
  重定向占用且 key 为空 → 引导改用 `--entry`（终端场景则提示重输）。
- `quota add` 向导含「script」第三选项（粘贴代码 + 干跑校验重试 +
  allowInsecure 确认，默认否）；`quota edit` 对 script 条目支持 baseUrl、
  代码重粘贴（无效保持原码）与 allowInsecure 修改。

### quota dev-smoke（仅 debug 构建）

```
quota dev-smoke [--key-file <path>]
```

- `#[cfg(debug_assertions)]` 包住整个子命令定义，release 构建不存在。
- 默认读仓库根 `.DevApiKey.json`（格式见 `.DevApiKey.json.example`），
  空值跳过、未知平台告警；逐平台走 core 完整链路（加密→解密→真实 HTTP→解析）。
- 已有参考实现：`crates/quota-core/examples/dev_smoke.rs`（M2b 将其逻辑
  迁入子命令，example 可保留或删除，二选一不留重复）。

## 4. 退出码约定

| 码 | 含义 |
|---|---|
| 0 | 全部成功 |
| 1 | 存在至少一个确定性失败（认证/解析/配置——需人工介入） |
| 2 | 仅存在瞬时失败（网络/限流/超时——可重试） |

确定性失败优先于瞬时失败（同时存在时报 1）。

## 5. 安全约定（红线映射）

- key 输入一律隐藏回显或 stdin，不提供明文参数。
- 错误信息、`--json` 输出不得包含明文 key（core 已保证 Debug/错误脱敏，CLI 层不新造泄漏面）。
- dev-smoke 与 key 文件只在 debug 构建与本地存在，CI 不执行。

## 6. 工程约束

- clap 4 derive；全部业务经 quota-core；CLI 自身不引入 HTTP/加密依赖。
- TDD：配置读写、退出码、输出格式的判定逻辑用单元测试锁（命令解析用
  `assert_cmd` 或 clap try_parse 测试）；真网交互只存在于 dev-smoke。
- 遵循 [AGENTS.md](../../AGENTS.md)：提交规范、文件树同步、并行开发约定。

## 7. 验收标准

- [ ] 全部子命令按本规格工作（人工核验清单逐项过）
- [ ] `quota query` 对 3 个 native 平台 + 1 个 template 条目输出正确
- [ ] 退出码三分约定有测试锁定
- [ ] `quota dev-smoke` 在 debug 构建可跑、release 构建不出现（有编译验证或文档说明）
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo test --workspace` 全绿
- [ ] key 全程不出现在终端回显、日志与 `--json` 输出中
