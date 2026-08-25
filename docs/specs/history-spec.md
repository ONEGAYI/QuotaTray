# 历史数据存储规格（M5）

> 状态：M5-a 实现中（core 存储层 + 迁移容器 v2 + CLI 命令 + 两端写入接线）；
> M5-b（GUI 走势图 IPC 与弹层 UI）留待后续版本
> 依赖：`quota-core::history` 新增模块；迁移容器 v2 向后兼容 v1

## 1. 定位与边界

记录每次成功查询的余额/额度快照，滚动保留 30 天，为走势图（M5-b）与
CLI 查看（M5-a）提供数据底座。存储使用 SQLite（rusqlite，bundled 内嵌编译）。

**非目标**：请求级用量日志、多维聚合统计、仪表盘——这些属于预研文档 §1
排除的「用量日志统计仪表盘」领地。本功能只存查询结果快照序列。

**对既有 SQLite 决策的说明**：预研文档 §3.3 曾以「数据量不值当」否决 SQLite，
其语境是**配置存储**（个位数供应商 × 少量字段）。历史时序数据
（30 天 × 多条目 × 多窗口 × 默认 5 分钟一拍）量级与访问模式不同，
时序追加 + 时间范围查询正是 SQLite 的舒适区，该决策不适用于本场景。

**里程碑**：M5-a 随 0.5.0 发布；M5-b（GUI 走势图）另立。

## 2. 数据模型

一次查询成功返回 `Vec<UsageData>`（多窗口多条），每条落一行：

| 列 | 类型 | 说明 |
|---|---|---|
| `provider_id` | TEXT | 条目 id |
| `window_key` | TEXT | 窗口键：`plan_name` 非空（去空白）取之，否则回退序数 `w0/w1…`（`history::window_key`） |
| `sampled_at` | INTEGER | 采样时刻，epoch 毫秒 |
| `used` / `remaining` / `total` | REAL | 可空数值（`Option<f64>` 原样保留空值） |
| `unit` | TEXT | `USD`/`CNY`/`%` 等 |

- 主键 `(provider_id, window_key, sampled_at)`，`WITHOUT ROWID`；
  同毫秒重放 `INSERT OR REPLACE` 幂等。
- 不存 raw JSON、不存 `reset_at`——走势只需数值；将来需要细节走 schema 迁移加列。
- 体积估算：默认 5 分钟轮询 × 30 天 ≈ 8640 点/窗口；典型 5 条目双窗 < 3MB。
- **`is_valid == Some(false)` 的行跳过落库**：凭据失效期间数值不可信（常为 0
  而非空），落库会在走势上留下无法区分的假断崖；失效期间时间线中断，
  由读取方按空档呈现。`is_valid` 未声明（None）视为有效照常记录。
- 窗口键重复语义：模板条目由 `template::validate` 拒绝重名窗口；script
  条目返回重复 `plan_name` 时后者覆盖前者（同毫秒同键天然幂等）。

## 3. 存储与运维

- 文件：`~/.quotatray/history.db`；桌面端跟随 `--data-dir` 覆盖，
  CLI 跟随 `--config` 同目录推导。
- PRAGMA：`journal_mode=WAL`、`synchronous=NORMAL`（伴生 `-wal/-shm` 文件）。
- 滚动清理：保留 `DEFAULT_RETENTION_DAYS = 30` 天；写入路径节流触发
  （间隔 ≥ 1h 才执行一次 `DELETE WHERE sampled_at < cutoff`，走
  `idx_history_sampled_at` 索引）。保留天数暂为常量，设置项留 M5-b。
- 历史库是**非关键附属数据**：两端写失败一律静默告警（stderr），
  不影响查询退出码、keep-last-good 与托盘行为；库文件损坏时桌面端降级
  内存库运行，CLI 跳过写入。

## 4. Schema 迁移机制

- `PRAGMA user_version` 记录库版本；`MIGRATIONS: &[&str]` 脚本数组，
  下标 i 的脚本把库从版本 i 升到 i+1，**只允许追加**。
- 打开库时逐版本应用，每个迁移在单事务内执行 DDL 并写入新版本号；
  失败则回滚且版本号不推进。
