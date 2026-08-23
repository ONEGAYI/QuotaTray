//! `quota pricing`：峰谷定价查看 / 自定义 / 清除。
//!
//! 判定与合并逻辑在 `quota_core::pricing`，本模块只做编排（配置读写、
//! stdin 解析）与展示；`render_show` / `show_json` / `apply_*` 为纯函数，
//! now 经参数注入（测试不碰真实时钟）。

use quota_core::AppConfig;
use quota_core::pricing::{
    self, PeakKind, PeakWindow, PriceTier, PricingConfig, PricingSource, ResolvedPricing,
};
use serde::Serialize;

use crate::ctx::Ctx;
use crate::lang::Lang;
use crate::render;
use crate::settings_io;
use crate::texts::{self, T, t};

/// 峰谷标签文案。
pub fn kind_label(lang: Lang, kind: PeakKind) -> &'static str {
    match kind {
        PeakKind::Peak => t(lang, T::PeakLabel),
        PeakKind::OffPeak => t(lang, T::OffPeakLabel),
    }
}

/// `pricing show --json` 的输出结构（生效定价 + 当前判定 + 下次切换）。
#[derive(Serialize)]
pub struct PricingShowJson {
    pub id: String,
    pub name: String,
    /// "peak" | "off_peak"
    pub kind: &'static str,
    /// "pay_as_you_go" | "subscription"（订阅项价格档为 null）。
    pub plan: &'static str,
    /// "preset" | "custom"
    pub source: &'static str,
    /// source=preset 时的来源定位。
    pub preset: Option<PresetInfoJson>,
    pub model_label: Option<String>,
    pub currency: Option<String>,
    /// null = 本地时区。
    pub timezone_offset_minutes: Option<i32>,
    pub windows: Vec<PeakWindow>,
    pub peak: Option<PriceTier>,
    pub off_peak: Option<PriceTier>,
    pub next_change: Option<NextChangeJson>,
}

#[derive(Serialize)]
pub struct PresetInfoJson {
    pub native_id: String,
    pub model: String,
}

#[derive(Serialize)]
pub struct NextChangeJson {
    pub at_ms: u64,
    /// 翻转后类型 "peak" | "off_peak"。
    pub kind: &'static str,
}

fn kind_str(kind: PeakKind) -> &'static str {
    match kind {
        PeakKind::Peak => "peak",
        PeakKind::OffPeak => "off_peak",
    }
}

fn plan_str(plan: quota_core::PlanKind) -> &'static str {
    match plan {
        quota_core::PlanKind::PayAsYouGo => "pay_as_you_go",
        quota_core::PlanKind::Subscription => "subscription",
    }
}

/// 组装 JSON 输出（纯函数）。
pub fn show_json(id: &str, name: &str, resolved: &ResolvedPricing, now_ms: u64) -> PricingShowJson {
    let (source, preset) = match &resolved.source {
        PricingSource::Preset { native_id, model } => (
            "preset",
            Some(PresetInfoJson {
                native_id: native_id.clone(),
                model: model.clone(),
            }),
        ),
        PricingSource::Custom => ("custom", None),
    };
    PricingShowJson {
        id: id.into(),
        name: name.into(),
        kind: kind_str(resolved.kind(now_ms)),
        plan: plan_str(resolved.plan),
        source,
        preset,
        model_label: resolved.model_label.clone(),
        currency: resolved.currency.clone(),
        timezone_offset_minutes: resolved.timezone_offset_minutes,
        windows: resolved.windows.clone(),
        peak: resolved.peak.clone(),
        off_peak: resolved.off_peak.clone(),
        next_change: pricing::next_change(
            &resolved.windows,
            resolved.timezone_offset_minutes,
            now_ms,
        )
        .map(|(at_ms, kind)| NextChangeJson {
            at_ms,
            kind: kind_str(kind),
        }),
    }
}

