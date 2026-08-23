//! `quota pricing model`：自定义模型库管理（按 native id 聚类）。
//!
//! 库的解析与生效在 `quota_core::pricing::resolve_with`（条目 `pricing.model`
//! 撞名时自定义优先）；本模块只做列表展示与增删编排。
//! `models_json` / `render_models_table` / `apply_add` / `apply_remove`
//! 为纯函数（测试不碰真实时钟与网络）。

use quota_core::AppConfig;
use quota_core::pricing::{self, CustomModelDef, PlanKind, PriceTier};
use quota_core::provider;
use serde::Serialize;

use crate::ctx::Ctx;
use crate::lang::Lang;
use crate::render;
use crate::texts::{self, T, t};

/// 单模型行的统一视图（预置/自定义同构，list --json 输出形状）。
#[derive(Serialize)]
pub struct ModelRowJson {
    pub id: String,
    pub display: String,
    /// "preset" | "custom"
    pub source: &'static str,
    /// "pay_as_you_go" | "subscription"
    pub plan: &'static str,
    /// 模型级窗口覆盖（预置订阅项在此携带折扣时段；null = 继承平台级）。
    pub windows: Option<Vec<quota_core::PeakWindow>>,
    pub peak: PriceTier,
    pub off_peak: PriceTier,
}

/// `pricing model list --json` 输出结构。
#[derive(Serialize)]
pub struct ModelListJson {
    pub provider: String,
    /// 平台默认币种（有预置时；无预置平台按 default_currency 兜底）。
    pub currency: String,
    /// 预置默认模型 id（无预置为 null）。
    pub default_model: Option<String>,
    pub models: Vec<ModelRowJson>,
}

/// 汇总平台预置与自定义模型（纯函数；provider 未注册返回 None）。
pub fn models_json(provider_id: &str, custom: &[CustomModelDef]) -> Option<ModelListJson> {
    provider::find(provider_id)?; // 未注册平台无库语义
    let preset = pricing::preset(provider_id);
    let mut models = Vec::new();
    if let Some(p) = &preset {
        for m in &p.models {
            models.push(ModelRowJson {
                id: m.id.into(),
                display: m.display.into(),
                source: "preset",
                plan: plan_str(m.plan),
                windows: m.windows.clone(),
                peak: m.peak.clone(),
                off_peak: m.off_peak.clone(),
            });
        }
    }
    for m in custom {
        models.push(ModelRowJson {
            id: m.id.clone(),
            display: m.display.clone(),
            source: "custom",
            // CustomModelDef 暂无 plan 字段（core from_lib_model 同口径硬编码
            // payg，放开时两处同步）
            plan: plan_str(PlanKind::PayAsYouGo),
            windows: m.windows.clone(),
            peak: m.peak.clone().unwrap_or_default(),
            off_peak: m.off_peak.clone().unwrap_or_default(),
        });
    }
    Some(ModelListJson {
        provider: provider_id.into(),
        currency: preset
            .as_ref()
            .map(|p| p.currency.into())
            .unwrap_or_else(|| pricing::default_currency(provider_id).into()),
        default_model: preset.as_ref().map(|p| p.default_model.into()),
        models,
    })
}

fn plan_str(plan: PlanKind) -> &'static str {
    match plan {
        PlanKind::PayAsYouGo => "pay_as_you_go",
        PlanKind::Subscription => "subscription",
    }
}

/// 三档价紧凑串（命中/输入/输出），缺失单值与全空档显示 "—"。
fn tier_cell(tier: &PriceTier) -> String {
    if tier.is_empty() {
        return "—".into();
    }
    let mut parts = Vec::new();
    for v in [tier.cache_hit_input, tier.cache_miss_input, tier.output] {
        match v {
            Some(x) => parts.push(pricing::format_price(x)),
            None => parts.push("—".into()),
        }
    }
    parts.join("/")
}

