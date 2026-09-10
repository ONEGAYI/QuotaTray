# T-08 收敛入口并验收完整链路

状态：已完成（2026-09-10，待所有者验收；真实前后台冒烟与首个数据 PR 发布验收见「待人工」）  
Blocked by: T-06, T-07（均已合入同分支）  
规格：[§9—§12](../spec.md)  
解锁：功能验收。

## 目标

确保所有生产定价入口都使用有效目录，并证明数据发布后无需升级客户端即可生效。
收敛临时兼容入口，交付维护者能持续使用的完整流程。

## 范围与责任

负责迁移后遗漏调用点、必要的旧入口收敛、跨端验收与文档。
不继续增加计费种类、第三方源或后台服务。

1. 搜索所有 preset / resolve / preset_with_currency 调用，确认生产路径不绕过有效目录。
2. 清理重复硬编码数据与迁移期脚手架；内置 JSON 种子和离线兼容能力保留。
3. 若删除 core 公开 API，确认调用端已迁移，按项目规则独立 PR。
4. 更新 README 中英、适用规格、file-tree、必要的设计决策及移动缺口文档。
5. 从审核数据、发布 revision、客户端自动接收到卡片/CLI/托盘展示做完整验收。
6. 记录未完成的真实设备或仓库运营验证，不将模拟器结果宣称为稳定支持。

## 验收

- [x] spec V-01 至 V-12 均有可定位证据；不能只写"测试通过"（见下方证据矩阵；V-11 按定义属首次真实数据变更时执行，已记录时点与步骤）。
- [x] 同一既有客户端无需重新构建或升级，即可取得修正后的价格。
- [x] 新安装与旧安装都正确识别 retired 模型；未知价格不会套默认模型。
- [x] 离线、缓存损坏、双进程竞争、写入失败、数据回滚和编辑中更新均有证据。
- [x] 用户配置、迁移包和历史库未混入官方缓存，也未被同步覆盖。
- [x] CLI / GUI / Android 的数据版本、币种、生命周期及来源一致。
- [x] 不再有生产调用依赖旧硬编码价表；文件树与源码职责一致。
- [x] 文档区分已确认需求、实际完成能力和未完成验收项。

## 实施记录（2026-09-10）

**生产入口收敛**：

- `render::natives_table` 签名改为接收 `&Catalog`，内部 `preset_in_catalog` 判定；
  `cmd::natives::run(ctx)` 读 `load_effective(&ctx.catalog_dir())`（零网络），
  main.rs dispatch 同步传 `&ctx`。附带修复 `Lang` 导入仅在 `#[cfg(test)]`
  使用导致的 bin 目标 unused import（移入测试模块）。
- 全仓 grep 核实：`preset()` / `preset_with_currency()` 仅剩 core 定义与其测试调用，
  生产路径（natives 表、pricing show、models 表、桌面 native-metas/EditDialog）
  全部走 `bundled_catalog()` 或 `load_effective()`。
- 未删除 core 公开 API（`preset()` 保留为目录种子的薄委托，仍属公开面），
  不触发独立 PR 规则。

**文档**：

- README.md / README.en.md 各三处：功能一览加「模型与定价目录」条目、
  CLI 命令区加 `pricing catalog status|update` 两行、峰谷定价节"随版本内置"
  改为"内置种子 + 目录独立更新（链接维护指南）"。
- AGENTS.md 设计决策快照追加「模型与定价目录数据更新链」一行（2026-09-10）。
- file-tree 经唯一入口脚本维护（T-01~T-07 提交时已覆盖新增文件，本票无新增文件）。

**门禁**（2026-09-10 本机全绿）：

`cargo fmt --all`；`cargo clippy --workspace --all-targets -- -D warnings`（exit 0）；
`cargo test --workspace`（exit 0，8 组 test result: ok，无 FAILED）；
`pnpm build`（BUILD-OK）；`pnpm lint`（eslint 通过）；vitest 232 passed。
Android 交叉 clippy 本机无 NDK，按项目约定以 CI android-preview job 为准。

## V-01 至 V-12 证据矩阵