/// 表格模式输出（纯函数）：头部行 + 来源 + 价格对照 + 时段 + 下次切换。
pub fn render_show(
    id: &str,
    name: &str,
    resolved: &ResolvedPricing,
    now_ms: u64,
    lang: Lang,
) -> String {
    let kind = kind_label(lang, resolved.kind(now_ms));
    let unit = resolved.currency.as_ref().map(|c| format!("{c}/MTokens"));
    let header = texts::pricing_header(
        lang,
        name,
        id,
        kind,
        resolved.model_label.as_deref(),
        unit.as_deref(),
    );
    let preset_info = match &resolved.source {
        PricingSource::Preset { native_id, model } => Some((native_id.as_str(), model.as_str())),
        PricingSource::Custom => None,
    };
    let source_line = texts::pricing_source(lang, preset_info);
    let table = render::pricing_table(resolved.peak.as_ref(), resolved.off_peak.as_ref(), lang);
    let windows_line = if resolved.windows.is_empty() {
        t(lang, T::PricingNoWindows).to_string()
    } else {
        texts::pricing_windows_line(
            lang,
            &render::tz_desc(lang, resolved.timezone_offset_minutes),
            &render::windows_desc(&resolved.windows, lang),
        )
    };
    let mut lines = vec![header, source_line, table, windows_line];
    if resolved.plan == quota_core::PlanKind::Subscription {
        lines.push(t(lang, T::PricingPlanNote).to_string());
    }
    if let Some((at, next_kind)) =
        pricing::next_change(&resolved.windows, resolved.timezone_offset_minutes, now_ms)
    {
        lines.push(texts::pricing_next_change(
            lang,
            &render::fmt_datetime_in_tz(at, resolved.timezone_offset_minutes),
            kind_label(lang, next_kind),
        ));
    }
    lines.join("\n")
}

/// `pricing show`：条目不存在 → 1；无定价 → 提示后 0（查看类，非错误）。
/// 预置选套带币种 hint：条目 `pricing.currency`（DeepSeek 单站双币时
/// 数字与标签一起切到 USD 套）；自定义模型库同链生效。
pub fn run_show(ctx: &Ctx, id: &str, json: bool) -> i32 {
    let lang = ctx.lang;
    let cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    let Some(entry) = cfg.providers.iter().find(|e| e.id == id) else {
        eprintln!("{}{}", t(lang, T::Err), texts::entry_not_found(lang, id));
        return 1;
    };
    let hint = entry.pricing.as_ref().and_then(|p| p.currency.as_deref());
    let Some(resolved) = pricing::resolve_in_currency(entry, &cfg.custom_models, hint) else {
        println!("{}", t(lang, T::PricingNotConfigured));
        return 0;
    };
    let now = settings_io::now_ms();
    if json {
        match serde_json::to_string_pretty(&show_json(&entry.id, &entry.name, &resolved, now)) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("{}{e}", t(lang, T::Err));
                return 1;
            }
        }
    } else {
        println!(
            "{}",
            render_show(&entry.id, &entry.name, &resolved, now, lang)
        );
    }
    0
}

/// 解析 `pricing set` 的 stdin JSON 并静态校验（纯函数）。
pub fn parse_pricing_json(text: &str, lang: Lang) -> Result<PricingConfig, String> {
    let cfg: PricingConfig =
        serde_json::from_str(text).map_err(|e| format!("{}{e}", t(lang, T::JsonParseFail)))?;
    pricing::validate(&cfg).map_err(|e| format!("{}{e}", t(lang, T::PricingValidateFail)))?;
    Ok(cfg)
}

/// 把自定义定价写入配置中指定条目（纯函数；条目不存在报错）。
pub fn apply_set(cfg: &mut AppConfig, id: &str, pricing: PricingConfig) -> Result<(), String> {
    let Some(entry) = cfg.providers.iter_mut().find(|e| e.id == id) else {
        return Err("entry-not-found".into());
    };
    entry.pricing = Some(pricing);
    Ok(())
}

/// 清除条目的自定义定价（纯函数；条目不存在报错）。
pub fn apply_clear(cfg: &mut AppConfig, id: &str) -> Result<(), String> {
    let Some(entry) = cfg.providers.iter_mut().find(|e| e.id == id) else {
        return Err("entry-not-found".into());
    };
    entry.pricing = None;
    Ok(())
}

/// `pricing set`：stdin 读 PricingConfig JSON → 校验 → 写入。
pub fn run_set(ctx: &Ctx, id: &str) -> i32 {
    let lang = ctx.lang;
    let mut text = String::new();
    if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut text) {
        eprintln!("{}{}{e}", t(lang, T::Err), t(lang, T::StdinReadFail));
        return 1;
    }
    let pricing_cfg = match parse_pricing_json(&text, lang) {
        Ok(c) => c,
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
    if let Err(msg) = apply_set(&mut cfg, id, pricing_cfg) {
        if msg == "entry-not-found" {
            eprintln!("{}{}", t(lang, T::Err), texts::entry_not_found(lang, id));
        } else {
            eprintln!("{}{msg}", t(lang, T::Err));
        }
        return 1;
    }
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    println!("{}", texts::pricing_saved(lang, id));
    0
}

