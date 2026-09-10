# T-01 提取可加载目录与兼容接口

状态：已完成（2026-09-10，待所有者验收）  
Blocked by: 无  
规格：[§2、§4、§5、§9](../spec.md)  
解锁：T-02。

## 目标

将已有硬编码预置抽成同一份可校验 JSON，运行结果保持等价。
这是 core 公开接口准备票，独立 PR 合入后才能迁移调用端。

## 范围与责任

主要负责 crates/quota-core/src/pricing.rs、新 pricing_catalog 模块、
core lib.rs、data/pricing/v1/catalog.json 及对应测试和文件树。
不负责网络、端侧设置页或发布。

1. 增加 owned 平台/模型目录类型与完整加载校验；保留现有 ID 和币种套。
2. 将当前所有预置逐项提取为构建时嵌入的 JSON 种子；不得顺便改价格。
3. 固定目录参数化解析所需的公开类型，包括模型生命周期、价格来源和核验信息。
4. 保留旧 preset / resolve 入口为兼容入口，让现有 CLI / GUI 继续编译运行。
5. 内部复用同一份数据，不引入第二张 Rust 价格表，不泄漏字符串换取静态引用。

## 验收

- [x] 对全部既有预置、默认模型、ID、CNY/USD、三价、时段、订阅项做旧结果与新目录等价检查。（现有快照测试一字未改全部通过；审查子代理对 git show main 硬编码逐数字核对一致）
- [x] 同一模型大小写碰撞、非法默认模型、非法价格/窗口、未知格式均被拒绝。（pricing_catalog 契约测试：大小写碰撞/重复 ID/负价/非有限价/跨日窗口/非法时区/schema_version/retired_at 一致性等 18 例）
- [x] null 价格与 0 价格不同；windows 缺省与空数组不同。（专项测试 null_price_differs_from_zero、windows_null_inherits_and_empty_means_flat）
- [x] 旧配置和两个现有应用端无需迁移即可使用兼容入口。（resolve/resolve_impl/default_currency 未动；CLI 与桌面端仅 String 字段机械适配，无迁移）
- [x] 没有联网、落用户配置或调整促销口径。
- [x] core 公开 API 变更以单独 PR 交付，记录接口供 T-02/T-03 使用。（见下方接口记录；同 PR 含两端最小机械适配——字段 String 化不拆两步否则无法编译）

## 实施记录（2026-09-10）

- 分支 `feat/pricing-catalog-foundation`，提交 `b50d29b`（本地，未推送）。
- 门禁：`cargo fmt --all`、`cargo clippy --workspace --all-targets -- -D warnings`、
  `cargo test --workspace` 全绿（core 388 / CLI 132 / 桌面 126 等全部通过）。
- 双轴审查（Standards/Spec 并行子代理）：无硬违规、无等价性偏差、无 scope creep。
  判断性遗留：default_currency 查表与目录字段双源（测试锁定一致，T-03 收敛）；
  `suite_to_preset` 空默认模型映射空串哨兵（T-02 引入生命周期语义时消除）；
  套级/平台级 windows 无独立核验字段（未来改窗口时按 spec §4.2 补核验信息）。
- 种子核验日期口径：DeepSeek CNY 2026-09-09 / USD 2026-08-23、智谱/Z.ai
  2026-09-09、Kimi 2026-08-23 均出自原 pricing.rs 注释或 git 历史；
  kimi_code 无据可查，verified_at 留 null、source_urls 留空。

### 已确定接口（供 T-02/T-03 消费）

- 类型（core lib.rs 已 re-export）：
  `Catalog { schema_version, revision, published_at, providers }`、
  `CatalogProvider { native_id, default_currency, suites }`、
  `CatalogSuite { currency, timezone_offset_minutes, windows, default_model: Option<String>, models }`、
  `CatalogModel { id, display, plan, windows, peak/off_peak: Option<PriceTier>, status, source_urls, verified_at, retired_at }`、
  `ModelStatus { Active, Retired }`。
- 函数：`parse_catalog(&str) -> Result<Catalog, CatalogError>`（解析+完整校验）、
  `validate_catalog(&Catalog)`、`bundled_catalog() -> &'static Catalog`（include_str 种子单例）、
  `find_suite(&Catalog, native_id, Option<currency_hint>) -> Option<&CatalogSuite>`
  （镜像旧 preset_with_currency 选套：多套按 hint 大小写不敏感、未命中回落
  default_currency、单套忽略）。
- 兼容入口：`preset()/preset_with_currency()` 签名不变，内部经
  `suite_to_preset` 从目录构造；`PresetModel/PresetProvider` 字段已 owned 化
  （`&'static str` → `String`），调用端读法不变。
- 常量：`CATALOG_SCHEMA_VERSION = 1`、`MAX_REVISION = 2^53-1`。

## 验证

先添加 V-01 和目录加载契约测试；运行相关 core 测试，
cargo fmt --all，cargo clippy --workspace --all-targets -- -D warnings。
依赖变更不得突破 MSRV 1.88。

（依赖零新增，MSRV 无影响。）

## 接手提示

代码库可能有其他人的改动；只负责本票据范围，不回退他人修改。
当前价格注释包含来源口径，提取前核对当前 HEAD，不以规格中的旧行号定位。
完成后在本票据记录提交、PR、测试结果和已确定接口。