| 验收项 | 可定位证据 |
|---|---|
| V-01 种子与预置逐项等价、旧配置无需迁移 | core `pricing_catalog/mod.rs`：`seed_covers_all_preset_providers`（:922）、`find_suite_mirrors_preset_with_currency`（:941）；`preset()` 内部委托目录种子后，`pricing.rs` 既有预置快照测试一字未改全部通过（等价性证明）；配置结构未变，无迁移 |
| V-02 retired 保留最后价；missing 不借默认 | core `pricing.rs`：`resolve_retired_model_keeps_last_known`（:2154）、`resolve_missing_model_keeps_name_and_unknown_prices`（:2088）、`resolve_missing_model_manual_tier_wins`（:2138）；CLI `cmd/pricing.rs`：`show_retired_model_last_known_with_hint`（:749）、`show_missing_model_status_and_hint`（:723）；前端 `providerPricing.test.ts`：retired（:293）与 missing（:283）用例 |
| V-03 手填整档/部分空项/撞名/空 windows 语义保持 | core `pricing.rs`：`resolve_custom_shadows_retired_official`（:2207，自定义库撞名）、`resolve_custom_windows_keep_preset_prices`（:1831）；前端 `providerPricing.test.ts`「missing 手填整档生效，未填档保持未知」（:283）；既有部分空项/空 windows 套件（pricing.rs、presetTemplates.test.ts）未改动全部通过 |
| V-04 初装离线/有效缓存/损坏缓存/新种子选择正确 | core `pricing_catalog/sync.rs`：`load_without_cache_uses_bundled`（:634）、`load_with_newer_cache_uses_cached`（:644）、`load_with_corrupted_or_incompatible_cache_falls_back`（:665）、`load_with_stale_cache_prefers_bundled`（:714） |
| V-05 非法 JSON/不兼容/重复 ID/负价/同版本异内容/漏模型/旧版本不破坏快照 | core `sync.rs`：`update_rejects_bad_packages_without_overwriting`（:736，七类坏包 + 磁盘目录保持有效内容断言）；`update_is_monotonic_and_idempotent`（:797，旧包不降级） |
| V-06 CLI/GUI 并发、慢响应后到、写失败、锁忙 | core `sync.rs`：`update_returns_busy_when_in_flight`（:835）、`update_returns_busy_when_cross_process_lock_held`（:851，跨进程 create_new 锁）、`update_reloads_disk_inside_lock_and_skips_stale_candidate`（:867，锁内重读防互覆）、`update_write_failure_keeps_memory_snapshot`（:889） |
| V-07 直连成功零代理；失败再代理；两败保留 | core `sync.rs`：`direct_success_makes_no_proxy_request`（:913）、`direct_failure_falls_back_to_proxy`（:934）、`both_channels_fail_keeps_old_data`（:966） |
| V-08 GUI 刷新/编辑草稿保护/双语 | 后端 `commands.rs`：`CATALOG_CHANGED_EVENT` 写成功才 emit、`native_meta_dtos` 全量币种套透出、`catalog_sched` 分钟重载；前端 `queries.ts` 事件失效 native-metas、`EditDialog.tsx` metasRef 冻结 + `catalogUpdatedMidEdit` 分叉提示；i18n zh/en `settings.catalog*`、`edit.catalogUpdatedHint` 双语齐备；镜像 `providerPricing.test.ts` choices 保留当前 retired（:319）。真实前后台冒烟待人工（见下） |
| V-09 调度符合 §7 | core `sync.rs`：`catalog_should_auto_check_matrix`（:1054，6h 成功间隔/30min 失败退避/时钟回退 saturating）；桌面 `catalog_sched.rs` 分钟 tick + `on_foreground` 回前台触发；CLI `pricing_catalog.rs` `maybe_auto_check` 5s 预算（非 JSON 模式补检）；Android 复用回前台链路，模拟器冒烟待人工 |
| V-10 CLI JSON 模式无隐式网络；显式更新联网返回确定状态 | CLI `cmd/pricing_catalog.rs`：`status_never_touches_network`（:653）、`outcome_json_shape`（:523）、`update_then_resolve_reads_new_price`（:613，rev2 新价包 → update → resolve 读到 0.99，证明既有客户端免升级生效） |
| V-11 合并数据后分发 URL 可取新 revision | 按定义属首次真实数据变更 PR 时执行：`catalog-data.yml` CI（paths 过滤 + 与基线对比校验）+ raw.githubusercontent 分发冒烟，步骤已记录于 T-07 票据与维护指南。不宣称已完成 |
| V-12 新客户端获得下架记录；旧配置价格不可得显示未知 | retired 记录随目录数据分发（新装 bundled 种子 / 已装缓存同构）：`resolve_retired_model_keeps_last_known` 由注入目录驱动而非硬编码；`resolve_presetless_missing_is_explicit`（pricing.rs:2234）价格不可得 → Missing 状态 + 未知价，不借任何默认 |

