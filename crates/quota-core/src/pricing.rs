//! 峰谷定价：按「周几 + 时间段」判定峰/谷，并给出三档价格
//! （输入·缓存命中 / 输入·缓存未命中 / 输出，单位：每 MTokens）。
//!
//! 时间入参一律 epoch 毫秒（纯函数，与 `update` 同约定，测试不碰真实时钟）。
//! 时区：`None` = 本地时区；`Some(分钟)` = 固定 UTC 偏移（预置 DeepSeek 为
//! UTC+8 = 480）。峰谷判定不参与查询链路，纯展示侧功能。

use chrono::{Datelike, TimeZone, Timelike};
use serde::{Deserialize, Serialize};

use crate::config::{ProviderEntry, ProviderKind};

/// 星期（serde lowercase：`mon`…`sun`，与 chrono 星期同序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    /// 转换为 chrono 星期（判定用）。
    fn to_chrono(self) -> chrono::Weekday {
        match self {
            Weekday::Mon => chrono::Weekday::Mon,
            Weekday::Tue => chrono::Weekday::Tue,
            Weekday::Wed => chrono::Weekday::Wed,
            Weekday::Thu => chrono::Weekday::Thu,
            Weekday::Fri => chrono::Weekday::Fri,
            Weekday::Sat => chrono::Weekday::Sat,
            Weekday::Sun => chrono::Weekday::Sun,
        }
    }
}

/// 一档价格。单位：每 MTokens；字段可部分缺失（缺失侧展示 "—"）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceTier {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_hit_input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_miss_input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
}

impl PriceTier {
    /// 三价齐全（预置数据构造与回退判定用）。
    pub fn full(cache_hit_input: f64, cache_miss_input: f64, output: f64) -> Self {
        Self {
            cache_hit_input: Some(cache_hit_input),
            cache_miss_input: Some(cache_miss_input),
            output: Some(output),
        }
    }

    /// 全字段缺失——视为「未提供」（resolve 回退判定与 CLI 列表占位共用）。
    pub fn is_empty(&self) -> bool {
        self.cache_hit_input.is_none() && self.cache_miss_input.is_none() && self.output.is_none()
    }
}

/// 高峰时段窗口：`days` 上每天的 `[start, end)`（左闭右开，同日不跨日）。
/// `end` 额外接受 `"24:00"`（当日结束，用于表达全天窗口）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeakWindow {
    #[serde(default)]
    pub days: Vec<Weekday>,
    /// "HH:MM"（24 小时制）。
    pub start: String,
    /// "HH:MM"（24 小时制；或 "24:00" 表示到当日结束）。
    pub end: String,
}

/// 用户自定义峰谷定价（挂在 [`ProviderEntry::pricing`] 上）。
///
/// 字段级回退预置：`windows` 缺失 = 回退预置时段；显式空数组或无预置 =
/// 恒空闲（允许只配价格不分峰谷）。价格档全字段缺失同样回退预置。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PricingConfig {
    /// 预置模型选择（匹配预置模型 id，大小写不敏感）或自定义展示标签。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// UTC 偏移（分钟，东八区 = 480）；None = 本地时区。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone_offset_minutes: Option<i32>,
    /// 高峰窗口集合；None = 回退预置，Some([]) = 恒空闲。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<PeakWindow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak: Option<PriceTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_peak: Option<PriceTier>,
    /// 计价币种（如 "CNY"；自由字符串）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

/// 峰谷判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeakKind {
    /// 高峰时段。
    Peak,
    /// 空闲（低谷）时段。
    OffPeak,
}

// ---- 判定（纯函数，epoch ms 入参） -----------------------------------------

/// 峰谷判定：命中任一窗口即高峰（左闭右开）；无窗口恒空闲。
/// 非法时刻串的窗口不参与判定（写入侧 `validate` 已拦截，此处防御）。
pub fn classify(
    windows: &[PeakWindow],
    timezone_offset_minutes: Option<i32>,
    now_ms: u64,
) -> PeakKind {
    let (weekday, minutes) = weekday_minutes(now_ms, timezone_offset_minutes);
    for w in windows {
        let (Some((sh, sm)), Some((eh, em))) = (parse_hhmm(&w.start), parse_hhmm(&w.end)) else {
            continue;
        };
        if !w.days.iter().any(|d| d.to_chrono() == weekday) {
            continue;
        }
        let start = sh as u32 * 60 + sm as u32;
        let end = eh as u32 * 60 + em as u32;
        if start <= minutes && minutes < end {
            return PeakKind::Peak;
        }
    }
    PeakKind::OffPeak
}

/// 下一次峰谷翻转：`(时刻 epoch ms, 翻转后类型)`。
/// 自当前起的下一整分钟向后扫 7 天（分钟粒度）；无窗口或 7 天内不翻转 → None。
pub fn next_change(
    windows: &[PeakWindow],
    timezone_offset_minutes: Option<i32>,
    now_ms: u64,
) -> Option<(u64, PeakKind)> {
    if windows.is_empty() {
        return None;
    }
    let current = classify(windows, timezone_offset_minutes, now_ms);
    let mut t = now_ms - now_ms % 60_000;
    for _ in 0..7 * 24 * 60 {
        // u64 极值附近溢出视为「找不到翻转」（理论路径，checked 保不 panic）
        t = t.checked_add(60_000)?;
        let kind = classify(windows, timezone_offset_minutes, t);
        if kind != current {
            return Some((t, kind));
        }
    }
    None
}

/// epoch 毫秒 → (时区星期, 当日分钟数)。tz None = 本地；非法偏移/超范围
/// 兜底仍基于**入参时刻**的本地时区（保持纯函数——`next_change` 的扫描
/// 依赖同一入参的可重判定），仅入参超出 chrono 有效范围才按当前时间兜底
/// （与 `update::local_datetime` 同策略，不 panic）。
fn weekday_minutes(now_ms: u64, tz: Option<i32>) -> (chrono::Weekday, u32) {
    fn parts<Tz: chrono::TimeZone>(dt: chrono::DateTime<Tz>) -> (chrono::Weekday, u32) {
        (dt.weekday(), dt.hour() * 60 + dt.minute())
    }
    if let Some(minutes) = tz {
        if let Some(offset) = minutes
            .checked_mul(60)
            .and_then(chrono::FixedOffset::east_opt)
        {
            if let Some(dt) = offset.timestamp_millis_opt(now_ms as i64).single() {
                return parts(dt);
            }
        }
    } else if let Some(dt) = chrono::Local.timestamp_millis_opt(now_ms as i64).single() {
        return parts(dt);
    }
    // 兜底：入参时刻的本地时区（非法偏移 / Local 转换失败时仍基于**入参**，
    // 保持纯函数——`next_change` 的扫描依赖同一入参的可重判定）；入参超出
    // chrono 有效范围时取 epoch 0（1970-01-01 周四 00:00 UTC）这个确定性
    // 值，不读真实时钟。
    chrono::Local
        .timestamp_millis_opt(now_ms as i64)
        .single()
        .map(parts)
        .unwrap_or((chrono::Weekday::Thu, 0))
}

/// "HH:MM" 解析（复用 `update::parse_hhmm`，24 小时制含边界）；非法 → None。
/// 峰谷特例：接受 `"24:00"`（仅作 end 的全天上界——classify 按 `[start, end)`
/// 分钟比较，1440 与「当日任何分钟」天然兼容；start 处 24:00 会被
/// validate 的 start<end 检查拒绝）。
fn parse_hhmm(s: &str) -> Option<(u8, u8)> {
    if s.trim() == "24:00" {
        Some((24, 0))
    } else {
        crate::update::parse_hhmm(s)
    }
}

// ---- 格式化 ----------------------------------------------------------------

/// 价格展示：最多 2 位小数去尾零（`0.30`→`0.3`、`27.00`→`27`）；
/// 小于 0.05 的非零价保留 4 位——美元档峰谷命中价（谷 0.007 / 峰 0.014）
/// 若按 2 位舍入会同显 "0.01"，两档撞车（阈值取 0.05 而非 0.01：
/// 0.014 这类值 2 位舍入同样损失信息）。
/// 非有限值（NaN/∞，未经 validate 的数据）显示 "—"。
pub fn format_price(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    if v == 0.0 {
        return "0".into(); // IEEE 相等覆盖 ±0（-0.0 不显示成 "-0"）
    }
    let s = if v.abs() < 0.05 {
        format!("{v:.4}")
    } else {
        format!("{v:.2}")
    };
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() { "0".into() } else { s.into() }
}

// ---- 校验 ------------------------------------------------------------------

/// 峰谷配置校验错误（保存时拦截，带字段定位；两端展示模式与 TemplateError 一致）。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PricingError {
    #[error("字段 {field}：{reason}")]
    Validation { field: String, reason: String },
}

/// 校验自定义峰谷配置：时刻格式、同日不跨日、days 非空、价格有限非负、
/// UTC 偏移 ±14h 内。预置数据不走此校验（随版本内置，正确性由测试锁定）。
pub fn validate(cfg: &PricingConfig) -> Result<(), PricingError> {
    let fail = |field: &str, reason: String| {
        Err(PricingError::Validation {
            field: field.into(),
            reason,
        })
    };
    if let Some(minutes) = cfg.timezone_offset_minutes {
        if minutes.unsigned_abs() > 14 * 60 {
            return fail(
                "timezone_offset_minutes",
                format!("UTC 偏移须在 ±{0} 分钟内，当前 {1}", 14 * 60, minutes),
            );
        }
    }
    if let Some(windows) = cfg.windows.as_ref() {
        for (i, w) in windows.iter().enumerate() {
            let field = |name: &str| format!("windows[{i}].{name}");
            if w.days.is_empty() {
                return fail(&field("days"), "至少选择一个星期".into());
            }
            let (start, end) = match (parse_hhmm(&w.start), parse_hhmm(&w.end)) {
                (Some(s), Some(e)) => (s, e),
                _ => {
                    return fail(
                        &field("start/end"),
                        format!("时刻须为 HH:MM，当前 {} → {}", w.start, w.end),
                    );
                }
            };
            if start >= end {
                return fail(
                    &field("start/end"),
                    format!("开始须早于结束且不跨日，当前 {} → {}", w.start, w.end),
                );
            }
        }
    }
    for (name, tier) in [
        ("peak", cfg.peak.as_ref()),
        ("off_peak", cfg.off_peak.as_ref()),
    ] {
        let Some(tier) = tier else { continue };
        for (price_name, value) in [
            ("cache_hit_input", &tier.cache_hit_input),
            ("cache_miss_input", &tier.cache_miss_input),
            ("output", &tier.output),
        ] {
            if let Some(v) = value
                && !(v.is_finite() && *v >= 0.0)
            {
                return fail(
                    &format!("{name}.{price_name}"),
                    format!("价格须为非负有限数，当前 {v}"),
                );
            }
        }
    }
    Ok(())
}

