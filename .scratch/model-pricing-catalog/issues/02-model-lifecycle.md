# T-02 修正模型缺失与下架展示

状态：已完成（2026-09-10，待所有者验收；提交 113c8b8）  
Blocked by: T-01（已合入同分支）  
规格：[§5、§10 V-02/V-03/V-12](../spec.md)  
解锁：T-03。

## 目标

用户指定 A 时，任何情况下都不能用默认模型 B 的价格冒充 A。
先通过 core 公开解析接口和 CLI 定价输出完成可观察的闭环。

## 范围与责任

负责 core pricing / pricing_catalog 的解析和生命周期校验，
CLI pricing.rs、pricing_models.rs、render/texts 的必要输出与测试。
沿用 T-01 已提供的公开类型，若发现必须再改公开 API，先拆出独立 core PR。

1. 显式模型依次匹配自定义库、active 官方、retired 官方；未命中返回 missing 状态。
2. 只有配置未指定模型时才使用 active 默认模型。
3. retired 记录继续携带同模型最后已知价格，并在 CLI 标记已下架。
4. missing 保留名称，缺失价格为 null / 未知；不借用默认模型的折扣或计费模式。
5. CLI JSON 在既有 source=preset/custom 之外增加状态信息，保持已有字段含义。
6. 对录入数据的物理删除、retired 默认模型和跨币种串价增加契约检查。

## 验收

- [x] A active → A retired 后，CLI 仍显示 A 的价格与“已下架”。
- [x] 明确选择缺失 A 且默认 B 存在时，结果不含 B 的价格、模型级时段或计费模式。
- [x] 没选模型时仍正确显示平台默认。
- [x] 用户手填整档优先；半档空项仍空；自定义库撞名优先且空价不借官方同名价。
- [x] 新安装目录直接包含 retired A 时，无旧缓存也能显示 A 的历史价格。
- [x] 未知模型不被错误标成已下架；订阅项无货币价格不被错误标为同步失败。
- [x] 中英文 CLI 文案和 JSON null/0 区分通过验证。

## 实施记录（2026-09-10）

- 提交 `113c8b8`（分支 feat/pricing-catalog-foundation）。门禁全绿：
  fmt / clippy --workspace --all-targets -D warnings / cargo test --workspace
  （core 398、CLI 135、桌面 126）/ 前端 tsc + eslint + vitest 229。
- 行为差异：唯一语义变更是「显式指定模型未命中时不再回退默认模型定价」
  （旧注释自认的缺陷，spec §2 已核实方向）；retired/missing 四态由
  ResolvedPricing.model_status 透出（ResolvedModelStatus），source 口径不变。
- 新接口：resolve_in_catalog(entry, custom, hint, catalog)（目录参数化，
  旧三入口等价委托种子）；validate_no_removal(new, baseline)（物理删除
  对比校验，T-03 缓存与 T-07 发布共用）；PresetModel/DTO/前端镜像 +status。
- 已知分叉（T-05 收敛）：前端 providerPricing.ts 镜像仍持旧回退语义，
  其「未知模型价格回退默认」测试仍锁定旧行为；GUI 生命周期展示未接。
- 验收对照：A→retired 仍显示最后价+已下架（show_retired_model_last_known_with_hint）；
  缺失 A 不含 B 价格/时段/计费（resolve_missing_model_keeps_name_and_unknown_prices、
  resolve_missing_not_borrows_subscription_default）；未选模型仍平台默认
  （resolve_unspecified_uses_active_default）；手填整档/半档/撞名/windows
  语义保持（resolve_missing_model_manual_tier_wins、resolve_custom_shadows_
  retired_official 及既有 V-03 测试全绿）；新装含 retired 直接展示
  （resolve_retired_model_keeps_last_known，注入目录 = 新装无旧缓存同构）；
  未知≠已下架、订阅 null≠失败（model_status 区分 + null 断言）；
  中英双语与 null/0 区分（show_missing_model_status_and_hint 等双语断言）。

## 验证

从 core 公开解析接口与 CLI show/model list 的可见输出测试 V-02/V-03/V-12。
先失败测试再改实现；Rust fmt、workspace all-targets clippy 和相关测试全部通过。
本票不接网络、不实现 GUI；GUI 对应行为由 T-05 消费同一契约。

## 接手提示

代码库可能有其他人的改动；不回退他人修改。
旧逻辑把任意模型名当自定义标签的能力仍可保留，但未知价格不能借其他模型补齐。
完成后记录行为差异、提交/PR 和测试证据。