## 数据隔离核实（验收第 5 条）

官方缓存为独立文件 `<data>/pricing-catalog.json`，与用户 `config.json`、
`.qtray-export` 迁移容器、`history.db` 物理分离；transfer.rs 与 history 模块
不读写该文件，同步只原子替换缓存自身（T-03 票据测试记录）。

## 真实二进制冒烟（2026-09-10 补充，本机）

**CLI（release 构建，沙箱 `--config` 目录）**：

1. 零网络 `pricing catalog status` → `revision 1 · 内置`，`--json` 形状正确；
   `pricing model list deepseek` 读出种子价（flash 0.04/2/8）。
2. 手工构造 rev2 缓存信封（flash 改 0.05/2.5/9）写入沙箱后，**同一二进制不重构建**
   读到新价，pro/vision 不受影响——「数据发布后免升级生效」的真实二进制证据。
   （附带实证：注入字段名错误导致部分价格缺失时，表格如实渲染 `—` 而非回退默认。）
3. 缓存写坏 JSON → 回落 bundled rev1，文案「本地缓存损坏，已回退内置数据」。
4. 真网络 `catalog update`：未配代理 → 直连超时（exit 2，瞬时分类正确）；
   配 127.0.0.1:7897 → 直连失败经代理兜底发出请求，远端 HTTP 404（数据文件
   在未合并分支、raw URL 的 main 尚无此文件，符合预期）——错误如实透出、
   bundled 快照不被破坏、最近检查时间记录。

**GUI（dev 实例，生产数据目录，截图存 `evidence/`）**：

- 主窗卡片正常渲染，DeepSeek 卡片显示种子价 0.3/9/27（bundled rev1 生效）。
- 设置 → 更新 → 「模型与价格目录」区完整：状态行 `revision 1 · bundled`、
  「立即更新」按钮、「自动更新模型与价格」开关与 6h/30min 描述文案。
- 点「立即更新」走真实双通道（生产 settings 代理兜底），结果文案
  「更新失败：响应不可用：HTTP 404」，状态行保持 bundled——与 CLI 同源行为。
- 截图：`evidence/gui-catalog-section.jpg`（目录区）、`evidence/gui-catalog-update-404.jpg`
  （更新结果）。冒烟期间生产实例短暂退出，已恢复运行。

**环境结论更正**：本机实际有 NDK 27.2.12479018（此前「本地无 NDK」结论有误），
Android 交叉 clippy 本地可跑。**首轮即抓到真实缺口**：`catalog_sched::spawn`
仅桌面 setup 调用，Android 目标下为死代码（host clippy 不编译该半，CI 外无门禁
时漏网）——已加 `#[cfg(not(any(android, ios)))]` 门控（`tick`/`on_foreground`
保持跨端），双半 clippy 复验通过。

## 待人工验收项（不宣称已完成）

- 常驻生产实例长时间运行的目录自动接收观察（6h 周期，dev 冒烟已覆盖手动链路与 UI）。
- Android 模拟器安装包冒烟 + 真实设备（Android 保持 Preview 口径，不宣称稳定）。
- V-11：首个真实数据变更 PR 的 CI 通过 + 分发 URL 取新 revision + 旧客户端自动应用
  （双通道真实到达远端已由本轮 404 冒烟证实，合并 main 后 URL 即生效）。

## 验证

运行与最终改动范围相称的完整验证：cargo test --workspace；
cargo fmt --all；cargo clippy --workspace --all-targets -- -D warnings；
前端 pnpm lint --fix、相关测试和 pnpm build；
Android 分叉 clippy 与模拟器冒烟，实机未验收如实记录。
检查数据发布工作流和文件树，避免因数据维护误触应用签名发布。

## 接手提示

代码库可能有其他人的改动；不回退他人修改。
最终交给人类验收，未经授权不合并、不推送；不创建无关工作或自动定时任务。
完成后留下最终 PR、测试/发布证据和仍待人类处理的具体项。