/// 校验自定义模型定义：id/display 非空白；窗口/时区/价格档语义复用
/// [`validate`]（空 windows 数组 = 恒空闲，合法）；currency 自由字符串不校验。
pub fn validate_custom_model(m: &CustomModelDef) -> Result<(), PricingError> {
    if m.id.trim().is_empty() {
        return Err(PricingError::Validation {
            field: "id".into(),
            reason: "模型 id 不能为空".into(),
        });
    }
    if m.display.trim().is_empty() {
        return Err(PricingError::Validation {
            field: "display".into(),
            reason: "展示名不能为空".into(),
        });
    }
    validate(&PricingConfig {
        model: None,
        timezone_offset_minutes: m.timezone_offset_minutes,
        windows: m.windows.clone(),
        peak: m.peak.clone(),
        off_peak: m.off_peak.clone(),
        currency: None,
    })
}

// ---- 预置（官方定价，随版本内置；数据以官网为准） ---------------------------

/// 计费模式：决定展示语义——按量为「每 MTokens 三档价」，
/// 订阅为「积分/额度倍率」（价格档留空，峰谷窗口表达折扣时段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    /// 按量付费（pay-as-you-go），三档价格有效。
    PayAsYouGo,
    /// 订阅套餐（如 GLM Coding Plan），积分制、无每 token 价。
    Subscription,
}

/// 预置单模型价格档。
#[derive(Debug, Clone, PartialEq)]
pub struct PresetModel {
    /// 模型 id（自定义配置的 `model` 匹配项，如 "flash"）。
    pub id: &'static str,
    /// 展示名（如 "V4 Flash"）。
    pub display: &'static str,
    /// 计费模式（订阅项价格档留空、窗口表达折扣时段）。
    pub plan: PlanKind,
    /// 模型级峰谷窗口覆盖：None = 继承平台级；Some(vec![]) = 该模型恒空闲。
    /// 订阅项在此携带自己的折扣窗口（如 Coding Plan 工作日 14:00–18:00），
    /// 同平台按量模型则继承平台级（无峰谷平台即恒空闲）。
    pub windows: Option<Vec<PeakWindow>>,
    pub peak: PriceTier,
    pub off_peak: PriceTier,
}

/// 预置平台峰谷定价（owned：调用频率低，随取随构）。
#[derive(Debug, Clone, PartialEq)]
pub struct PresetProvider {
    pub native_id: &'static str,
    pub currency: &'static str,
    /// UTC 偏移（分钟）。
    pub timezone_offset_minutes: i32,
    /// 平台级高峰窗口（模型级 `windows` 为 None 时生效）。
    pub windows: Vec<PeakWindow>,
    pub models: Vec<PresetModel>,
    pub default_model: &'static str,
}

/// 平台的默认预置币种（条目未指定时 `preset` 取这套）。
/// OpenRouter 虽无峰谷预置但按量计价为 USD，一并给出以防误兜底。
pub fn default_currency(native_id: &str) -> &'static str {
    match native_id {
        "kimi_global" | "kimi_code_global" | "zai" | "zai_api" | "siliconflow_global"
        | "openrouter" | "novita" | "minimax_global" => "USD",
        _ => "CNY",
    }
}

/// 按币种取预置套：DeepSeek 单站双币（余额 API `currency` 字段区分账户），
/// 其余平台忽略 `currency` 返回唯一套。无预置 → None。
pub fn preset_with_currency(native_id: &str, currency: &str) -> Option<PresetProvider> {
    match native_id {
        "deepseek" if currency.eq_ignore_ascii_case("USD") => Some(deepseek_preset("USD")),
        "deepseek" => Some(deepseek_preset("CNY")),
        _ => preset(native_id),
    }
}

/// 按 native id 取预置峰谷定价；无预置 → None。
///
/// 数据抓取自各官网定价页（2026-08-23，中英文页交叉验证）：
/// - DeepSeek：https://api-docs.deepseek.com/zh-cn/quick_start/pricing/ （CNY）
///   与英文页（USD）。高峰 = 北京时间周一至周五 09:00–12:00、14:00–18:00，
///   空闲价为高峰一半。
/// - Kimi：https://platform.kimi.com/docs/pricing/chat （CNY）/
///   platform.kimi.ai（USD），无峰谷（恒空闲，两档同价）。
/// - 智谱/Z.ai：open.bigmodel.cn/pricing（CNY，SPA 实抓）/ docs.z.ai
///   （USD）。按量无峰谷；Coding Plan 订阅积分制高峰 = 工作日 14:00–18:00，
///   其余时段（含周末全天）积分消耗更低——倍率口径以官网权益说明为准
///   （Z.ai 为闲时 0.5×，智谱国内站倍率曾调整过，两站均非峰谷时段本身）。
pub fn preset(native_id: &str) -> Option<PresetProvider> {
    match native_id {
        "deepseek" => Some(deepseek_preset("CNY")),
        "kimi_cn" => Some(kimi_preset(
            "kimi_cn",
            "CNY",
            &[
                ("k3", "Kimi K3", 2.0, 20.0, 100.0),
                ("k27-code", "Kimi K2.7 Code", 1.3, 6.5, 27.0),
            ],
            "k3",
        )),
        "kimi_global" => Some(kimi_preset(
            "kimi_global",
            "USD",
            &[
                ("k3", "Kimi K3", 0.30, 3.00, 15.00),
                ("k26", "Kimi K2.6", 0.16, 0.95, 4.00),
            ],
            "k3",
        )),
        "kimi_code_cn" => Some(kimi_code_preset("kimi_code_cn", "CNY")),
        "kimi_code_global" => Some(kimi_code_preset("kimi_code_global", "USD")),
        "zhipu_api" => Some(zhipu_payg_preset(
            "zhipu_api",
            "CNY",
            &[
                ("glm-5.3", "GLM-5.3", 2.0, 8.0, 28.0),
                ("glm-5.2", "GLM-5.2", 2.0, 8.0, 28.0),
                ("glm-5-turbo", "GLM-5-Turbo", 1.2, 5.0, 22.0),
            ],
        )),
        "zhipu" => Some(zhipu_preset(
            "zhipu",
            "CNY",
            &[
                ("glm-5.3", "GLM-5.3", 2.0, 8.0, 28.0),
                ("glm-5.2", "GLM-5.2", 2.0, 8.0, 28.0),
                ("glm-5-turbo", "GLM-5-Turbo", 1.2, 5.0, 22.0),
            ],
        )),
        "zai_api" => Some(zhipu_payg_preset(
            "zai_api",
            "USD",
            &[
                ("glm-5.3", "GLM-5.3", 0.26, 1.4, 4.4),
                ("glm-5.2", "GLM-5.2", 0.26, 1.4, 4.4),
                ("glm-5-turbo", "GLM-5-Turbo", 0.24, 1.2, 4.0),
            ],
        )),
        "zai" => Some(zhipu_preset(
            "zai",
            "USD",
            &[
                ("glm-5.3", "GLM-5.3", 0.26, 1.4, 4.4),
                ("glm-5.2", "GLM-5.2", 0.26, 1.4, 4.4),
                ("glm-5-turbo", "GLM-5-Turbo", 0.24, 1.2, 4.0),
            ],
        )),
        _ => None,
    }
}

/// DeepSeek 预置（单站双币：账户币种由余额 API `currency` 字段返回）。
fn deepseek_preset(currency: &'static str) -> PresetProvider {
    let (flash, pro): ((f64, f64, f64), (f64, f64, f64)) = if currency == "USD" {
        ((0.014, 0.44, 1.32), (0.044, 1.32, 3.96))
    } else {
        ((0.10, 3.0, 9.0), (0.30, 9.0, 27.0))
    };
    let half = |(a, b, c): (f64, f64, f64)| (a / 2.0, b / 2.0, c / 2.0);
    let (flash_off, pro_off) = (half(flash), half(pro));
    PresetProvider {
        native_id: "deepseek",
        currency,
        timezone_offset_minutes: 480,
        windows: vec![
            peak_window_workday("09:00", "12:00"),
            peak_window_workday("14:00", "18:00"),
        ],
        models: vec![
            payg_model("flash", "V4 Flash", flash, flash_off),
            payg_model("pro", "V4 Pro", pro, pro_off),
            payg_model("vision", "V4 Flash Vision Exp", flash, flash_off),
        ],
        default_model: "flash",
    }
}

/// Kimi 预置：无峰谷（恒空闲，两档同价）。
fn kimi_preset(
    native_id: &'static str,
    currency: &'static str,
    models: &[(&'static str, &'static str, f64, f64, f64)],
    default_model: &'static str,
) -> PresetProvider {
    PresetProvider {
        native_id,
        currency,
        timezone_offset_minutes: 480,
        windows: vec![],
        models: models
            .iter()
            .map(|&(id, display, cache_hit, miss, output)| {
                payg_model(
                    id,
                    display,
                    (cache_hit, miss, output),
                    (cache_hit, miss, output),
                )
            })
            .collect(),
        default_model,
    }
}

