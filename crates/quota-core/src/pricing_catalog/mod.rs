//! 定价目录：预置平台定价的 JSON 目录类型、完整校验与种子装载。
//!
//! 目录是预置定价的**单一数据源**：仓库 `data/pricing/v1/catalog.json`
//! 同时充当人工审核对象与构建时嵌入的离线种子（`include_str!`），
//! [`crate::pricing::preset`] 系列兼容入口由本模块构造结果，
//! 不再维护第二张 Rust 价格表。缓存同步与更新状态由后续票据在此模块扩展。
//!
//! 约束（spec §4/§5）：
//! - 未知价格为 `null`，零价格为 `0`，两者不得混淆；不要求三价齐全。
//! - 模型级 `windows` 缺省（`null`）= 继承平台级；`[]` = 恒空闲。
//! - 同币种套内模型 id 大小写不敏感碰撞视为重复。
//! - 默认模型必须存在且 active；允许整套没有默认模型（`null`）。
//! - retired 模型保留最后已知价格并记录 `retired_at`（物理删除由
//!   发布侧对比校验拦截，见 T-07；客户端侧缓存防降级见 T-03）。

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::pricing::{
    PeakWindow, PlanKind, PriceTier, PricingConfig, PricingError, validate as validate_pricing,
};

/// 目录格式主版本；不匹配的包按「格式不兼容」处理，客户端回退内置种子。
pub const CATALOG_SCHEMA_VERSION: u32 = 1;

/// `revision` 上限：JS 安全整数（2^53-1），保证前端与脚本可直接比较。
pub const MAX_REVISION: u64 = 9_007_199_254_740_991;

/// 内置种子（仓库根 `data/pricing/v1/catalog.json`，构建时嵌入）。
/// 正确性由快照测试与种子加载测试锁定；损坏属构建期错误。
const BUNDLED_SEED: &str = include_str!("../../../../data/pricing/v1/catalog.json");

// ---- 错误 -------------------------------------------------------------------

/// 目录加载/校验错误（带字段定位，两端展示模式与 `PricingError` 一致）。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CatalogError {
    #[error("字段 {field}：{reason}")]
    Validation { field: String, reason: String },
    #[error("JSON 解析失败：{0}")]
    Parse(String),
}

impl From<serde_json::Error> for CatalogError {
    fn from(e: serde_json::Error) -> Self {
        CatalogError::Parse(e.to_string())
    }
}

fn validation_error(field: &str, reason: String) -> CatalogError {
    CatalogError::Validation {
        field: field.into(),
        reason,
    }
}

// ---- 类型（owned：可从 JSON 反序列化，不泄漏静态字符串） --------------------

/// 模型生命周期状态：`retired` 保留最后已知价格供展示，不参与新配置默认选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    /// 在售（或订阅可订）模型。
    #[default]
    Active,
    /// 已下架：保留最后经审核的价格、来源与 `retired_at`。
    Retired,
}

/// 目录单模型条目（`Option` 字段区分「未知」与「零值/空集」）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogModel {
    /// 稳定匹配键（保持现有短 ID，不随官网改名更换）。
    pub id: String,
    /// 展示名。
    pub display: String,
    /// 计费模式（订阅项价格档为 null、窗口表达折扣时段）。
    pub plan: PlanKind,
    /// 模型级峰谷窗口覆盖：`None` = 继承平台级；`Some(vec![])` = 恒空闲。
    #[serde(default)]
    pub windows: Option<Vec<PeakWindow>>,
    /// 高峰档；`None` = 未知（订阅项/未核验），档内单项仍可缺值。
    #[serde(default)]
    pub peak: Option<PriceTier>,
    /// 空闲档；语义同 `peak`。
    #[serde(default)]
    pub off_peak: Option<PriceTier>,
    /// 生命周期状态（缺省 active，兼容省略字段的旧手写数据）。
    #[serde(default)]
    pub status: ModelStatus,
    /// 官方定价页来源（人工审核输入；空 = 未记录）。
    #[serde(default)]
    pub source_urls: Vec<String>,
    /// 价格核验日期（`YYYY-MM-DD` 或 RFC3339）；`None` = 核验时间未知，
    /// 不得以抓取/发布时间冒充。
    #[serde(default)]
    pub verified_at: Option<String>,
    /// 下架日期；仅 `retired` 模型允许非空。
    #[serde(default)]
    pub retired_at: Option<String>,
}