/// `pricing model list` 表格：模型 / id / 来源 / 模式 / 峰价 / 闲价（纯函数）。
pub fn render_models_table(list: &ModelListJson, lang: Lang) -> String {
    use comfy_table::Cell;
    let mut table = render::new_table(&[
        t(lang, T::ColModel),
        "id",
        t(lang, T::ColSource),
        t(lang, T::ColPlanKind),
        t(lang, T::ColPeakPrice),
        t(lang, T::ColOffPeakPrice),
    ]);
    for m in &list.models {
        table.add_row(vec![
            Cell::new(&m.display),
            Cell::new(&m.id),
            Cell::new(t(
                lang,
                if m.source == "preset" {
                    T::PricingModelSourcePreset
                } else {
                    T::PricingModelSourceCustom
                },
            )),
            Cell::new(t(
                lang,
                if m.plan == "subscription" {
                    T::ColPlanSubscription
                } else {
                    T::ColPlanPayg
                },
            )),
            Cell::new(tier_cell(&m.peak)),
            Cell::new(tier_cell(&m.off_peak)),
        ]);
    }
    table.to_string()
}

/// 添加/覆盖自定义模型（纯函数；同 id 大小写不敏感覆盖，与 resolve 匹配口径一致）。
pub fn apply_add(cfg: &mut AppConfig, provider_id: &str, model: CustomModelDef) {
    let entry = cfg
        .custom_models
        .entry(provider_id.to_string())
        .or_default();
    match entry
        .iter_mut()
        .find(|m| m.id.eq_ignore_ascii_case(&model.id))
    {
        Some(slot) => *slot = model,
        None => entry.push(model),
    }
}

/// 删除自定义模型（纯函数；大小写不敏感；不存在返回 false）。
/// 删空后移除平台键，保持配置文件不留空数组。
pub fn apply_remove(cfg: &mut AppConfig, provider_id: &str, id: &str) -> bool {
    let Some(list) = cfg.custom_models.get_mut(provider_id) else {
        return false;
    };
    let before = list.len();
    list.retain(|m| !m.id.eq_ignore_ascii_case(id));
    let removed = list.len() < before;
    if list.is_empty() {
        cfg.custom_models.remove(provider_id);
    }
    removed
}

/// 平台 id 未注册时输出错误并返回 false（run_* 共用）。
fn ensure_provider(ctx: &Ctx, provider_id: &str) -> bool {
    if provider::find(provider_id).is_some() {
        return true;
    }
    eprintln!(
        "{}{}",
        t(ctx.lang, T::Err),
        texts::pricing_model_provider_unknown(ctx.lang, provider_id)
    );
    false
}

/// `pricing model list`：未知平台 → 1。
pub fn run_list(ctx: &Ctx, provider_id: &str, json: bool) -> i32 {
    let lang = ctx.lang;
    if !ensure_provider(ctx, provider_id) {
        return 1;
    }
    let cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    let custom = cfg
        .custom_models
        .get(provider_id)
        .cloned()
        .unwrap_or_default();
    // ensure_provider 已拦截未注册 id，此处 None 仅剩注册表竞争修改的
    // 理论路径，防御回退到与入口同一双语文案
    let Some(list) = models_json(provider_id, &custom) else {
        eprintln!(
            "{}{}",
            t(lang, T::Err),
            texts::pricing_model_provider_unknown(lang, provider_id)
        );
        return 1;
    };
    if json {
        match serde_json::to_string_pretty(&list) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{}{e}", t(lang, T::Err));
                return 1;
            }
        }
    } else if list.models.is_empty() {
        println!("{}", t(lang, T::PricingModelListEmpty));
    } else {
        println!("{}", render_models_table(&list, lang));
    }
    0
}

/// 解析 `pricing model add` 的 stdin JSON 并校验（纯函数，错误路径可测）。
pub fn parse_add_input(text: &str, lang: Lang) -> Result<CustomModelDef, String> {
    let model: CustomModelDef =
        serde_json::from_str(text).map_err(|e| format!("{}{e}", t(lang, T::JsonParseFail)))?;
    pricing::validate_custom_model(&model)
        .map_err(|e| format!("{}{e}", t(lang, T::PricingValidateFail)))?;
    Ok(model)
}

