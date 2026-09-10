# T-03 建立目录同步与持久缓存

状态：已完成（2026-09-10，待所有者验收；提交 bd6c085）  
Blocked by: T-02（已合入同分支）  
规格：[§4—§6、§10 V-04/V-05/V-06/V-07](../spec.md)  
解锁：T-04、T-05、T-07。

## 目标

core 提供统一的装载、更新和状态接口，让任何前端都能安全使用最近有效目录。
本票是新增同步公开 API 的准备票，独立 core PR 合入后再接应用端。

## 范围与责任

负责 core pricing_catalog、必要的 core 小工具复用、缓存集成测试和文件树。
网络注入 HttpClient；价格计算不执行 IO。不得把目录更新塞进安装包状态机。

1. 装载缓存与嵌入种子，选较新且兼容的完整目录。
2. 固定 HTTPS 源按真直连 → 已配置代理的顺序获取并校验。
3. 实现版本单调、同版本异内容拒绝、已知模型物理遗漏拒绝。
4. 完整目录与同步状态写入同一缓存信封；替换成功后才返回 applied 快照。
5. 跨进程写锁内重读 revision，避免 CLI 和 GUI 并发覆盖新数据；不在网络期间持锁。
6. 对外返回 updated / unchanged / busy / failed，并区分当前可用目录与最近同步错误。
7. 缓存独立于 config.json 与迁移包，不存用户条目或凭据。

## 验收

- [x] 初装离线、缓存损坏、较旧缓存、较新内置种子均得到确定的有效目录。
- [x] 404/HTML/非法 UTF-8/非法 schema/负价/同版本异内容不会覆盖缓存。
- [x] 两请求先发后到及跨进程并发写入时，revision 不倒退且不出现半包。
- [x] 写入失败仍使用原内存快照；锁忙可返回 busy；无需强制删除锁文件。
- [x] 直连成功不发代理请求；有代理且直连失败才回退；双失败仍可读旧数据。
- [x] 重复相同包不会发目录变更事件所需的“有变化”信号。
- [x] 更高 revision 携带旧价格可实现人工回滚。
- [x] 测试只使用临时数据根和 mock HTTP，不读取生产凭据或真实账户。
- [x] 同步状态接口、缓存路径规则和失败语义被记录，供应用端消费。

## 实施记录（2026-09-10）

- 提交 `bd6c085`。门禁全绿：fmt / clippy --workspace --all-targets
  -D warnings / cargo test --workspace（core 415，含 sync 17 例
  临时目录 + mock HTTP 集成测试）。
- 并发策略：进程内 AtomicBool 在途标志（RAII 复位，await 安全）+
  数据根级 create_new 锁文件（Guard Drop 释放；锁忙返回 Busy 且
  不删他人锁文件；网络期不持锁）。锁内重读磁盘信封，磁盘 revision
  高于内存时先升级基准再比较，防止 CLI/GUI 互相覆盖新版本。
- 接口（lib.rs 已导出，T-04/T-05 消费）：
  `CatalogSync::new(data_root, direct, proxied, now_ms)` / `effective()`
  / `status() -> CatalogStatusView{revision,origin,fallback_reason,
  last_attempt_ms,last_success_ms,last_error}` / `update() ->
  Updated{catalog}|Unchanged{revision}|Busy|Failed(CatalogSyncError)`
  / `reload_from_disk_if_newer() -> bool`；
  纯函数 decide_between / evaluate_incoming / load_effective /
  effective_from_envelope_json；常量 CATALOG_URL / CATALOG_CACHE_FILE
  （数据根下 pricing-catalog.json）/ CATALOG_LOCK_FILE / CATALOG_MAX_BYTES。
- 失败语义：网络(Network)/响应不可用(BadResponse)/候选包拒绝
  (Rejected)/缓存写入失败(Io) 四类；写入失败沿用内存快照；拒绝类
  坏包参与代理兜底（直连劫持防护）；元数据信封允许落盘但目录数据
  从不被坏包替换（测试断言磁盘目录保持种子等价）。
- 回退原因四态：NoCache / CorruptedCache / IncompatibleCache /
  StaleCache（缓存不高于种子——修订权在应用发布侧）。
- Windows 实测：缓存路径被目录占用时 tmp 写入成功、rename 失败
  → Io 分类（update_write_failure_keeps_memory_snapshot）；
  write_atomic_bytes 对已存在目标的替换为既有已验证路径。

## 验证

公开同步接口加临时目录集成测试，覆盖 V-04 至 V-07。
Windows 上实测“目标缓存已存在”时的替换行为；不要仅凭函数名承诺原子性。
现有 HttpResponse 无响应头，首期不扩展冻结 trait 来强加 ETag。
MSRV、fmt、workspace all-targets clippy 与相关测试通过。

## 接手提示

代码库可能有其他人的改动；不回退他人修改。
core 公共 API 先独立 PR，不在此票引入 UI 或 CLI 自动任务。
完成后记录并发策略、接口、提交/PR 和验证证据。