/// 币种套：一个平台在某币种下的完整定价（窗口 + 模型集 + 默认模型）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogSuite {
    /// 币种代码（如 "CNY"；与套内价格同币种，不做自动换算）。
    pub currency: String,
    /// UTC 偏移（分钟）。
    pub timezone_offset_minutes: i32,
    /// 平台级高峰窗口（模型级 `windows` 为 `None` 时生效；空 = 平台恒空闲）。
    #[serde(default)]
    pub windows: Vec<PeakWindow>,
    /// 默认模型 id；`None` = 整套没有 active 模型（发布审核显式置空）。
    #[serde(default)]
    pub default_model: Option<String>,
    pub models: Vec<CatalogModel>,
}

/// 平台目录：同一 native id 下按币种分套（如 DeepSeek CNY/USD）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogProvider {
    /// 已有供应商 ID（目录不新增查询供应商，见 spec §4.3）。
    pub native_id: String,
    /// 平台默认币种（条目未指定时兼容入口取这套）。
    pub default_currency: String,
    pub suites: Vec<CatalogSuite>,
}

/// 目录包顶层信封。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    /// 格式主版本，首期 1；不匹配即格式不兼容。
    pub schema_version: u32,
    /// 单调递增的数据版本（同 revision 不允许不同内容；回滚以更高 revision
    /// 重发旧内容完成，不降级客户端已接受版本）。
    pub revision: u64,
    /// 发布日期（`YYYY-MM-DD` 或 RFC3339）；`None` = 未记录。
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub providers: Vec<CatalogProvider>,
}

// ---- 解析与校验 --------------------------------------------------------------