/// `pricing model add`：stdin 读 CustomModelDef JSON → 校验 → 添加/覆盖。
pub fn run_add(ctx: &Ctx, provider_id: &str) -> i32 {
    let lang = ctx.lang;
    if !ensure_provider(ctx, provider_id) {
        return 1;
    }
    let mut text = String::new();
    if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text) {
        eprintln!("{}{}{e}", t(lang, T::Err), t(lang, T::StdinReadFail));
        return 1;
    }
    let model = match parse_add_input(&text, lang) {
        Ok(m) => m,
        Err(msg) => {
            eprintln!("{}{msg}", t(lang, T::Err));
            return 1;
        }
    };
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    apply_add(&mut cfg, provider_id, model.clone());
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    println!(
        "{}",
        texts::pricing_model_saved(lang, provider_id, &model.id)
    );
    0
}

/// `pricing model remove`：不存在 → 1。
pub fn run_remove(ctx: &Ctx, provider_id: &str, id: &str) -> i32 {
    let lang = ctx.lang;
    if !ensure_provider(ctx, provider_id) {
        return 1;
    }
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    if !apply_remove(&mut cfg, provider_id, id) {
        eprintln!(
            "{}{}",
            t(lang, T::Err),
            texts::pricing_model_not_found(lang, provider_id, id)
        );
        return 1;
    }
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    println!("{}", texts::pricing_model_removed(lang, provider_id, id));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn custom_model(id: &str) -> CustomModelDef {
        CustomModelDef {
            id: id.into(),
            display: format!("{id} 转售价"),
            peak: Some(PriceTier::full(1.0, 4.0, 2.0)),
            currency: Some("CNY".into()),
            ..Default::default()
        }
    }

    /// 契约：list 汇总——预置在前自定义在后，source/plan 字段正确，
    /// 无预置平台（siliconflow）仅有自定义行且币种走 default_currency。
    #[test]
    fn models_json_merges_preset_and_custom() {
        let list = models_json("deepseek", &[custom_model("flash")]).unwrap();
        assert_eq!(list.currency, "CNY");
        assert_eq!(list.default_model.as_deref(), Some("flash"));
        assert_eq!(list.models.len(), 4);
        assert_eq!(list.models[0].source, "preset");
        assert_eq!(list.models[3].source, "custom");
        assert_eq!(list.models[3].id, "flash");

        let list = models_json("siliconflow", &[custom_model("glm-5.2")]).unwrap();
        assert_eq!(list.default_model, None);
        assert_eq!(list.currency, "CNY");
        assert_eq!(list.models.len(), 1);
        assert_eq!(list.models[0].source, "custom");

        // 智谱订阅项：plan=subscription 且携带模型级窗口
        let list = models_json("zhipu", &[]).unwrap();
        let coding = list.models.iter().find(|m| m.id == "coding-plan").unwrap();
        assert_eq!(coding.plan, "subscription");
        assert_eq!(coding.windows.as_ref().map(Vec::len), Some(1));
        assert!(coding.peak.is_empty());

        // 未注册平台 → None
        assert!(models_json("no-such", &[]).is_none());
    }

    /// 契约：表格含模型 id、双语表头、来源与模式标签、紧凑三档价。
    #[test]
    fn table_renders_sources_and_prices() {
        let list = models_json("deepseek", &[custom_model("night-x")]).unwrap();
        for lang in [Lang::Zh, Lang::En] {
            let table = render_models_table(&list, lang);
            assert!(table.contains("night-x"), "{lang:?}: {table}");
            assert!(table.contains("0.1/3/9"), "{lang:?} 峰价紧凑串：{table}");
            assert!(
                table.contains(t(lang, T::PricingModelSourceCustom)),
                "{table}"
            );
        }
        let zh = render_models_table(&list, Lang::Zh);
        assert!(zh.contains(t(Lang::Zh, T::ColModel)), "{zh}");
    }

    /// 契约：添加同 id 覆盖（大小写不敏感）、不同 id 追加；
    /// 删除后空列表移除平台键。
    #[test]
    fn add_overrides_and_remove_cleans_key() {
        let mut cfg = AppConfig::default();
        apply_add(&mut cfg, "siliconflow", custom_model("glm-5.2"));
        apply_add(&mut cfg, "siliconflow", custom_model("GLM-5.2"));
        let list = &cfg.custom_models["siliconflow"];
        assert_eq!(list.len(), 1, "同 id 大小写不敏感应覆盖不追加");

        apply_add(&mut cfg, "siliconflow", custom_model("k3"));
        assert_eq!(cfg.custom_models["siliconflow"].len(), 2);

        assert!(apply_remove(&mut cfg, "siliconflow", "glm-5.2"));
        assert!(!apply_remove(&mut cfg, "siliconflow", "gone"));
        assert!(apply_remove(&mut cfg, "siliconflow", "k3"));
        assert!(
            !cfg.custom_models.contains_key("siliconflow"),
            "删空后应移除平台键"
        );
    }

    /// 契约：add 端到端——stdin JSON 校验入库、非法模型（跨日窗口）拦截。
    #[test]
    fn run_add_end_to_end() {
        let path =
            std::env::temp_dir().join(format!("quotatray-model-add-{}.json", std::process::id()));
        AppConfig::default().save(&path).unwrap();
        let ctx = Ctx::with_store(path.clone(), Arc::new(quota_core::InMemoryStore::new()));

        // 手动走 apply + save 链路（stdin 不便在测试注入）
        let mut cfg = AppConfig::load(&ctx.config_path).unwrap();
        apply_add(&mut cfg, "siliconflow", custom_model("glm-5.2"));
        cfg.save(&ctx.config_path).unwrap();
        let reloaded = AppConfig::load(&ctx.config_path).unwrap();
        assert_eq!(
            reloaded.custom_models["siliconflow"][0].display,
            "glm-5.2 转售价"
        );

        // 未知平台：run 层拦截
        assert_eq!(run_list(&ctx, "no-such", false), 1);
        assert_eq!(run_remove(&ctx, "no-such", "x"), 1);
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：add 的 stdin 解析错误链——非法 JSON、serde 缺必填字段、
    /// 校验失败（跨日窗口/空白 id）三类路径全拦截（双语前缀）。
    #[test]
    fn parse_add_input_error_paths() {
        let ok = parse_add_input(
            r#"{"id":"glm-5.5","display":"GLM-5.5","peak":{"cache_miss_input":2,"output":6}}"#,
            Lang::Zh,
        );
        assert!(ok.is_ok());

        let bad_json = parse_add_input("{ not json", Lang::Zh).unwrap_err();
        assert!(bad_json.contains("JSON 解析失败"), "{bad_json}");

        let missing_id = parse_add_input(r#"{"display":"缺 id"}"#, Lang::En).unwrap_err();
        assert!(
            missing_id.contains("JSON parse failed") && missing_id.contains("id"),
            "{missing_id}"
        );

        let cross_day = parse_add_input(
            r#"{"id":"x","display":"X","windows":[{"days":["mon"],"start":"22:00","end":"06:00"}]}"#,
            Lang::En,
        )
        .unwrap_err();
        assert!(
            cross_day.contains("validation failed") && cross_day.contains("windows[0].start/end"),
            "{cross_day}"
        );

        let blank_id = parse_add_input(r#"{"id":"  ","display":"X"}"#, Lang::En).unwrap_err();
        assert!(blank_id.contains("id"), "{blank_id}");
    }

    /// 契约：自定义模型含窗口时入库校验口径与 core 一致（合法窗口通过）。
    #[test]
    fn custom_model_with_windows_validates() {
        use quota_core::pricing::{PeakWindow, Weekday};
        let m = CustomModelDef {
            id: "night".into(),
            display: "夜间".into(),
            windows: Some(vec![PeakWindow {
                days: vec![Weekday::Mon],
                start: "09:00".into(),
                end: "12:00".into(),
            }]),
            ..custom_model("x")
        };
        assert!(pricing::validate_custom_model(&m).is_ok());
    }
}