/// Kimi Code 预置：订阅额度模式，无每 token 三档价，也无峰谷折扣窗口。
fn kimi_code_preset(native_id: &'static str, currency: &'static str) -> PresetProvider {
    PresetProvider {
        native_id,
        currency,
        timezone_offset_minutes: 480,
        windows: vec![],
        models: vec![PresetModel {
            id: "coding-plan",
            display: "Kimi Code（订阅额度）",
            plan: PlanKind::Subscription,
            windows: Some(vec![]),
            peak: PriceTier::default(),
            off_peak: PriceTier::default(),
        }],
        default_model: "coding-plan",
    }
}

/// 智谱/Z.ai 预置：按量模型无峰谷（平台级恒空闲）+ Coding Plan 订阅项
/// （模型级窗口覆盖：工作日 14:00–18:00 高峰，其余时段积分消耗更低，
/// 倍率口径以官网权益说明为准——窗口结构是本预置锁定的部分）。
/// GLM-5-Turbo 国内为输入长度阶梯价（<32K / ≥32K），预置取基础档（<32K）。
fn zhipu_preset(
    native_id: &'static str,
    currency: &'static str,
    models: &[(&'static str, &'static str, f64, f64, f64)],
) -> PresetProvider {
    let mut preset = zhipu_payg_preset(native_id, currency, models);
    preset.models.push(PresetModel {
        id: "coding-plan",
        display: "GLM Coding Plan（订阅积分）",
        plan: PlanKind::Subscription,
        // 订阅项不继承平台级空窗口，显式携带积分折扣时段
        windows: Some(vec![peak_window_workday("14:00", "18:00")]),
        peak: PriceTier::default(),
        off_peak: PriceTier::default(),
    });
    preset
}

/// 智谱/Z.ai 通用 API：只包含按量模型，不混入 Coding Plan 订阅项。
fn zhipu_payg_preset(
    native_id: &'static str,
    currency: &'static str,
    models: &[(&'static str, &'static str, f64, f64, f64)],
) -> PresetProvider {
    debug_assert!(!models.is_empty(), "按量模型列表为空则无默认模型");
    PresetProvider {
        native_id,
        currency,
        timezone_offset_minutes: 480,
        windows: vec![],
        models: models
            .iter()
            .map(|&(id, display, cache_hit, miss, output)| {
                payg_model(
                    id,
                    display,
                    (cache_hit, miss, output),
                    (cache_hit, miss, output),
                )
            })
            .collect(),
        default_model: models[0].0,
    }
}

/// 按量模型构造（无模型级窗口覆盖）。
fn payg_model(
    id: &'static str,
    display: &'static str,
    peak: (f64, f64, f64),
    off_peak: (f64, f64, f64),
) -> PresetModel {
    PresetModel {
        id,
        display,
        plan: PlanKind::PayAsYouGo,
        windows: None,
        peak: PriceTier::full(peak.0, peak.1, peak.2),
        off_peak: PriceTier::full(off_peak.0, off_peak.1, off_peak.2),
    }
}

fn peak_window_workday(start: &str, end: &str) -> PeakWindow {
    PeakWindow {
        days: vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
        ],
        start: start.into(),
        end: end.into(),
    }
}

// ---- 生效解析（自定义与预置的字段级合并） -----------------------------------

/// 生效定价的来源。
#[derive(Debug, Clone, PartialEq)]
pub enum PricingSource {
    /// 全部生效值来自预置（`model` 为生效的预置模型 id）。
    Preset { native_id: String, model: String },
    /// 任一时段/价格/币种生效值来自用户自定义。
    Custom,
}

/// 条目最终生效的峰谷定价（自定义字段级覆盖预置，两端展示共用）。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPricing {
    pub timezone_offset_minutes: Option<i32>,
    /// 生效窗口（空 = 恒空闲）。
    pub windows: Vec<PeakWindow>,
    pub peak: Option<PriceTier>,
    pub off_peak: Option<PriceTier>,
    pub currency: Option<String>,
    /// 生效模型展示标签（预置模型 display 或自定义字符串）。
    pub model_label: Option<String>,
    /// 生效计费模式（订阅项价格档为 None、窗口表达折扣时段）。
    pub plan: PlanKind,
    pub source: PricingSource,
}

impl ResolvedPricing {
    /// 当前峰谷判定（便利封装）。
    pub fn kind(&self, now_ms: u64) -> PeakKind {
        classify(&self.windows, self.timezone_offset_minutes, now_ms)
    }
}

impl PricingConfig {
    /// 全字段缺省——空对象 `"pricing": {}` 等价未配置（resolve 视同 None，
    /// 避免无预置条目落进「有自定义但无任何生效值」的歧义态）。
    fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.timezone_offset_minutes.is_none()
            && self.windows.is_none()
            && self.peak.is_none()
            && self.off_peak.is_none()
            && self.currency.is_none()
    }
}

/// 用户自定义模型定价（按 native id 聚类，存 `AppConfig::custom_models`）。
///
/// 与预置模型同等参与条目 `model` 匹配，**id 撞名时自定义优先**——
/// 可作为官方改价后用户自行修正预置的通道。字段缺失语义：windows/
/// timezone/currency 缺失回退平台级预置（有预置时；与条目级覆盖链一致），
/// **peak/off_peak 缺失不回退**（预置模型价格只在条目未选库模型时生效）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CustomModelDef {
    /// 模型选择键（条目 `pricing.model` 匹配项，大小写不敏感）。
    pub id: String,
    /// 展示名。
    pub display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone_offset_minutes: Option<i32>,
    /// 高峰窗口；None = 回退平台级预置（无预置则恒空闲），Some([]) = 恒空闲。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<Vec<PeakWindow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak: Option<PriceTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off_peak: Option<PriceTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

/// 解析条目生效定价：无可展示内容（无预置，且自定义为空或缺失）→ None。
///
/// 合并规则：`model` 匹配预置模型 id（大小写不敏感）则选定该模型，否则
/// 仅作展示标签（价格回退默认模型）；`windows`/`peak`/`off_peak`/`currency`/
/// `timezone_offset_minutes` 自定义非空即覆盖，否则回退预置；来源以
/// 「是否有任何自定义值生效」判定（时区与 model 标签同口径）。
/// 模型级窗口：选中预置/自定义模型自带 `windows` 时优先于平台级
/// （订阅项如 Coding Plan 由此携带折扣时段）；价格档全空的模型
/// （订阅项）不回退平台价，生效为 None。
pub fn resolve(entry: &ProviderEntry) -> Option<ResolvedPricing> {
    resolve_with(entry, &Default::default())
}

/// [`resolve`] 的完整形态：额外接受按 native id 聚类的用户自定义模型库。
/// 空库时与 [`resolve`] 行为一致。
pub fn resolve_with(
    entry: &ProviderEntry,
    custom_models: &std::collections::BTreeMap<String, Vec<CustomModelDef>>,
) -> Option<ResolvedPricing> {
    resolve_impl(entry, custom_models, None)
}

/// [`resolve_with`] 的带币种形态：`currency_hint` 参与**预置选套**——
/// DeepSeek 单站双币时按 hint（如余额 API 返回的 `currency` 或条目
/// `pricing.currency`）取 CNY/USD 套，其余平台忽略（唯一套）。
/// None = 平台默认套（同 [`resolve_with`]）。生效 `currency` 标签仍走
/// 「条目自定义 > 模型 > 预置套」链，不受 hint 强制。
pub fn resolve_in_currency(
    entry: &ProviderEntry,
    custom_models: &std::collections::BTreeMap<String, Vec<CustomModelDef>>,
    currency_hint: Option<&str>,
) -> Option<ResolvedPricing> {
    resolve_impl(entry, custom_models, currency_hint)
}