/// 解析并完整校验目录 JSON（严格 UTF-8 由 `&str` 入参保证）。
/// 未通过校验的包不得进入缓存或内存快照（spec §5.1：先完整校验再整体切换）。
pub fn parse_catalog(json: &str) -> Result<Catalog, CatalogError> {
    let catalog: Catalog = serde_json::from_str(json)?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

/// 完整校验目录：格式版本、revision 范围、ID 唯一性（含大小写碰撞）、
/// 价格非负有限、窗口/时区合法性（复用 [`crate::pricing::validate`] 语义）、
/// 默认模型引用有效性、生命周期字段一致性、日期格式。
pub fn validate_catalog(catalog: &Catalog) -> Result<(), CatalogError> {
    if catalog.schema_version != CATALOG_SCHEMA_VERSION {
        return Err(validation_error(
            "schema_version",
            format!(
                "不支持的目录格式版本 {}（本程序支持 {CATALOG_SCHEMA_VERSION}）",
                catalog.schema_version
            ),
        ));
    }
    if catalog.revision < 1 || catalog.revision > MAX_REVISION {
        return Err(validation_error(
            "revision",
            format!(
                "数据版本须为 1..={MAX_REVISION} 的正整数，当前 {}",
                catalog.revision
            ),
        ));
    }
    if let Some(at) = catalog.published_at.as_deref()
        && !valid_date(at)
    {
        return Err(validation_error(
            "published_at",
            format!("发布时间须为 YYYY-MM-DD 或 RFC3339，当前 {at}"),
        ));
    }
    for (pi, provider) in catalog.providers.iter().enumerate() {
        // 平台间 native_id 不得重复（注册键精确匹配）
        if catalog.providers[..pi]
            .iter()
            .any(|p| p.native_id == provider.native_id)
        {
            return Err(validation_error(
                &format!("providers[{pi}].native_id"),
                format!("平台 {} 重复", provider.native_id),
            ));
        }
        validate_provider(pi, provider)?;
    }
    Ok(())
}

fn validate_provider(pi: usize, provider: &CatalogProvider) -> Result<(), CatalogError> {
    let field = |name: &str| format!("providers[{pi}].{name}");
    if provider.native_id.trim().is_empty() {
        return Err(validation_error(
            &field("native_id"),
            "native id 不能为空".into(),
        ));
    }
    if provider.default_currency.trim().is_empty() {
        return Err(validation_error(
            &field("default_currency"),
            "默认币种不能为空".into(),
        ));
    }
    if provider.suites.is_empty() {
        return Err(validation_error(
            &field("suites"),
            "至少需要一个币种套".into(),
        ));
    }
    for (si, suite) in provider.suites.iter().enumerate() {
        validate_suite(pi, si, suite)?;
    }
    // 币种套不重复（大小写不敏感：防止 "CNY"/"cny" 两套并存）
    for (si, suite) in provider.suites.iter().enumerate() {
        if provider.suites[..si]
            .iter()
            .any(|s| s.currency.eq_ignore_ascii_case(&suite.currency))
        {
            return Err(validation_error(
                &format!("providers[{pi}].suites[{si}].currency"),
                format!("币种套 {} 重复", suite.currency),
            ));
        }
    }
    // 默认币种必须命中某套（兼容入口的选套锚点）
    if !provider
        .suites
        .iter()
        .any(|s| s.currency.eq_ignore_ascii_case(&provider.default_currency))
    {
        return Err(validation_error(
            &field("default_currency"),
            format!("默认币种 {} 须命中实际的币种套", provider.default_currency),
        ));
    }
    Ok(())
}

fn validate_suite(pi: usize, si: usize, suite: &CatalogSuite) -> Result<(), CatalogError> {
    let field = |name: &str| format!("providers[{pi}].suites[{si}].{name}");
    if suite.currency.trim().is_empty() {
        return Err(validation_error(&field("currency"), "币种不能为空".into()));
    }
    // 时区与窗口语义复用 pricing::validate（单一来源；Some([]) 恒空闲合法）
    let cfg = PricingConfig {
        timezone_offset_minutes: Some(suite.timezone_offset_minutes),
        windows: Some(suite.windows.clone()),
        ..Default::default()
    };
    if let Err(PricingError::Validation { field: f, reason }) = validate_pricing(&cfg) {
        return Err(validation_error(&field(&f), reason));
    }
    for (mi, model) in suite.models.iter().enumerate() {
        validate_model(pi, si, mi, model)?;
    }
    // 套内模型 id 唯一（精确重复与大小写碰撞同拒——现有匹配即大小写不敏感）
    for (mi, model) in suite.models.iter().enumerate() {
        if suite.models[..mi]
            .iter()
            .any(|m| m.id.eq_ignore_ascii_case(&model.id))
        {
            return Err(validation_error(
                &format!("providers[{pi}].suites[{si}].models[{mi}].id"),
                format!("模型 id {} 与同套既有模型大小写碰撞", model.id),
            ));
        }
    }
    // 默认模型必须命中且 active；空默认合法（整套无 active 模型）
    if let Some(default_id) = suite.default_model.as_deref() {
        let default = suite
            .models
            .iter()
            .find(|m| m.id.eq_ignore_ascii_case(default_id));
        match default {
            None => {
                return Err(validation_error(
                    &field("default_model"),
                    format!("默认模型 {default_id} 不在套内"),
                ));
            }
            Some(m) if m.status != ModelStatus::Active => {
                return Err(validation_error(
                    &field("default_model"),
                    format!("默认模型 {default_id} 已下架，须改指 active 模型或置空"),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_model(
    pi: usize,
    si: usize,
    mi: usize,
    model: &CatalogModel,
) -> Result<(), CatalogError> {
    let field = |name: &str| format!("providers[{pi}].suites[{si}].models[{mi}].{name}");
    if model.id.trim().is_empty() {
        return Err(validation_error(&field("id"), "模型 id 不能为空".into()));
    }
    if model.display.trim().is_empty() {
        return Err(validation_error(&field("display"), "展示名不能为空".into()));
    }
    // 模型级窗口复用同一校验（None = 继承，跳过）
    if let Some(windows) = model.windows.as_ref() {
        let cfg = PricingConfig {
            windows: Some(windows.clone()),
            ..Default::default()
        };
        if let Err(PricingError::Validation { field: f, reason }) = validate_pricing(&cfg) {
            return Err(validation_error(&field(&f), reason));
        }
    }
    for (name, tier) in [("peak", &model.peak), ("off_peak", &model.off_peak)] {
        let Some(tier) = tier else { continue };
        for (price_name, value) in [
            ("cache_hit_input", &tier.cache_hit_input),
            ("cache_miss_input", &tier.cache_miss_input),
            ("output", &tier.output),
        ] {
            if let Some(v) = value
                && !(v.is_finite() && *v >= 0.0)
            {
                return Err(validation_error(
                    &field(&format!("{name}.{price_name}")),
                    format!("价格须为非负有限数，当前 {v}"),
                ));
            }
        }
    }
    if let Some(at) = model.verified_at.as_deref()
        && !valid_date(at)
    {
        return Err(validation_error(
            &field("verified_at"),
            format!("核验时间须为 YYYY-MM-DD 或 RFC3339，当前 {at}"),
        ));
    }
    // 生命周期与 retired_at 一致
    match model.status {
        ModelStatus::Retired if model.retired_at.is_none() => {
            return Err(validation_error(
                &field("retired_at"),
                "已下架模型必须记录下架日期".into(),
            ));
        }
        ModelStatus::Active if model.retired_at.is_some() => {
            return Err(validation_error(
                &field("retired_at"),
                "在售模型不应携带下架日期".into(),
            ));
        }
        _ => {}
    }
    if let Some(at) = model.retired_at.as_deref()
        && !valid_date(at)
    {
        return Err(validation_error(
            &field("retired_at"),
            format!("下架日期须为 YYYY-MM-DD 或 RFC3339，当前 {at}"),
        ));
    }
    Ok(())
}

/// 日期格式：`YYYY-MM-DD` 或 RFC3339 时间戳。
fn valid_date(s: &str) -> bool {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
        || chrono::DateTime::parse_from_rfc3339(s).is_ok()
}

// ---- 装载与选套 --------------------------------------------------------------

/// 内置种子目录（进程内单例）。种子随版本构建且由测试锁定，
/// 解析失败属构建期错误而非运行时数据问题。
pub fn bundled_catalog() -> &'static Catalog {
    static BUNDLED: OnceLock<Catalog> = OnceLock::new();
    BUNDLED.get_or_init(|| {
        parse_catalog(BUNDLED_SEED)
            .expect("内置定价目录种子未通过校验：data/pricing/v1/catalog.json 与校验规则不一致，属构建期错误")
    })
}

/// 按平台与币种提示选套，语义镜像旧 `preset_with_currency`：
/// 多套平台（DeepSeek）按 hint 大小写不敏感匹配，未命中或无 hint 回落
/// `default_currency` 套；单套平台忽略 hint。无该平台 → None。
pub fn find_suite<'a>(
    catalog: &'a Catalog,
    native_id: &str,
    currency_hint: Option<&str>,
) -> Option<&'a CatalogSuite> {
    let provider = catalog
        .providers
        .iter()
        .find(|p| p.native_id == native_id)?;
    match provider.suites.as_slice() {
        [only] => Some(only),
        _ => {
            let fallback = provider
                .suites
                .iter()
                .find(|s| s.currency.eq_ignore_ascii_case(&provider.default_currency));
            match currency_hint {
                Some(hint) => provider
                    .suites
                    .iter()
                    .find(|s| s.currency.eq_ignore_ascii_case(hint))
                    .or(fallback),
                None => fallback,
            }
        }
    }
}

// ---- 测试 --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小合法目录（变异测试基座：各拒绝用例在此基础上改一处）。
    const BASE: &str = r#"{
        "schema_version": 1,
        "revision": 1,
        "published_at": "2026-09-10",
        "providers": [{
            "native_id": "demo",
            "default_currency": "CNY",
            "suites": [{
                "currency": "CNY",
                "timezone_offset_minutes": 480,
                "windows": [],
                "default_model": "m1",
                "models": [{
                    "id": "m1",
                    "display": "M1",
                    "plan": "pay_as_you_go",
                    "windows": null,
                    "peak": null,
                    "off_peak": null,
                    "status": "active",
                    "source_urls": [],
                    "verified_at": null,
                    "retired_at": null
                }]
            }]
        }]
    }"#;

    /// 基座变异：`f` 直接改 JSON 树后重新序列化。
    fn variant(mut f: impl FnMut(&mut serde_json::Value)) -> String {
        let mut v: serde_json::Value = serde_json::from_str(BASE).unwrap();
        f(&mut v);
        v.to_string()
    }

    fn suite(v: &mut serde_json::Value) -> &mut serde_json::Value {
        &mut v["providers"][0]["suites"][0]
    }

    fn model(v: &mut serde_json::Value) -> &mut serde_json::Value {
        &mut v["providers"][0]["suites"][0]["models"][0]
    }

    fn expect_reject(json: &str, expect_field_contains: &str) {
        let err = parse_catalog(json).unwrap_err();
        assert!(
            err.to_string().contains(expect_field_contains),
            "应点名 {expect_field_contains}，实际：{err}"
        );
    }

    // ---- 种子加载（V-01 数据侧入口） ----

    /// 契约：内置种子解析并通过完整校验；版本字段就位。
    #[test]
    fn bundled_seed_loads_and_validates() {
        let cat = bundled_catalog();
        assert_eq!(cat.schema_version, 1);
        assert!(cat.revision >= 1);
        assert!(cat.published_at.is_some());
    }

    /// 契约：种子覆盖全部既有预置平台（9 平台 10 币种套，DeepSeek 双币），
    /// 且各平台 default_currency 与 pricing::default_currency 查表一致
    /// （目录是价格数据的单一来源，币种默认表不得与之分叉）。
    #[test]
    fn seed_covers_all_preset_providers() {
        let cat = bundled_catalog();
        assert_eq!(cat.providers.len(), 9);
        let suite_count: usize = cat.providers.iter().map(|p| p.suites.len()).sum();
        assert_eq!(suite_count, 10, "DeepSeek 双币，其余 8 平台各一套");
        for provider in &cat.providers {
            assert_eq!(
                provider.default_currency,
                crate::pricing::default_currency(&provider.native_id),
                "{} 的目录默认币种须与查表一致",
                provider.native_id
            );
        }
    }

    /// 契约：find_suite 选套语义与旧 preset_with_currency 等价——
    /// 多套平台按 hint 大小写不敏感选套、未命中/无 hint 回落默认币种套；
    /// 单套平台忽略 hint；未知平台 None。
    #[test]
    fn find_suite_mirrors_preset_with_currency() {
        let cat = bundled_catalog();
        let deepseek =
            |hint: Option<&str>| find_suite(cat, "deepseek", hint).unwrap().currency.clone();
        assert_eq!(deepseek(None), "CNY", "无 hint 取默认币种套");
        assert_eq!(deepseek(Some("USD")), "USD");
        assert_eq!(deepseek(Some("usd")), "USD", "大小写不敏感");
        assert_eq!(
            deepseek(Some("JPY")),
            "CNY",
            "未命中 hint 回落默认套（不产生第三套）"
        );
        assert_eq!(
            find_suite(cat, "kimi_cn", Some("USD")).unwrap().currency,
            "CNY",
            "单套平台忽略 hint"
        );
        assert!(find_suite(cat, "nope", None).is_none());
    }

    // ---- 拒绝：包级 ----

    /// 契约：未知 schema_version 拒绝（格式不兼容，客户端须回退种子）。
    #[test]
    fn rejects_unknown_schema_version() {
        expect_reject(
            &variant(|v| v["schema_version"] = 2.into()),
            "schema_version",
        );
        expect_reject(
            &variant(|v| v["schema_version"] = 0.into()),
            "schema_version",
        );
    }

    /// 契约：revision 必须是 1..=2^53-1（JS 安全整数）内的正整数。
    #[test]
    fn rejects_out_of_range_revision() {
        expect_reject(&variant(|v| v["revision"] = 0.into()), "revision");
        expect_reject(
            &variant(|v| v["revision"] = (MAX_REVISION + 1).into()),
            "revision",
        );
    }

    /// 契约：published_at 非法日期格式拒绝。
    #[test]
    fn rejects_bad_published_at() {
        expect_reject(
            &variant(|v| v["published_at"] = "2026/09/10".into()),
            "published_at",
        );
    }

    /// 契约：非法 JSON 拒绝（serde 层）。
    #[test]
    fn rejects_malformed_json() {
        assert!(parse_catalog("{ not json").is_err());
        assert!(parse_catalog("").is_err());
    }

    // ---- 拒绝：平台级 ----

    /// 契约：空白 native_id、重复平台、空 suites、默认币种未命中套均拒绝。
    #[test]
    fn rejects_provider_level_violations() {
        expect_reject(
            &variant(|v| v["providers"][0]["native_id"] = " ".into()),
            "native_id",
        );
        expect_reject(
            &variant(|v| {
                let dup = v["providers"][0].clone();
                v["providers"].as_array_mut().unwrap().push(dup);
            }),
            "providers[1].native_id",
        );
        expect_reject(
            &variant(|v| v["providers"][0]["suites"] = serde_json::json!([])),
            "suites",
        );
        expect_reject(
            &variant(|v| v["providers"][0]["default_currency"] = "USD".into()),
            "default_currency",
        );
    }

    /// 契约：未知计费模式与未知生命周期状态被 serde 拒绝。
    #[test]
    fn rejects_unknown_plan_and_status_values() {
        expect_reject(&variant(|v| model(v)["plan"] = "per_token".into()), "解析");
        expect_reject(
            &variant(|v| model(v)["status"] = "deprecated".into()),
            "解析",
        );
    }

    // ---- 拒绝：套级 ----

    /// 契约：币种套重复（精确与大小写不敏感）、非法时区、非法窗口拒绝。
    #[test]
    fn rejects_suite_level_violations() {
        expect_reject(
            &variant(|v| {
                let mut dup = suite(v).clone();
                dup["currency"] = "cny".into();
                v["providers"][0]["suites"]
                    .as_array_mut()
                    .unwrap()
                    .push(dup);
            }),
            "currency",
        );
        expect_reject(
            &variant(|v| suite(v)["timezone_offset_minutes"] = 900.into()),
            "timezone_offset_minutes",
        );
        // 跨日窗口（复用 pricing::validate 语义）
        expect_reject(
            &variant(|v| {
                suite(v)["windows"] = serde_json::json!([
                    {"days": ["mon"], "start": "22:00", "end": "06:00"}
                ]);
            }),
            "windows",
        );
        // 非法时刻
        expect_reject(
            &variant(|v| {
                suite(v)["windows"] = serde_json::json!([
                    {"days": ["mon"], "start": "25:00", "end": "26:00"}
                ]);
            }),
            "windows",
        );
    }

    /// 契约：默认模型不在套内、指向 retired 模型均拒绝；置空默认合法。
    #[test]
    fn rejects_invalid_default_model() {
        expect_reject(
            &variant(|v| suite(v)["default_model"] = "absent".into()),
            "default_model",
        );
        expect_reject(
            &variant(|v| {
                model(v)["status"] = "retired".into();
                model(v)["retired_at"] = "2026-09-01".into();
            }),
            "default_model",
        );
        // 整套无 active 默认：显式置空 + 唯一模型 retired = 合法
        let json = variant(|v| {
            model(v)["status"] = "retired".into();
            model(v)["retired_at"] = "2026-09-01".into();
            suite(v)["default_model"] = serde_json::Value::Null;
        });
        assert!(
            parse_catalog(&json).is_ok(),
            "空默认模型 + 全 retired 套合法"
        );
    }

    // ---- 拒绝：模型级 ----

    /// 契约：套内模型 id 精确重复与大小写碰撞同拒（匹配即大小写不敏感）。
    #[test]
    fn rejects_model_id_collisions() {
        expect_reject(
            &variant(|v| {
                let dup = model(v).clone();
                v["providers"][0]["suites"][0]["models"]
                    .as_array_mut()
                    .unwrap()
                    .push(dup);
            }),
            "models[1].id",
        );
        expect_reject(
            &variant(|v| {
                let mut dup = model(v).clone();
                dup["id"] = "M1".into();
                v["providers"][0]["suites"][0]["models"]
                    .as_array_mut()
                    .unwrap()
                    .push(dup);
            }),
            "models[1].id",
        );
    }

    /// 契约：空白 id/display、负价拒绝；非有限价（∞/NaN——JSON 文本无法
    /// 表达，属反序列化后的病态数据）由校验器直接拒绝；模型级非法窗口
    /// 同样复用 pricing 校验。
    #[test]
    fn rejects_model_level_violations() {
        expect_reject(&variant(|v| model(v)["id"] = " ".into()), "id");
        expect_reject(&variant(|v| model(v)["display"] = "".into()), "display");
        expect_reject(
            &variant(|v| model(v)["peak"] = serde_json::json!({"output": -1})),
            "peak.output",
        );
        let mut cat = parse_catalog(BASE).unwrap();
        for bad in [f64::INFINITY, f64::NAN] {
            cat.providers[0].suites[0].models[0].peak = Some(PriceTier {
                output: Some(bad),
                ..Default::default()
            });
            let err = validate_catalog(&cat).unwrap_err();
            assert!(
                err.to_string().contains("peak.output"),
                "非有限价 {bad} 应被点名，实际：{err}"
            );
        }
        expect_reject(
            &variant(|v| {
                model(v)["windows"] =
                    serde_json::json!([{"days": [], "start": "09:00", "end": "12:00"}]);
            }),
            "models[0].windows",
        );
    }

    /// 契约：生命周期字段一致性——retired 必须带 retired_at，
    /// active 不得携带；日期格式非法拒绝。
    #[test]
    fn rejects_lifecycle_inconsistency() {
        expect_reject(
            &variant(|v| {
                model(v)["status"] = "retired".into();
            }),
            "retired_at",
        );
        expect_reject(
            &variant(|v| model(v)["retired_at"] = "2026-09-01".into()),
            "retired_at",
        );
        expect_reject(
            &variant(|v| {
                model(v)["status"] = "retired".into();
                model(v)["retired_at"] = "09/01/2026".into();
            }),
            "retired_at",
        );
        expect_reject(
            &variant(|v| model(v)["verified_at"] = "昨天".into()),
            "verified_at",
        );
    }

    // ---- 语义区分（null ≠ 0，缺省 ≠ 空数组） ----

    /// 契约：未知价格是 null、零价格是 0，解析后互不相同；
    /// 档位 null（未知）与档内单项缺失（部分未知）均可表达且合法。
    #[test]
    fn null_price_differs_from_zero() {
        let json = variant(|v| {
            model(v)["peak"] = serde_json::json!({
                "cache_hit_input": 0,
                "cache_miss_input": null,
                "output": null
            });
        });
        let cat = parse_catalog(&json).unwrap();
        let m = &cat.providers[0].suites[0].models[0];
        assert_eq!(m.peak.as_ref().unwrap().cache_hit_input, Some(0.0));
        assert_eq!(m.peak.as_ref().unwrap().cache_miss_input, None);
        assert_eq!(m.off_peak, None, "档位 null = 未知，非零价");
        assert_eq!(
            serde_json::to_value(m).unwrap()["off_peak"],
            serde_json::Value::Null
        );
    }

    /// 契约：模型级 windows 缺省（null）= 继承平台级，空数组 = 恒空闲，
    /// 解析后互不相同（Option 语义）。
    #[test]
    fn windows_null_inherits_and_empty_means_flat() {
        let cat = parse_catalog(BASE).unwrap();
        assert_eq!(cat.providers[0].suites[0].models[0].windows, None);
        let json = variant(|v| {
            model(v)["windows"] = serde_json::json!([]);
        });
        let cat = parse_catalog(&json).unwrap();
        assert_eq!(cat.providers[0].suites[0].models[0].windows, Some(vec![]));
    }

    /// 契约：RFC3339 完整时间戳同样被接受（未来机器生成的发布时间）。
    #[test]
    fn accepts_rfc3339_dates() {
        let json = variant(|v| v["published_at"] = "2026-09-10T08:00:00Z".into());
        assert!(parse_catalog(&json).is_ok());
    }

    /// 契约：目录 JSON roundtrip（缓存写入/重读的格式基础）。
    #[test]
    fn catalog_serde_roundtrip() {
        let cat = bundled_catalog();
        let json = serde_json::to_string(cat).unwrap();
        let back: Catalog = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, &back);
    }
}