- 库版本比二进制新（降级运行旧应用）时拒绝打开（`HistoryError::NewerVersion`）。

## 5. 跨机器迁移（容器 v2）

`.qtray-export` 容器 `FORMAT_VERSION` 1 → 2：

- v1（历史版本）：明文载荷为 `AppConfig`，仍可导入（凭据转写逻辑不变）。
- v2（当前导出版本）：明文载荷为 `{ "config": AppConfig, "history": [行] | null }`
  信封；信封 AAD 改用 `quotatray-config-export:v2`，防跨版本密文重放。
- 历史行仅含数值列（§2 的列 + `provider_id`），不进凭据转写（无敏感字段），
  但随容器整体加密，不泄漏明文。
- 旧二进制读 v2 容器报「不支持的版本」——单向升级，先例同 script
  ProviderKind。
- 导入合并：`HistoryStore::merge_rows` 按主键 `INSERT OR REPLACE`，幂等；
  两台机器各自积累的时间线自然拼接（同条目 id 在导入配置时保留原 id）。
- 16 MiB 容器上限对历史同样生效；超限导出报错（`TooLarge`），
  用户可先 `quota history clear`。
- 条目删除时同步 `clear(id)`（与快照孤儿过滤语义对齐）；
  配置导入**不**清本机历史（旧条目数据自然滚动淘汰）。

## 6. API（quota-core::history）

```rust
pub fn window_key(data: &UsageData, ordinal: usize) -> String;

impl HistoryStore {
    pub fn open(path: &Path) -> Result<Self, HistoryError>;      // 建目录+WAL+迁移
    pub fn open_in_memory() -> Result<Self, HistoryError>;
    pub fn record(&self, provider_id: &str, data: &[UsageData], at_ms: u64) -> Result<(), HistoryError>;
    pub fn range(&self, provider_id: &str, from_ms: u64) -> Result<Vec<HistoryPoint>, HistoryError>;
    pub fn clear(&self, provider_id: Option<&str>) -> Result<(), HistoryError>;
    pub fn export_rows(&self) -> Result<Vec<HistoryExportRow>, HistoryError>;
    pub fn merge_rows(&self, rows: &[HistoryExportRow]) -> Result<(), HistoryError>;
}
```

`HistoryPoint`（`window_key/sampled_at/used/remaining/total/unit`）的 serde
形状预留为 M5-b 的 IPC DTO。

## 7. CLI 命令（M5-a）

```text
quota history show <id> [--window KEY] [--range 24h|7d|30d] [--page-size N] [--page N] [--json]
quota history clear [id] [--yes]
```

- `quota query`（含 `--watch` 每轮）查询成功后写历史；写失败仅 stderr 告警，
  不影响查询输出与退出码。
- `show` 三档范围（默认 7d）：24h=15 分钟桶 / 7d=1 小时桶 / 30d=6 小时桶，
  桶内取最后一点，按窗口时间线分组展示（列：时间/窗口/已用/剩余/单位）；
  `--window` 只看指定时间线（如 five_hour / weekly）。
- 分页：默认每页 20 行（`--page-size` 1..=500 覆盖）；终端下默认交互翻页
  （空格/→ 下一页、b/← 上一页、q/Esc 退出）；`--page N` 指定页码即非交互
  打印该页（与 `--json` 互斥）；管道（非终端）输出整表，翻页交由调用方。
- `--json` 输出原始点（`{id, name, range, points}`，不分页不聚合）。
- 退出码：id 不存在、历史库打开失败、页码超界 → 1；范围内无历史 → 提示并 0。
- `clear`：无 id 清全部、有 id 清单条目（id 须存在）；确认默认否，`--yes` 跳过。

## 8. 桌面端接线（M5-a）

- `DataPaths::history()` 派生路径；`AppState` 持 `Mutex<HistoryStore>`，
  打开失败降级 `open_in_memory` 并告警，不阻断启动。
- `refetch_and_store` 成功分支写历史；`remove_provider` 清条目历史；
  `export_configuration` / `import_configuration` 携带历史。
- 不新增 IPC 与 UI（M5-b 随走势图一起提供 `get_history`）。