fn resolve_impl(
    entry: &ProviderEntry,
    custom_models: &std::collections::BTreeMap<String, Vec<CustomModelDef>>,
    currency_hint: Option<&str>,
) -> Option<ResolvedPricing> {
    let native_id = match &entry.kind {
        ProviderKind::Native { provider } => Some(provider.as_str()),
        _ => None,
    };
    let preset = native_id.and_then(|id| {
        let currency = currency_hint.unwrap_or_else(|| default_currency(id));
        preset_with_currency(id, currency)
    });
    let lib = native_id.and_then(|id| custom_models.get(id));
    let custom = entry.pricing.as_ref();
    // 无预置平台的「有内容」判定是条目级：条目自定义非空，或条目 model
    // 命中库（库的存在本身不算——同平台其他条目未引用时保持 None，
    // 维持「未配置峰谷定价返回空」的托盘契约）。
    let lib_hit = |id: &str| lib.is_some_and(|l| l.iter().any(|m| m.id.eq_ignore_ascii_case(id)));
    let entry_has_content = custom.is_some_and(|c| !c.is_empty())
        || custom.is_some_and(|c| c.model.as_deref().is_some_and(lib_hit));
    if preset.is_none() && !entry_has_content {
        return None;
    }

    // 模型选择：条目 model 先匹配自定义库（撞名优先），再匹配预置；
    // 均不匹配时回退预置默认模型，条目 model 仅作展示标签。
    // 选中模型统一为中间形态：字段缺失处由调用链回退（预置→平台级）。
    struct Selected {
        id: Option<String>,
        label: String,
        plan: PlanKind,
        windows: Option<Vec<PeakWindow>>,
        timezone: Option<i32>,
        peak: Option<PriceTier>,
        off_peak: Option<PriceTier>,
        currency: Option<String>,
        from_lib: bool,
    }
    let non_empty = |t: &PriceTier| (!t.is_empty()).then(|| t.clone());
    // 自定义模型暂只有按量语义（CustomModelDef 无 plan 字段，订阅项
    // 只能来自官方预置；放开属后续能力）
    let from_lib_model = |m: &CustomModelDef| Selected {
        id: Some(m.id.clone()),
        label: m.display.clone(),
        plan: PlanKind::PayAsYouGo,
        windows: m.windows.clone(),
        timezone: m.timezone_offset_minutes,
        peak: m.peak.as_ref().and_then(non_empty),
        off_peak: m.off_peak.as_ref().and_then(non_empty),
        currency: m.currency.clone(),
        from_lib: true,
    };
    let from_preset_model = |m: &PresetModel, p: &PresetProvider| Selected {
        id: Some(m.id.into()),
        label: m.display.into(),
        plan: m.plan,
        windows: m.windows.clone(),
        timezone: Some(p.timezone_offset_minutes),
        peak: non_empty(&m.peak),
        off_peak: non_empty(&m.off_peak),
        currency: None,
        from_lib: false,
    };

    let (model, model_label, model_from_custom) =
        match (&preset, &lib, custom.and_then(|c| c.model.as_deref())) {
            (Some(p), _, Some(id)) => {
                let lib_hit = lib
                    .and_then(|l| l.iter().find(|m| m.id.eq_ignore_ascii_case(id)))
                    .map(from_lib_model);
                let preset_hit = p
                    .models
                    .iter()
                    .find(|m| m.id.eq_ignore_ascii_case(id))
                    .map(|m| from_preset_model(m, p));
                match lib_hit.or(preset_hit) {
                    Some(m) => {
                        let label = m.label.clone();
                        let from_lib = m.from_lib;
                        (Some(m), label, from_lib)
                    }
                    None => {
                        let default = p
                            .models
                            .iter()
                            .find(|m| m.id == p.default_model)
                            .map(|m| from_preset_model(m, p));
                        (default, id.into(), true)
                    }
                }
            }
            (Some(p), _, None) => {
                let default = p
                    .models
                    .iter()
                    .find(|m| m.id == p.default_model)
                    .map(|m| from_preset_model(m, p));
                let label = default.as_ref().map(|m| m.label.clone());
                (default, label.unwrap_or_default(), false)
            }
            (None, Some(l), Some(id)) => {
                let hit = l
                    .iter()
                    .find(|m| m.id.eq_ignore_ascii_case(id))
                    .map(from_lib_model);
                match hit {
                    Some(m) => {
                        let label = m.label.clone();
                        (Some(m), label, true)
                    }
                    // 无预置平台：自定义库存在但未命中 → 纯标签（现状语义）
                    None => (None, id.into(), true),
                }
            }
            (None, _, Some(id)) => (None, id.into(), true),
            (None, _, None) => (None, String::new(), false),
        };

    // 生效值：条目显式 > 选中模型（含其模型级窗口）> 平台级（仅预置）。
    // 自定义库模型缺失 windows/timezone/currency 时回退平台级预置
    // （有预置时；与 CustomModelDef 文档约定一致），平台无预置则无值。
    let tier_or = |custom_tier: Option<&PriceTier>, model_tier: Option<PriceTier>| {
        custom_tier
            .filter(|t| !t.is_empty())
            .cloned()
            .or(model_tier)
    };
    let model_windows = model.as_ref().and_then(|m| m.windows.clone());
    let windows = custom
        .and_then(|c| c.windows.clone())
        .or(model_windows)
        .or_else(|| preset.as_ref().map(|p| p.windows.clone()))
        .unwrap_or_default();
    let peak = tier_or(
        custom.and_then(|c| c.peak.as_ref()),
        model.as_ref().and_then(|m| m.peak.clone()),
    );
    let off_peak = tier_or(
        custom.and_then(|c| c.off_peak.as_ref()),
        model.as_ref().and_then(|m| m.off_peak.clone()),
    );
    let currency = custom
        .and_then(|c| c.currency.clone())
        .or_else(|| model.as_ref().and_then(|m| m.currency.clone()))
        .or_else(|| preset.as_ref().map(|p| p.currency.into()));

    let any_custom = custom.is_some_and(|c| {
        c.windows.is_some()
            || c.timezone_offset_minutes.is_some()
            || c.peak.as_ref().is_some_and(|t| !t.is_empty())
            || c.off_peak.as_ref().is_some_and(|t| !t.is_empty())
            || c.currency.is_some()
            || model_from_custom
    });
    let source = match (any_custom, preset.as_ref()) {
        (false, Some(p)) => PricingSource::Preset {
            native_id: p.native_id.into(),
            model: model
                .as_ref()
                .and_then(|m| m.id.clone())
                .unwrap_or_else(|| p.default_model.into()),
        },
        // 有自定义生效 / 无预置但自定义非空（is_empty 已在入口拦截空对象）
        _ => PricingSource::Custom,
    };

    Some(ResolvedPricing {
        timezone_offset_minutes: custom
            .and_then(|c| c.timezone_offset_minutes)
            .or_else(|| model.as_ref().and_then(|m| m.timezone))
            .or_else(|| preset.as_ref().map(|p| p.timezone_offset_minutes)),
        windows,
        peak,
        off_peak,
        currency,
        model_label: (!model_label.is_empty()).then_some(model_label),
        plan: model.as_ref().map_or(PlanKind::PayAsYouGo, |m| m.plan),
        source,
    })
}