/// `pricing clear`：清除自定义（回退预置）。
pub fn run_clear(ctx: &Ctx, id: &str) -> i32 {
    let lang = ctx.lang;
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    if let Err(msg) = apply_clear(&mut cfg, id) {
        if msg == "entry-not-found" {
            eprintln!("{}{}", t(lang, T::Err), texts::entry_not_found(lang, id));
        } else {
            eprintln!("{}{msg}", t(lang, T::Err));
        }
        return 1;
    }
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    println!("{}", texts::pricing_cleared(lang, id));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use quota_core::config::ProviderKind;
    use quota_core::pricing::Weekday;
    use quota_core::template::TemplateRequest;
    use quota_core::{ProviderEntry, TemplateConfig};
    use std::sync::Arc;

    /// 北京时间 2026-08-19（周三）09:30 —— DeepSeek 高峰内（core 测试同锚点）。
    const PEAK_NOW_MS: u64 = 1_787_103_000_000;

    fn deepseek_entry() -> ProviderEntry {
        ProviderEntry {
            id: "p1".into(),
            name: "DeepSeek".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
        }
    }

    fn template_entry() -> ProviderEntry {
        ProviderEntry {
            id: "t1".into(),
            name: "自建".into(),
            kind: ProviderKind::Template(Box::new(TemplateConfig {
                request: TemplateRequest {
                    method: Default::default(),
                    url: "https://x/api".into(),
                    headers: Default::default(),
                    body: None,
                },
                extract: Default::default(),
                transforms: vec![],
                windows_from: None,
                windows: vec![],
                allow_insecure: false,
            })),
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
        }
    }

    /// 契约：表格输出含峰谷标签、模型、三档价格与时段聚合（双语）。
    #[test]
    fn render_show_deepseek_preset() {
        let entry = deepseek_entry();
        let resolved = pricing::resolve(&entry).unwrap();
        for (lang, peak_label, not_next) in [
            (Lang::Zh, "⚡高峰", "下次切换"),
            (Lang::En, "Peak", "Next change"),
        ] {
            let out = render_show(&entry.id, &entry.name, &resolved, PEAK_NOW_MS, lang);
            assert!(out.contains(peak_label), "{lang:?}: {out}");
            assert!(out.contains("V4 Flash"), "{lang:?}: {out}");
            assert!(out.contains("CNY/MTokens"), "{lang:?}: {out}");
            // flash 价格（去尾零格式）
            assert!(out.contains("0.1"), "{lang:?}: {out}");
            assert!(out.contains("0.05"), "{lang:?}: {out}");
            assert!(out.contains("9"), "{lang:?}: {out}");
            assert!(out.contains("4.5"), "{lang:?}: {out}");
            assert!(out.contains(not_next), "{lang:?} 应含下次切换：{out}");
        }
        // 中文时段聚合与偏移
        let zh = render_show(&entry.id, &entry.name, &resolved, PEAK_NOW_MS, Lang::Zh);
        assert!(zh.contains("周一至周五 09:00–12:00"), "{zh}");
        assert!(zh.contains("UTC+08:00"), "{zh}");
        // 来源行
        assert!(zh.contains("定价来源：预置（deepseek · flash）"), "{zh}");
    }

    /// 契约：JSON 输出形状——kind/source/preset/next_change 关键字段。
    #[test]
    fn show_json_shape() {
        let entry = deepseek_entry();
        let resolved = pricing::resolve(&entry).unwrap();
        let j = serde_json::to_value(show_json(&entry.id, &entry.name, &resolved, PEAK_NOW_MS))
            .unwrap();
        assert_eq!(j["kind"], "peak");
        assert_eq!(j["source"], "preset");
        assert_eq!(j["preset"]["native_id"], "deepseek");
        assert_eq!(j["preset"]["model"], "flash");
        assert_eq!(j["model_label"], "V4 Flash");
        assert_eq!(j["currency"], "CNY");
        assert_eq!(j["timezone_offset_minutes"], 480);
        assert_eq!(j["windows"].as_array().unwrap().len(), 2);
        assert_eq!(j["next_change"]["kind"], "off_peak");
        assert!(j["next_change"]["at_ms"].is_u64());
        // 峰谷价格档齐全
        assert_eq!(j["peak"]["cache_hit_input"], 0.1);
        assert_eq!(j["off_peak"]["output"], 4.5);
    }

    /// 契约：model 选择切档后 source 仍为 preset、价格随之切换。
    #[test]
    fn show_json_with_model_selection() {
        let mut entry = deepseek_entry();
        entry.pricing = Some(PricingConfig {
            model: Some("pro".into()),
            ..Default::default()
        });
        let resolved = pricing::resolve(&entry).unwrap();
        let j = serde_json::to_value(show_json(&entry.id, &entry.name, &resolved, PEAK_NOW_MS))
            .unwrap();
        assert_eq!(j["source"], "preset");
        assert_eq!(j["preset"]["model"], "pro");
        assert_eq!(j["model_label"], "V4 Pro");
        assert_eq!(j["peak"]["cache_hit_input"], 0.3);
    }

    /// 契约：set 的 stdin 解析——合法 JSON 通过、非法 JSON 与校验失败拦截（双语前缀）。
    #[test]
    fn parse_pricing_json_valid_and_invalid() {
        let ok = parse_pricing_json(
            r#"{"model":"pro","windows":[{"days":["mon"],"start":"09:00","end":"12:00"}]}"#,
            Lang::Zh,
        );
        assert!(ok.is_ok());

        let bad_json = parse_pricing_json("{ not json", Lang::Zh).unwrap_err();
        assert!(bad_json.contains("JSON 解析失败"), "{bad_json}");

        let cross_day = parse_pricing_json(
            r#"{"windows":[{"days":["mon"],"start":"22:00","end":"06:00"}]}"#,
            Lang::En,
        )
        .unwrap_err();
        assert!(cross_day.contains("validation failed"), "{cross_day}");
        assert!(cross_day.contains("windows[0].start/end"), "{cross_day}");
    }

    /// 契约：set/clear 端到端——写入自定义、resolve 生效 Custom、清除回退预置。
    #[test]
    fn set_and_clear_roundtrip() {
        let path = std::env::temp_dir().join(format!(
            "quotatray-pricing-test-{}.json",
            std::process::id()
        ));
        let cfg = AppConfig {
            custom_models: Default::default(),
            providers: vec![deepseek_entry()],
        };
        cfg.save(&path).unwrap();

        let ctx = Ctx::with_store(path.clone(), Arc::new(quota_core::InMemoryStore::new()));
        // set：仅自定义 currency（其余回退预置）
        let custom = PricingConfig {
            currency: Some("USD".into()),
            ..Default::default()
        };
        let mut loaded = AppConfig::load(&ctx.config_path).unwrap();
        apply_set(&mut loaded, "p1", custom).unwrap();
        loaded.save(&ctx.config_path).unwrap();

        let reloaded = AppConfig::load(&ctx.config_path).unwrap();
        let resolved = pricing::resolve(&reloaded.providers[0]).unwrap();
        assert_eq!(resolved.source, PricingSource::Custom);
        assert_eq!(resolved.currency.as_deref(), Some("USD"));
        assert_eq!(
            resolved.peak.as_ref().unwrap().cache_hit_input,
            Some(0.1),
            "价格回退预置"
        );

        // clear：回退预置 flash
        let mut loaded = AppConfig::load(&ctx.config_path).unwrap();
        apply_clear(&mut loaded, "p1").unwrap();
        loaded.save(&ctx.config_path).unwrap();
        let reloaded = AppConfig::load(&ctx.config_path).unwrap();
        assert!(reloaded.providers[0].pricing.is_none());

        // 条目不存在
        let mut loaded = AppConfig::load(&ctx.config_path).unwrap();
        assert!(apply_clear(&mut loaded, "nope").is_err());
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：自定义条目（template）显示来源为自定义、时段聚合。
    #[test]
    fn render_show_custom_template_entry() {
        let mut entry = template_entry();
        entry.pricing = Some(PricingConfig {
            timezone_offset_minutes: Some(480),
            windows: Some(vec![PeakWindow {
                days: vec![Weekday::Sat, Weekday::Sun, Weekday::Mon],
                start: "00:00".into(),
                end: "08:00".into(),
            }]),
            peak: Some(PriceTier::full(1.0, 2.0, 3.0)),
            off_peak: Some(PriceTier::full(0.5, 1.0, 1.5)),
            currency: Some("USD".into()),
            ..Default::default()
        });
        let resolved = pricing::resolve(&entry).unwrap();
        let zh = render_show(&entry.id, &entry.name, &resolved, PEAK_NOW_MS, Lang::Zh);
        assert!(zh.contains("定价来源：自定义"), "{zh}");
        // 周六+周日+周一 → 周一、周六至周日
        assert!(zh.contains("周一、周六至周日 00:00–08:00"), "{zh}");
        assert!(zh.contains("USD/MTokens"), "{zh}");
        assert!(zh.contains("1"), "{zh}");
    }

    /// 契约：show 接线——自定义模型库生效（手编 config 即可用）、
    /// 条目 currency 作为币种 hint 切 DeepSeek USD 数字套、
    /// JSON 透出 plan 字段。
    #[test]
    fn show_wires_library_and_currency() {
        use quota_core::pricing::CustomModelDef;
        use std::sync::Arc;

        let path = std::env::temp_dir().join(format!(
            "quotatray-pricing-wire-{}.json",
            std::process::id()
        ));
        let mut cfg = AppConfig {
            custom_models: {
                let mut m = std::collections::BTreeMap::new();
                m.insert(
                    "deepseek".into(),
                    vec![CustomModelDef {
                        id: "flash".into(),
                        display: "V4 Flash（自算）".into(),
                        peak: Some(PriceTier::full(0.11, 3.1, 9.1)),
                        ..Default::default()
                    }],
                );
                m
            },
            providers: vec![deepseek_entry()],
        };
        // 条目选库模型 + currency USD：峰价应同时来自库（0.11）与 USD 套无关
        cfg.providers[0].pricing = Some(PricingConfig {
            model: Some("flash".into()),
            ..Default::default()
        });
        cfg.save(&path).unwrap();
        let ctx = Ctx::with_store(path.clone(), Arc::new(quota_core::InMemoryStore::new()));
        let loaded = AppConfig::load(&ctx.config_path).unwrap();
        let entry = &loaded.providers[0];
        let hint = entry.pricing.as_ref().and_then(|p| p.currency.as_deref());
        let resolved = pricing::resolve_in_currency(entry, &loaded.custom_models, hint).unwrap();
        let j = serde_json::to_value(show_json(&entry.id, &entry.name, &resolved, PEAK_NOW_MS))
            .unwrap();
        assert_eq!(j["model_label"], "V4 Flash（自算）");
        assert_eq!(j["peak"]["cache_hit_input"], 0.11, "库模型价格生效");
        assert_eq!(j["plan"], "pay_as_you_go");

        // USD hint（条目仅设 currency）：数字与标签一起切 USD 套
        let mut cfg2 = AppConfig {
            custom_models: Default::default(),
            providers: vec![deepseek_entry()],
        };
        cfg2.providers[0].pricing = Some(PricingConfig {
            currency: Some("USD".into()),
            ..Default::default()
        });
        let resolved =
            pricing::resolve_in_currency(&cfg2.providers[0], &Default::default(), Some("USD"))
                .unwrap();
        assert_eq!(resolved.currency.as_deref(), Some("USD"));
        assert_eq!(resolved.peak.as_ref().unwrap().cache_hit_input, Some(0.014));
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：订阅项展示——JSON plan=subscription、价格档 null、
    /// 表格输出订阅说明行。
    #[test]
    fn show_renders_subscription_plan() {
        let entry = ProviderEntry {
            id: "z1".into(),
            name: "智谱".into(),
            kind: ProviderKind::Native {
                provider: "zhipu".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: Some(PricingConfig {
                model: Some("coding-plan".into()),
                ..Default::default()
            }),
        };
        let resolved = pricing::resolve(&entry).unwrap();
        let j = serde_json::to_value(show_json(&entry.id, &entry.name, &resolved, PEAK_NOW_MS))
            .unwrap();
        assert_eq!(j["plan"], "subscription");
        assert!(j["peak"].is_null() && j["off_peak"].is_null());
        let zh = render_show(&entry.id, &entry.name, &resolved, PEAK_NOW_MS, Lang::Zh);
        assert!(zh.contains("订阅积分制"), "{zh}");
    }

    /// 契约：show 对无定价条目（无预置 native）返回 0 并走未配置提示。
    #[test]
    fn run_show_without_pricing_is_zero_exit() {
        let path = std::env::temp_dir().join(format!(
            "quotatray-pricing-none-{}.json",
            std::process::id()
        ));
        let entry = quota_core::ProviderEntry {
            id: "s1".into(),
            name: "SF".into(),
            kind: ProviderKind::Native {
                provider: "siliconflow".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
        };
        AppConfig {
            custom_models: Default::default(),
            providers: vec![entry],
        }
        .save(&path)
        .unwrap();
        let ctx = Ctx::with_store(path.clone(), Arc::new(quota_core::InMemoryStore::new()));
        assert_eq!(run_show(&ctx, "s1", false), 0);
        assert_eq!(run_show(&ctx, "missing", false), 1);
        let _ = std::fs::remove_file(&path);
    }
}