// ---- 测试 ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlanVariant;

    /// 北京时间 2026-08-19（周三）09:30 = UTC 01:30。
    const WED_0930_BJ_MS: u64 = 1_787_103_000_000;
    /// 北京时间 2026-08-19（周三）04:30 = UTC 前一日（周二）20:30。
    const WED_0430_BJ_MS: u64 = 1_787_085_000_000;
    /// 北京时间 2026-08-22（周六）08:40 = UTC 00:40。
    const SAT_0840_BJ_MS: u64 = 1_787_359_200_000;

    /// UTC+8 偏移。
    const BJ: Option<i32> = Some(480);

    fn deepseek_windows() -> Vec<PeakWindow> {
        preset("deepseek").unwrap().windows
    }

    fn parts_ms(weekday: chrono::Weekday, hour: u32, minute: u32) -> u64 {
        // 以 2026-08-17（周一）为基准周构造 UTC 时刻，再按需用偏移判定
        let base_mon_utc = 1_786_924_800_000u64; // 2026-08-17 00:00 UTC
        let day_idx = match weekday {
            chrono::Weekday::Mon => 0,
            chrono::Weekday::Tue => 1,
            chrono::Weekday::Wed => 2,
            chrono::Weekday::Thu => 3,
            chrono::Weekday::Fri => 4,
            chrono::Weekday::Sat => 5,
            chrono::Weekday::Sun => 6,
        };
        base_mon_utc + day_idx as u64 * 86_400_000 + (hour as u64 * 60 + minute as u64) * 60_000
    }

    // ---- classify ----

    /// 契约：DeepSeek 预置时段——工作日窗口边界左闭右开。
    #[test]
    fn classify_deepseek_windows_boundaries() {
        let w = deepseek_windows();
        // 09:00（含）→ 峰
        assert_eq!(
            classify(&w, BJ, parts_ms(chrono::Weekday::Wed, 1, 0)),
            PeakKind::Peak
        );
        // 12:00（排）→ 谷
        assert_eq!(
            classify(&w, BJ, parts_ms(chrono::Weekday::Wed, 4, 0)),
            PeakKind::OffPeak
        );
        // 13:59 → 谷（午间）；14:00 → 峰
        assert_eq!(
            classify(&w, BJ, parts_ms(chrono::Weekday::Wed, 5, 59)),
            PeakKind::OffPeak
        );
        assert_eq!(
            classify(&w, BJ, parts_ms(chrono::Weekday::Wed, 6, 0)),
            PeakKind::Peak
        );
        // 18:00（排）→ 谷
        assert_eq!(
            classify(&w, BJ, parts_ms(chrono::Weekday::Wed, 10, 0)),
            PeakKind::OffPeak
        );
    }

    /// 契约：周末全天、工作日夜间均为空闲。
    #[test]
    fn classify_weekend_and_night_off_peak() {
        let w = deepseek_windows();
        // 周六 UTC 02:00 = 北京 10:00
        assert_eq!(
            classify(&w, BJ, parts_ms(chrono::Weekday::Sat, 2, 0)),
            PeakKind::OffPeak
        );
        // 周三 21:00 BJ = UTC 13:00 → 夜间谷
        assert_eq!(
            classify(&w, BJ, parts_ms(chrono::Weekday::Wed, 13, 0)),
            PeakKind::OffPeak
        );
        // 周五 11:30 BJ（UTC 03:30）→ 峰；同 UTC 时刻的周六 → 谷
        assert_eq!(
            classify(&w, BJ, parts_ms(chrono::Weekday::Fri, 3, 30)),
            PeakKind::Peak
        );
        assert_eq!(
            classify(&w, BJ, parts_ms(chrono::Weekday::Sat, 3, 30)),
            PeakKind::OffPeak
        );
    }

    /// 契约：UTC 偏移参与换算（官网英文页：UTC 01:00–04:00 峰）。
    #[test]
    fn classify_explicit_offset_matches_utc_page() {
        let w = deepseek_windows();
        // 周三 UTC 01:30 = 北京 09:30 → 峰（预置真实锚点交叉验证）
        assert_eq!(classify(&w, BJ, WED_0930_BJ_MS), PeakKind::Peak);
        // 北京周三 04:30（夜间）→ 谷
        assert_eq!(classify(&w, BJ, WED_0430_BJ_MS), PeakKind::OffPeak);
        // 周六 UTC 00:40 → 谷
        assert_eq!(classify(&w, BJ, SAT_0840_BJ_MS), PeakKind::OffPeak);
    }

    /// 契约：无窗口恒空闲；非法时刻窗口不参与判定（防御）。
    #[test]
    fn classify_empty_windows_and_invalid_defense() {
        assert_eq!(classify(&[], BJ, WED_0930_BJ_MS), PeakKind::OffPeak);
        let bad = vec![PeakWindow {
            days: vec![Weekday::Mon],
            start: "9点".into(),
            end: "12:00".into(),
        }];
        assert_eq!(classify(&bad, BJ, WED_0930_BJ_MS), PeakKind::OffPeak);
    }

    /// 契约：偏移为 None 时用本地时区判定（北京机器上同一时刻两种入法等价）。
    #[test]
    fn classify_local_timezone_default() {
        let w = deepseek_windows();
        let now_ms = WED_0930_BJ_MS;
        // 偏移取**入参时刻**的本地偏移（而非运行"此刻"）——DST 时区的
        // 冬夏偏移不同，用"此刻"会在冬季跑出 flaky
        let offset = chrono::Local
            .timestamp_millis_opt(now_ms as i64)
            .single()
            .map(|dt| dt.offset().local_minus_utc())
            .expect("入参在 chrono 有效范围内");
        assert_eq!(
            classify(&w, None, now_ms),
            classify(&w, Some(offset), now_ms),
            "tz=None 应等价于入参时刻的本地偏移"
        );
    }

    /// 契约：非法偏移不 panic，且等价于**入参时刻**的本地时区判定（保持
    /// 纯函数——兜底若换成"此刻"，next_change 扫描会恒等于 current 而
    /// 静默返回 None）。
    #[test]
    fn classify_invalid_offset_falls_back() {
        let w = deepseek_windows();
        let now = WED_0930_BJ_MS;
        // 入参时刻的本地偏移（同上，避免 DST 冬季 flaky）
        let local_offset = chrono::Local
            .timestamp_millis_opt(now as i64)
            .single()
            .map(|dt| dt.offset().local_minus_utc())
            .expect("入参在 chrono 有效范围内");
        assert_eq!(
            classify(&w, Some(i32::MAX), now),
            classify(&w, Some(local_offset), now),
            "非法偏移应按入参时刻的本地时区判定"
        );
        // 非法偏移下 next_change 仍能找到翻转（而非静默 None）。
        // 翻转方向动态推导（与当前态相反）——不依赖运行机器时区：
        // 入参时刻的本地判定在 UTC 机器上是谷，固定断言 OffPeak 会红 CI。
        let cur = classify(&w, Some(i32::MAX), now);
        let (t, kind) = next_change(&w, Some(i32::MAX), now).expect("应找到翻转");
        assert_ne!(kind, cur);
        assert!(t > now);
    }

    // ---- next_change ----

    /// 契约：峰内可找到翻转点（11:30 → 12:00 转谷）；谷内找到下一窗口开启。
    #[test]
    fn next_change_finds_flip() {
        let w = deepseek_windows();
        // 周三 09:30（峰）→ 12:00 转谷
        let (t, kind) = next_change(&w, BJ, parts_ms(chrono::Weekday::Wed, 1, 30)).unwrap();
        assert_eq!(kind, PeakKind::OffPeak);
        assert_eq!(t, parts_ms(chrono::Weekday::Wed, 4, 0));
        // 周三 13:00（谷）→ 14:00 转峰
        let (t, kind) = next_change(&w, BJ, parts_ms(chrono::Weekday::Wed, 5, 0)).unwrap();
        assert_eq!(kind, PeakKind::Peak);
        assert_eq!(t, parts_ms(chrono::Weekday::Wed, 6, 0));
        // 周六 10:00（谷）→ 周一 09:00 转峰（跨周末）
        let (t, kind) = next_change(&w, BJ, parts_ms(chrono::Weekday::Sat, 2, 0)).unwrap();
        assert_eq!(kind, PeakKind::Peak);
        assert_eq!(t, parts_ms(chrono::Weekday::Mon, 1, 0) + 7 * 86_400_000);
    }

    /// 契约：无窗口 → 无翻转。
    #[test]
    fn next_change_none_without_windows() {
        assert_eq!(next_change(&[], BJ, WED_0930_BJ_MS), None);
    }

    /// 契约：非空窗口但 7 天内不翻转（全周 00:00–24:00 恒峰）→ None。
    #[test]
    fn next_change_none_when_never_flips() {
        let always_peak = vec![PeakWindow {
            days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
                Weekday::Sat,
                Weekday::Sun,
            ],
            start: "00:00".into(),
            end: "24:00".into(),
        }];
        assert_eq!(next_change(&always_peak, BJ, WED_0930_BJ_MS), None);
    }

    // ---- format_price ----

    /// 契约：最多 2 位小数去尾零；非有限值显示 "—"。
    #[test]
    fn format_price_trims_zeros() {
        assert_eq!(format_price(0.30), "0.3");
        assert_eq!(format_price(27.0), "27");
        assert_eq!(format_price(1.5), "1.5");
        assert_eq!(format_price(0.05), "0.05");
        assert_eq!(format_price(0.0), "0");
        assert_eq!(
            format_price(-0.0),
            "0",
            "负零不显示成 -0（validate 放行 -0.0）"
        );
        assert_eq!(format_price(f64::NAN), "—");
        assert_eq!(format_price(f64::INFINITY), "—");
    }

    /// 契约：小于 0.05 的非零价保留 4 位小数——美元档峰谷命中价
    /// （谷 0.007 / 峰 0.014）不得都舍入成 "0.01"（两档显示撞车）。
    #[test]
    fn format_price_keeps_sub_cents_distinct() {
        assert_eq!(format_price(0.007), "0.007");
        assert_eq!(format_price(0.014), "0.014");
        assert_eq!(format_price(0.0001), "0.0001");
        assert_ne!(
            format_price(0.007),
            format_price(0.014),
            "美元档峰谷命中价不得显示相同"
        );
    }

    // ---- validate ----

    fn valid_cfg() -> PricingConfig {
        PricingConfig {
            model: None,
            timezone_offset_minutes: Some(480),
            windows: Some(vec![PeakWindow {
                days: vec![Weekday::Mon, Weekday::Fri],
                start: "09:00".into(),
                end: "12:00".into(),
            }]),
            peak: Some(PriceTier::full(0.3, 9.0, 27.0)),
            off_peak: Some(PriceTier::full(0.15, 4.5, 13.5)),
            currency: Some("CNY".into()),
        }
    }

    /// 契约：合法配置通过。
    #[test]
    fn validate_accepts_valid() {
        assert_eq!(validate(&valid_cfg()), Ok(()));
        // 全空配置也合法（恒空闲 + 无价格，展示层自然处理）
        assert_eq!(validate(&PricingConfig::default()), Ok(()));
    }

    /// 契约：跨日窗口、非法时刻、空 days、负价、越界偏移逐一拦截并带字段定位。
    #[test]
    fn validate_rejects_bad_configs() {
        let mut cfg = valid_cfg();
        cfg.windows.as_mut().unwrap()[0].start = "22:00".into();
        cfg.windows.as_mut().unwrap()[0].end = "06:00".into();
        let err = validate(&cfg).unwrap_err();
        assert!(err.to_string().contains("不跨日"), "{err}");
        assert!(
            matches!(err, PricingError::Validation { field, .. } if field == "windows[0].start/end")
        );

        let mut cfg = valid_cfg();
        cfg.windows.as_mut().unwrap()[0].start = "25:00".into();
        assert!(validate(&cfg).unwrap_err().to_string().contains("HH:MM"));

        let mut cfg = valid_cfg();
        cfg.windows.as_mut().unwrap()[0].days = vec![];
        assert!(validate(&cfg).unwrap_err().to_string().contains("星期"));

        let mut cfg = valid_cfg();
        cfg.peak.as_mut().unwrap().output = Some(-1.0);
        let err = validate(&cfg).unwrap_err();
        assert!(matches!(err, PricingError::Validation { field, .. } if field == "peak.output"));

        let mut cfg = valid_cfg();
        cfg.timezone_offset_minutes = Some(900);
        assert!(validate(&cfg).unwrap_err().to_string().contains("偏移"));
    }

    // ---- serde ----

    /// 契约：PricingConfig JSON roundtrip（snake_case、Option 字段缺省）。
    #[test]
    fn pricing_config_serde_roundtrip() {
        let json = r#"{
            "model": "pro",
            "timezone_offset_minutes": 480,
            "windows": [
                {"days": ["mon","tue","wed","thu","fri"], "start": "09:00", "end": "12:00"}
            ],
            "peak": {"cache_hit_input": 0.3, "cache_miss_input": 9.0, "output": 27.0},
            "currency": "CNY"
        }"#;
        let cfg: PricingConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.model.as_deref(), Some("pro"));
        assert_eq!(cfg.off_peak, None);
        assert_eq!(cfg.windows.as_ref().unwrap().len(), 1);
        assert_eq!(
            cfg.windows.as_ref().unwrap()[0].days,
            vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri
            ]
        );
        let back = serde_json::to_string(&cfg).unwrap();
        let cfg2: PricingConfig = serde_json::from_str(&back).unwrap();
        assert_eq!(cfg, cfg2);
        // 空对象 = 全默认（恒空闲 + 无价格）
        let empty: PricingConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, PricingConfig::default());
    }

    // ---- 预置数据快照 ----

    /// 契约：DeepSeek 预置数据逐字锁定（官网 2026-08-23 抓取，改价须核对官网）。
    #[test]
    fn deepseek_preset_snapshot() {
        let p = preset("deepseek").unwrap();
        assert_eq!(p.currency, "CNY");
        assert_eq!(p.timezone_offset_minutes, 480);
        assert_eq!(p.default_model, "flash");
        assert_eq!(p.models.len(), 3);
        let flash = &p.models[0];
        let pro = &p.models[1];
        let vision = &p.models[2];
        assert_eq!(flash.id, "flash");
        assert_eq!(flash.peak, PriceTier::full(0.10, 3.0, 9.0));
        assert_eq!(flash.off_peak, PriceTier::full(0.05, 1.5, 4.5));
        assert_eq!(pro.id, "pro");
        assert_eq!(pro.peak, PriceTier::full(0.30, 9.0, 27.0));
        assert_eq!(pro.off_peak, PriceTier::full(0.15, 4.5, 13.5));
        assert_eq!(vision.id, "vision");
        assert_eq!(vision.peak, PriceTier::full(0.10, 3.0, 9.0));
        assert_eq!(vision.off_peak, PriceTier::full(0.05, 1.5, 4.5));
        // 空闲价 = 高峰一半（官网规则自检）
        for m in &p.models {
            for (a, b) in [
                (m.peak.cache_hit_input, m.off_peak.cache_hit_input),
                (m.peak.cache_miss_input, m.off_peak.cache_miss_input),
                (m.peak.output, m.off_peak.output),
            ] {
                assert!((b.unwrap() * 2.0 - a.unwrap()).abs() < 1e-9);
            }
        }
        assert_eq!(
            p.windows,
            vec![
                peak_window_workday("09:00", "12:00"),
                peak_window_workday("14:00", "18:00"),
            ]
        );
    }

    /// 契约：无预置的 native id 与 template 条目返回 None。
    #[test]
    fn preset_absent_for_others() {
        assert!(preset("siliconflow").is_none());
        assert!(preset("openrouter").is_none());
        assert!(preset("nope").is_none());
    }

    // ---- resolve 合并回退链 ----

    /// 无峰谷平台的按量模型构造（两档同价）。
    fn flat_payg(id: &'static str, display: &'static str, t: (f64, f64, f64)) -> PresetModel {
        PresetModel {
            id,
            display,
            plan: PlanKind::PayAsYouGo,
            windows: None,
            peak: PriceTier::full(t.0, t.1, t.2),
            off_peak: PriceTier::full(t.0, t.1, t.2),
        }
    }

    /// 契约：新平台预置数据逐字锁定（官网 2026-08-23 抓取，改价须核对官网）。
    /// Kimi 定价页 platform.kimi.com（CNY）/ platform.kimi.ai（USD）；
    /// 智谱 open.bigmodel.cn/pricing（SPA 实抓）/ docs.z.ai（USD）。
    /// 整 Vec 对比：任何模型/字段的手滑都会击穿本测试。
    #[test]
    fn new_provider_presets_snapshot() {
        // Kimi 国内：K3 与 K2.7-Code，无峰谷（两档同价）
        let p = preset("kimi_cn").unwrap();
        assert_eq!(p.currency, "CNY");
        assert_eq!(p.timezone_offset_minutes, 480);
        assert!(p.windows.is_empty(), "Kimi 无峰谷");
        assert_eq!(p.default_model, "k3");
        assert_eq!(
            p.models,
            vec![
                flat_payg("k3", "Kimi K3", (2.0, 20.0, 100.0)),
                flat_payg("k27-code", "Kimi K2.7 Code", (1.3, 6.5, 27.0)),
            ]
        );

        // Kimi 国际：K3 与 K2.6 美元价
        let p = preset("kimi_global").unwrap();
        assert_eq!(p.currency, "USD");
        assert_eq!(p.timezone_offset_minutes, 480);
        assert!(p.windows.is_empty());
        assert_eq!(p.default_model, "k3");
        assert_eq!(
            p.models,
            vec![
                flat_payg("k3", "Kimi K3", (0.30, 3.00, 15.00)),
                flat_payg("k26", "Kimi K2.6", (0.16, 0.95, 4.00)),
            ]
        );

        // 智谱国内：GLM-5.3 / 5.2 / 5-Turbo（基础档）+ Coding Plan 订阅项
        let p = preset("zhipu").unwrap();
        assert_eq!(p.currency, "CNY");
        assert_eq!(p.timezone_offset_minutes, 480);
        assert!(p.windows.is_empty(), "智谱按量无峰谷（平台级恒空闲）");
        assert_eq!(p.default_model, "glm-5.3");
        assert_eq!(
            p.models[..3],
            [
                flat_payg("glm-5.3", "GLM-5.3", (2.0, 8.0, 28.0)),
                flat_payg("glm-5.2", "GLM-5.2", (2.0, 8.0, 28.0)),
                flat_payg("glm-5-turbo", "GLM-5-Turbo", (1.2, 5.0, 22.0)),
            ]
        );
        let coding = &p.models[3];
        assert_eq!(coding.id, "coding-plan");
        assert_eq!(coding.display, "GLM Coding Plan（订阅积分）");
        assert_eq!(coding.plan, PlanKind::Subscription);
        assert!(
            coding.peak.is_empty() && coding.off_peak.is_empty(),
            "订阅项无三档价"
        );
        assert_eq!(
            coding.windows.as_deref(),
            Some(&[peak_window_workday("14:00", "18:00")][..]),
            "Coding Plan 高峰 = 工作日 14:00–18:00"
        );
        assert_eq!(p.models.len(), 4, "不应有第 5 个模型");

        // Z.ai：美元价（5.3 与 5.2 同价）+ 同款订阅项
        let p = preset("zai").unwrap();
        assert_eq!(p.currency, "USD");
        assert_eq!(p.timezone_offset_minutes, 480);
        assert_eq!(p.default_model, "glm-5.3");
        assert_eq!(
            p.models[..3],
            [
                flat_payg("glm-5.3", "GLM-5.3", (0.26, 1.4, 4.4)),
                flat_payg("glm-5.2", "GLM-5.2", (0.26, 1.4, 4.4)),
                flat_payg("glm-5-turbo", "GLM-5-Turbo", (0.24, 1.2, 4.0)),
            ]
        );
        assert_eq!(p.models[3].plan, PlanKind::Subscription);
        assert_eq!(
            p.models[3].windows,
            Some(vec![peak_window_workday("14:00", "18:00")])
        );
        assert_eq!(p.models.len(), 4);

        // 无峰谷预置的平台
        assert!(preset("siliconflow").is_none());
        assert!(preset("siliconflow_global").is_none());
        assert!(preset("openrouter").is_none());
    }

    /// 契约：Kimi Code 是独立订阅 Provider，不复用开放平台按量模型；
    /// 国内/国际仅币种不同，订阅模型无三档价、无折扣窗口。
    #[test]
    fn kimi_code_subscription_presets_snapshot() {
        for (id, currency) in [("kimi_code_cn", "CNY"), ("kimi_code_global", "USD")] {
            let p = preset(id).unwrap();
            assert_eq!(p.native_id, id);
            assert_eq!(p.currency, currency);
            assert_eq!(p.default_model, "coding-plan");
            assert!(p.windows.is_empty());
            assert_eq!(p.models.len(), 1);
            let model = &p.models[0];
            assert_eq!(model.id, "coding-plan");
            assert_eq!(model.display, "Kimi Code（订阅额度）");
            assert_eq!(model.plan, PlanKind::Subscription);
            assert_eq!(model.windows, Some(vec![]));
            assert!(model.peak.is_empty() && model.off_peak.is_empty());
        }
    }

    /// 契约：智谱/Z.ai 通用 API Provider 只提供按量模型，不混入
    /// Coding Plan 订阅项；两站分别使用 CNY/USD。
    #[test]
    fn zhipu_metered_presets_only_contain_payg_models() {
        for (id, currency) in [("zhipu_api", "CNY"), ("zai_api", "USD")] {
            let p = preset(id).unwrap();
            assert_eq!(p.native_id, id);
            assert_eq!(p.currency, currency);
            assert_eq!(p.default_model, "glm-5.3");
            assert_eq!(p.models.len(), 3);
            assert!(
                p.models
                    .iter()
                    .all(|model| model.plan == PlanKind::PayAsYouGo)
            );
            assert!(p.models.iter().all(|model| model.windows.is_none()));
        }
    }

    /// 契约：DeepSeek 单站双币——余额 API 的 currency 决定取哪套预置；
    /// USD 套同样满足「空闲 = 高峰减半」。
    #[test]
    fn deepseek_currency_variants() {
        let cny = preset_with_currency("deepseek", "CNY").unwrap();
        let usd = preset_with_currency("deepseek", "USD").unwrap();
        assert_eq!(cny.currency, "CNY");
        assert_eq!(usd.currency, "USD");
        // USD 套三模型逐字锁定（Flash 与 Vision 同价）
        assert_eq!(
            usd.models,
            vec![
                PresetModel {
                    id: "flash",
                    display: "V4 Flash",
                    plan: PlanKind::PayAsYouGo,
                    windows: None,
                    peak: PriceTier::full(0.014, 0.44, 1.32),
                    off_peak: PriceTier::full(0.007, 0.22, 0.66),
                },
                PresetModel {
                    id: "pro",
                    display: "V4 Pro",
                    plan: PlanKind::PayAsYouGo,
                    windows: None,
                    peak: PriceTier::full(0.044, 1.32, 3.96),
                    off_peak: PriceTier::full(0.022, 0.66, 1.98),
                },
                PresetModel {
                    id: "vision",
                    display: "V4 Flash Vision Exp",
                    plan: PlanKind::PayAsYouGo,
                    windows: None,
                    peak: PriceTier::full(0.014, 0.44, 1.32),
                    off_peak: PriceTier::full(0.007, 0.22, 0.66),
                },
            ]
        );
        // 减半自检（与 CNY 套的 deepseek_preset_snapshot 同口径）
        for m in &usd.models {
            for (peak_v, off_v) in [
                (m.peak.cache_hit_input, m.off_peak.cache_hit_input),
                (m.peak.cache_miss_input, m.off_peak.cache_miss_input),
                (m.peak.output, m.off_peak.output),
            ] {
                assert_eq!(off_v.unwrap(), peak_v.unwrap() / 2.0);
            }
        }
        // 窗口与 CNY 套一致（同一峰谷时段，仅价格币种不同）
        assert_eq!(cny.windows, usd.windows);
        // 大小写不敏感；其余平台忽略币种返回唯一套
        assert_eq!(
            preset_with_currency("deepseek", "usd").unwrap().currency,
            "USD"
        );
        assert_eq!(
            preset_with_currency("kimi_cn", "USD").unwrap().currency,
            "CNY",
            "Kimi 国内无美元套，忽略币种"
        );
        // 默认币种表与注册表对齐（openrouter 按量为 USD，防误兜底）；
        // 遍历注册表防新增平台漏配清单
        for (id, expect) in [
            ("deepseek", "CNY"),
            ("siliconflow", "CNY"),
            ("siliconflow_global", "USD"),
            ("openrouter", "USD"),
            ("kimi_cn", "CNY"),
            ("kimi_global", "USD"),
            ("kimi_code_cn", "CNY"),
            ("kimi_code_global", "USD"),
            ("zhipu_api", "CNY"),
            ("zai_api", "USD"),
            ("zhipu", "CNY"),
            ("zai", "USD"),
            ("stepfun", "CNY"),
            ("novita", "USD"),
            ("minimax", "CNY"),
            ("minimax_global", "USD"),
            ("claude", "CNY"),
            ("codex", "CNY"),
        ] {
            assert_eq!(default_currency(id), expect, "{id}");
        }
        let known: std::collections::HashSet<&str> = [
            "deepseek",
            "siliconflow",
            "siliconflow_global",
            "openrouter",
            "kimi_cn",
            "kimi_global",
            "kimi_code_cn",
            "kimi_code_global",
            "zhipu_api",
            "zai_api",
            "zhipu",
            "zai",
            "stepfun",
            "novita",
            "minimax",
            "minimax_global",
            "claude",
            "codex",
        ]
        .into_iter()
        .collect();
        for meta in crate::provider::metas() {
            assert!(
                known.contains(meta.id),
                "default_currency 测试清单缺少注册表平台 {}，请补断言",
                meta.id
            );
        }
    }

    fn native_entry(provider: &str) -> ProviderEntry {
        ProviderEntry {
            id: "e1".into(),
            name: provider.into(),
            kind: ProviderKind::Native {
                provider: provider.into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        }
    }

    /// 契约：订阅项 resolve——Coding Plan 窗口生效、三档价为 None、
    /// 计费模式透出 Subscription（GUI 依此切换"积分倍率"文案）。
    #[test]
    fn subscription_model_resolution() {
        let mut entry = native_entry("zhipu");
        entry.pricing = Some(PricingConfig {
            model: Some("coding-plan".into()),
            ..Default::default()
        });
        let r = resolve(&entry).unwrap();
        assert_eq!(r.plan, PlanKind::Subscription);
        assert!(r.peak.is_none() && r.off_peak.is_none(), "订阅项无价格档");
        assert_eq!(r.windows, vec![peak_window_workday("14:00", "18:00")]);
        assert_eq!(
            r.model_label.as_deref(),
            Some("GLM Coding Plan（订阅积分）")
        );
        // 峰谷判定与下次翻转对订阅窗口照常生效：
        // 工作日 15:00（北京）= UTC 07:00 高峰；周六恒空闲
        assert_eq!(r.kind(parts_ms(chrono::Weekday::Wed, 7, 0)), PeakKind::Peak);
        assert_eq!(r.kind(SAT_0840_BJ_MS), PeakKind::OffPeak);

        // 按量模型：恒空闲（无峰谷）、三档价生效、模式 PayAsYouGo
        let mut entry = native_entry("zhipu");
        entry.pricing = Some(PricingConfig {
            model: Some("glm-5.3".into()),
            ..Default::default()
        });
        let r = resolve(&entry).unwrap();
        assert_eq!(r.plan, PlanKind::PayAsYouGo);
        assert!(r.windows.is_empty());
        assert_eq!(r.peak.as_ref().unwrap().output, Some(28.0));
    }

    /// 契约：自定义模型库——id 撞名时自定义优先（官方改价修正通道），
    /// 未撞名时按 id 正常匹配，条目字段仍可覆盖模型值。
    #[test]
    fn resolve_with_custom_library() {
        let lib = |models: Vec<CustomModelDef>| {
            let mut m = std::collections::BTreeMap::new();
            m.insert("deepseek".to_string(), models);
            m
        };

        // 撞名覆盖：用户自建的 "flash" 价格优先于官方预置；
        // 缺失的 windows/timezone/currency 回退平台级预置
        // （「撞名只改价、保留官方窗口」主用例），仅价格档缺失不回退
        let custom_flash = CustomModelDef {
            id: "flash".into(),
            display: "V4 Flash（自算）".into(),
            peak: Some(PriceTier::full(0.11, 3.1, 9.1)),
            off_peak: None,
            currency: None,
            ..Default::default()
        };
        let mut entry = deepseek_entry(None);
        entry.pricing = Some(PricingConfig {
            model: Some("FLASH".into()),
            ..Default::default()
        });
        let r = resolve_with(&entry, &lib(vec![custom_flash])).unwrap();
        assert_eq!(r.model_label.as_deref(), Some("V4 Flash（自算）"));
        assert_eq!(r.peak.as_ref().unwrap().cache_hit_input, Some(0.11));
        assert_eq!(r.off_peak, None, "自定义模型缺失档不回退预置");
        assert_eq!(r.windows, deepseek_windows(), "缺失窗口回退平台级预置");
        assert_eq!(r.timezone_offset_minutes, BJ, "缺失时区回退平台级预置");
        assert_eq!(r.currency.as_deref(), Some("CNY"), "缺失币种回退平台级预置");
        assert!(matches!(r.source, PricingSource::Custom));

        // 未撞名的库模型：windows/tz/currency 全量来自模型定义
        // （currency/tz 故意取与平台预置不同的值，正向锁定模型级优先链）
        let night = CustomModelDef {
            id: "night-only".into(),
            display: "夜间特惠档".into(),
            windows: Some(vec![peak_window_workday("09:00", "12:00")]),
            timezone_offset_minutes: Some(0),
            peak: Some(PriceTier::full(1.0, 2.0, 3.0)),
            off_peak: Some(PriceTier::full(0.5, 1.0, 1.5)),
            currency: Some("USD".into()),
        };
        let mut entry = deepseek_entry(None);
        entry.pricing = Some(PricingConfig {
            model: Some("night-only".into()),
            ..Default::default()
        });
        let r = resolve_with(&entry, &lib(vec![night.clone()])).unwrap();
        assert_eq!(r.windows, night.windows.clone().unwrap());
        assert_eq!(r.peak.as_ref().unwrap().output, Some(3.0));
        assert_eq!(
            r.currency.as_deref(),
            Some("USD"),
            "模型级币种优先于平台预置（CNY）"
        );
        assert_eq!(
            r.timezone_offset_minutes,
            Some(0),
            "模型级时区优先于平台预置（480）"
        );

        // 条目显式字段仍可覆盖库模型（条目 > 模型 > 平台级）：
        // 价格与窗口两条链各自独立覆盖
        let mut entry = deepseek_entry(None);
        entry.pricing = Some(PricingConfig {
            model: Some("night-only".into()),
            peak: Some(PriceTier::full(9.0, 9.0, 9.0)),
            windows: Some(vec![]),
            ..Default::default()
        });
        let r = resolve_with(&entry, &lib(vec![night])).unwrap();
        assert_eq!(r.peak.as_ref().unwrap().output, Some(9.0), "价格链覆盖");
        assert!(r.windows.is_empty(), "条目显式空窗口覆盖库模型窗口");

        // 空库 = 现有 resolve 行为（逐字段等价）
        let entry = deepseek_entry(None);
        assert_eq!(resolve_with(&entry, &Default::default()), resolve(&entry));
    }

    /// 契约：自定义库对无预置平台同样生效（如 siliconflow），
    /// 库未命中时 model 仍是纯标签（现状语义不变）。
    #[test]
    fn library_on_presetless_provider() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            "siliconflow".to_string(),
            vec![CustomModelDef {
                id: "glm-5.2".into(),
                display: "GLM-5.2 转售价".into(),
                peak: Some(PriceTier::full(1.0, 4.0, 2.0)),
                currency: Some("CNY".into()),
                ..Default::default()
            }],
        );
        let mut entry = native_entry("siliconflow");
        entry.pricing = Some(PricingConfig {
            model: Some("GLM-5.2".into()),
            ..Default::default()
        });
        let r = resolve_with(&entry, &m).unwrap();
        assert_eq!(r.model_label.as_deref(), Some("GLM-5.2 转售价"));
        assert_eq!(r.peak.as_ref().unwrap().cache_miss_input, Some(4.0));
        assert_eq!(r.plan, PlanKind::PayAsYouGo);

        // 库的存在是平台级、生效是条目级：同平台未引用库/未自定义的
        // 条目保持 None（「未配置峰谷定价返回空」的托盘契约不被打破）
        assert_eq!(
            resolve_with(&native_entry("siliconflow"), &m),
            None,
            "未引用库的条目不应凭库存在而获得定价"
        );
    }

    /// 契约：带币种 resolve——hint 选 DeepSeek USD 套（数字与标签一起切），
    /// None 同默认套，非双币平台忽略 hint；自定义 currency 标签仍最高优先。
    #[test]
    fn resolve_in_currency_selects_preset_variant() {
        let entry = deepseek_entry(None);
        // 默认（无 hint）：CNY 套
        let r = resolve_in_currency(&entry, &Default::default(), None).unwrap();
        assert_eq!(r.currency.as_deref(), Some("CNY"));
        assert_eq!(r.peak.as_ref().unwrap().cache_hit_input, Some(0.1));
        // USD hint：数字与标签同时切到 USD 套
        let r = resolve_in_currency(&entry, &Default::default(), Some("USD")).unwrap();
        assert_eq!(r.currency.as_deref(), Some("USD"));
        assert_eq!(r.peak.as_ref().unwrap().cache_hit_input, Some(0.014));
        assert_eq!(r.peak.as_ref().unwrap().output, Some(1.32));
        // 非双币平台忽略 hint（zhipu 唯一 CNY 套）
        let zhipu = native_entry("zhipu");
        let r = resolve_in_currency(&zhipu, &Default::default(), Some("USD")).unwrap();
        assert_eq!(r.currency.as_deref(), Some("CNY"));
        // 条目显式 currency 优先于 hint 选套的标签（用户强制声明）
        let entry = deepseek_entry(Some(PricingConfig {
            currency: Some("CNY".into()),
            ..Default::default()
        }));
        let r = resolve_in_currency(&entry, &Default::default(), Some("USD")).unwrap();
        assert_eq!(
            r.currency.as_deref(),
            Some("CNY"),
            "条目 currency 标签优先（hint 只选套不强制标签）"
        );
    }

    /// 契约：自定义模型校验——id/display 空白拦截；窗口/时区/价格语义
    /// 复用 validate（跨日窗口、负价格拦截）；合法定义通过。
    #[test]
    fn validate_custom_model_contract() {
        let ok = CustomModelDef {
            id: "glm-5.5".into(),
            display: "GLM-5.5".into(),
            windows: Some(vec![peak_window_workday("09:00", "12:00")]),
            peak: Some(PriceTier::full(1.0, 2.0, 3.0)),
            ..Default::default()
        };
        assert!(validate_custom_model(&ok).is_ok());
        // 空 windows 数组 = 恒空闲，合法
        let flat = CustomModelDef {
            windows: Some(vec![]),
            ..ok.clone()
        };
        assert!(validate_custom_model(&flat).is_ok());

        for (bad, field) in [
            (
                CustomModelDef {
                    id: "  ".into(),
                    ..ok.clone()
                },
                "id",
            ),
            (
                CustomModelDef {
                    display: String::new(),
                    ..ok.clone()
                },
                "display",
            ),
            (
                CustomModelDef {
                    windows: Some(vec![PeakWindow {
                        days: vec![Weekday::Mon],
                        start: "22:00".into(),
                        end: "06:00".into(),
                    }]),
                    ..ok.clone()
                },
                "windows[0].start/end",
            ),
            (
                CustomModelDef {
                    peak: Some(PriceTier {
                        cache_miss_input: Some(-1.0),
                        ..Default::default()
                    }),
                    ..ok
                },
                "peak.cache_miss_input",
            ),
        ] {
            let err = validate_custom_model(&bad).unwrap_err();
            assert!(
                err.to_string().contains(field),
                "{field} 应被点名，实际：{err}"
            );
        }
    }

    fn deepseek_entry(pricing: Option<PricingConfig>) -> ProviderEntry {
        ProviderEntry {
            id: "p1".into(),
            name: "DeepSeek".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        }
    }

    fn template_entry(pricing: Option<PricingConfig>) -> ProviderEntry {
        use crate::template::{TemplateConfig, TemplateRequest};
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
            pricing,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        }
    }

    /// 契约：无自定义的 DeepSeek 条目 → 预置 flash 全套。
    #[test]
    fn resolve_defaults_to_preset_flash() {
        let r = resolve(&deepseek_entry(None)).unwrap();
        assert_eq!(
            r.source,
            PricingSource::Preset {
                native_id: "deepseek".into(),
                model: "flash".into()
            }
        );
        assert_eq!(r.model_label.as_deref(), Some("V4 Flash"));
        assert_eq!(r.timezone_offset_minutes, Some(480));
        assert_eq!(r.windows.len(), 2);
        assert_eq!(r.peak, Some(PriceTier::full(0.10, 3.0, 9.0)));
        assert_eq!(r.currency.as_deref(), Some("CNY"));
    }

    /// 契约：model 选择（大小写不敏感）切换预置价格档；来源仍为预置。
    #[test]
    fn resolve_model_selection_switches_preset_tier() {
        let cfg = PricingConfig {
            model: Some("PRO".into()),
            ..Default::default()
        };
        let r = resolve(&deepseek_entry(Some(cfg))).unwrap();
        assert_eq!(
            r.source,
            PricingSource::Preset {
                native_id: "deepseek".into(),
                model: "pro".into()
            }
        );
        assert_eq!(r.model_label.as_deref(), Some("V4 Pro"));
        assert_eq!(r.peak, Some(PriceTier::full(0.30, 9.0, 27.0)));
    }

    /// 契约：仅自定义时段 → 价格回退预置默认模型，来源 Custom。
    #[test]
    fn resolve_custom_windows_keep_preset_prices() {
        let cfg = PricingConfig {
            windows: Some(vec![peak_window_workday("10:00", "11:00")]),
            ..Default::default()
        };
        let r = resolve(&deepseek_entry(Some(cfg))).unwrap();
        assert_eq!(r.source, PricingSource::Custom);
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.windows[0].start, "10:00");
        assert_eq!(
            r.peak,
            Some(PriceTier::full(0.10, 3.0, 9.0)),
            "价格回退预置 flash"
        );
    }

    /// 契约：全字段自定义完全覆盖；model 不匹配预置集时仅作标签。
    #[test]
    fn resolve_full_custom_overrides_everything() {
        let cfg = PricingConfig {
            model: Some("我的定制档".into()),
            timezone_offset_minutes: Some(0),
            windows: Some(vec![]),
            peak: Some(PriceTier::full(1.0, 2.0, 3.0)),
            off_peak: Some(PriceTier::full(0.5, 1.0, 1.5)),
            currency: Some("USD".into()),
        };
        let r = resolve(&deepseek_entry(Some(cfg))).unwrap();
        assert_eq!(r.source, PricingSource::Custom);
        assert_eq!(r.model_label.as_deref(), Some("我的定制档"));
        assert_eq!(r.timezone_offset_minutes, Some(0));
        assert!(r.windows.is_empty(), "显式空窗口 = 恒空闲");
        assert_eq!(r.peak, Some(PriceTier::full(1.0, 2.0, 3.0)));
        assert_eq!(r.currency.as_deref(), Some("USD"));
    }

    /// 契约：全空价格档回退预置（GUI 空表单序列化不破坏回退）。
    #[test]
    fn resolve_empty_tier_falls_back_to_preset() {
        let cfg = PricingConfig {
            peak: Some(PriceTier::default()),
            ..Default::default()
        };
        let r = resolve(&deepseek_entry(Some(cfg))).unwrap();
        assert_eq!(r.peak, Some(PriceTier::full(0.10, 3.0, 9.0)));
        assert!(
            matches!(r.source, PricingSource::Preset { .. }),
            "全空档视为未提供"
        );
    }

    /// 契约：template 条目无自定义 → None；自定义后生效（无预置回退）。
    #[test]
    fn resolve_template_entry() {
        assert_eq!(resolve(&template_entry(None)), None);
        let cfg = PricingConfig {
            windows: Some(vec![PeakWindow {
                days: vec![Weekday::Sun],
                start: "00:00".into(),
                end: "23:59".into(),
            }]),
            peak: Some(PriceTier::full(1.0, 2.0, 3.0)),
            ..Default::default()
        };
        let r = resolve(&template_entry(Some(cfg))).unwrap();
        assert_eq!(r.source, PricingSource::Custom);
        assert_eq!(
            r.timezone_offset_minutes, None,
            "无预置 → 回退 None（本地时区）"
        );
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.currency, None);
        // 判定用固定偏移 0（tz=None 走本地时区，UTC+12+ 的高偏移时区
        // 会把 UTC 周日正午翻到周一导致断言不可移植）
        let sunday = parts_ms(chrono::Weekday::Sun, 12, 0);
        assert_eq!(
            classify(&r.windows, Some(0), sunday),
            PeakKind::Peak,
            "窗口覆盖周日全天，UTC 周日正午必为峰"
        );
    }

    /// 契约：空对象 `"pricing": {}`（= 全字段缺省）视同未配置 → None
    /// （回归：曾在此组合 panic——无预置条目 + Some(PricingConfig::default())
    /// 落进 `preset.expect`）。
    #[test]
    fn resolve_empty_custom_is_none() {
        assert_eq!(
            resolve(&template_entry(Some(PricingConfig::default()))),
            None
        );
        assert_eq!(
            resolve(&deepseek_entry(Some(PricingConfig::default())))
                .unwrap()
                .source,
            PricingSource::Preset {
                native_id: "deepseek".into(),
                model: "flash".into(),
            },
            "有预置时空自定义仍按预置生效"
        );
    }

    /// 契约：旧 config.json（无 pricing 字段）反序列化兼容。
    #[test]
    fn legacy_config_without_pricing_field_parses() {
        let json = r#"[{
            "id": "p1", "name": "DeepSeek",
            "kind": {"type": "native", "provider": "deepseek"},
            "enabled": true
        }]"#;
        let providers: Vec<ProviderEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(providers.len(), 1);
        assert!(providers[0].pricing.is_none());
        assert!(resolve(&providers[0]).is_some(), "预置仍生效");
    }

    /// 契约：仅自定义时区也算 Custom（生效时区是用户值，与 model 标签口径一致）。
    #[test]
    fn resolve_tz_only_counts_as_custom() {
        let cfg = PricingConfig {
            timezone_offset_minutes: Some(0),
            ..Default::default()
        };
        let r = resolve(&deepseek_entry(Some(cfg))).unwrap();
        assert_eq!(r.source, PricingSource::Custom);
        assert_eq!(r.timezone_offset_minutes, Some(0));
        // 价格/时段仍回退预置
        assert_eq!(r.windows.len(), 2);
        assert_eq!(r.peak, Some(PriceTier::full(0.10, 3.0, 9.0)));
    }

    /// 契约：end="24:00" 表达全天窗口——validate 接受、classify 全天命中、
    /// start 处 24:00 被拒。
    #[test]
    fn full_day_window_via_24_00() {
        let all_day = PeakWindow {
            days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
                Weekday::Sat,
                Weekday::Sun,
            ],
            start: "00:00".into(),
            end: "24:00".into(),
        };
        let cfg = PricingConfig {
            windows: Some(vec![all_day]),
            peak: Some(PriceTier::full(1.0, 2.0, 3.0)),
            ..Default::default()
        };
        assert!(validate(&cfg).is_ok(), "24:00 作为 end 应被接受");
        // 任意时刻命中（含 23:59 与 00:00 边界）
        for now in [WED_0930_BJ_MS, WED_0430_BJ_MS, SAT_0840_BJ_MS] {
            assert_eq!(
                classify(cfg.windows.as_ref().unwrap(), None, now),
                PeakKind::Peak,
                "全天窗口应恒峰"
            );
        }
        // start 处 24:00 被拒（start 须早于 end）
        let mut bad = cfg.clone();
        bad.windows.as_mut().unwrap()[0].start = "24:00".into();
        assert!(validate(&bad).is_err());
    }

    /// 契约：ResolvedPricing::kind 便利封装与 classify 一致。
    #[test]
    fn resolved_kind_helper() {
        let r = resolve(&deepseek_entry(None)).unwrap();
        assert_eq!(r.kind(WED_0930_BJ_MS), PeakKind::Peak);
        assert_eq!(r.kind(WED_0430_BJ_MS), PeakKind::OffPeak);
    }
}
