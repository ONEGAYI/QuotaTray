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

    /// 全字段缺失——视为"未提供"，resolve 时回退预置。
    fn is_empty(&self) -> bool {
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

// ---- 预置（官方定价，随版本内置；数据以官网为准） ---------------------------

/// 预置单模型价格档。
#[derive(Debug, Clone, PartialEq)]
pub struct PresetModel {
    /// 模型 id（自定义配置的 `model` 匹配项，如 "flash"）。
    pub id: &'static str,
    /// 展示名（如 "V4 Flash"）。
    pub display: &'static str,
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
    pub windows: Vec<PeakWindow>,
    pub models: Vec<PresetModel>,
    pub default_model: &'static str,
}

/// 按 native id 取预置峰谷定价；无预置 → None。
///
/// DeepSeek 数据抓取自官网定价页（2026-08-23，
/// https://api-docs.deepseek.com/zh-cn/quick_start/pricing/ ，中英文页交叉验证）：
/// 高峰 = 北京时间周一至周五 09:00–12:00、14:00–18:00，空闲价为高峰一半。
pub fn preset(native_id: &str) -> Option<PresetProvider> {
    match native_id {
        "deepseek" => Some(PresetProvider {
            native_id: "deepseek",
            currency: "CNY",
            timezone_offset_minutes: 480,
            windows: vec![
                peak_window_workday("09:00", "12:00"),
                peak_window_workday("14:00", "18:00"),
            ],
            models: vec![
                PresetModel {
                    id: "flash",
                    display: "V4 Flash",
                    peak: PriceTier::full(0.10, 3.0, 9.0),
                    off_peak: PriceTier::full(0.05, 1.5, 4.5),
                },
                PresetModel {
                    id: "pro",
                    display: "V4 Pro",
                    peak: PriceTier::full(0.30, 9.0, 27.0),
                    off_peak: PriceTier::full(0.15, 4.5, 13.5),
                },
                PresetModel {
                    id: "vision",
                    display: "V4 Flash Vision Exp",
                    peak: PriceTier::full(0.10, 3.0, 9.0),
                    off_peak: PriceTier::full(0.05, 1.5, 4.5),
                },
            ],
            default_model: "flash",
        }),
        _ => None,
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

/// 解析条目生效定价：无可展示内容（无预置，且自定义为空或缺失）→ None。
///
/// 合并规则：`model` 匹配预置模型 id（大小写不敏感）则选定该模型，否则
/// 仅作展示标签（价格回退默认模型）；`windows`/`peak`/`off_peak`/`currency`/
/// `timezone_offset_minutes` 自定义非空即覆盖，否则回退预置；来源以
/// 「是否有任何自定义值生效」判定（时区与 model 标签同口径）。
pub fn resolve(entry: &ProviderEntry) -> Option<ResolvedPricing> {
    let preset = match &entry.kind {
        ProviderKind::Native { provider } => preset(provider),
        _ => None,
    };
    let custom = entry.pricing.as_ref();
    if preset.is_none() && custom.is_none_or(|c| c.is_empty()) {
        return None;
    }

    // 模型选择：自定义 id 匹配预置模型 → 该模型；否则默认模型 + 自定义标签
    let (model, model_label, model_from_custom) =
        match (&preset, custom.and_then(|c| c.model.as_deref())) {
            (Some(p), Some(id)) => {
                let matched = p.models.iter().find(|m| m.id.eq_ignore_ascii_case(id));
                match matched {
                    Some(m) => (Some(m.clone()), Some(m.display.into()), false),
                    None => {
                        let default = p.models.iter().find(|m| m.id == p.default_model);
                        (default.cloned(), Some(id.into()), true)
                    }
                }
            }
            (Some(p), None) => {
                let default = p.models.iter().find(|m| m.id == p.default_model);
                (default.cloned(), default.map(|m| m.display.into()), false)
            }
            (None, Some(id)) => (None, Some(id.into()), true),
            (None, None) => (None, None, false),
        };

    let tier_or = |custom_tier: Option<&PriceTier>, preset_tier: Option<PriceTier>| {
        custom_tier
            .filter(|t| !t.is_empty())
            .cloned()
            .or(preset_tier)
    };
    let windows = custom
        .and_then(|c| c.windows.clone())
        .or_else(|| preset.as_ref().map(|p| p.windows.clone()))
        .unwrap_or_default();
    let peak = tier_or(
        custom.and_then(|c| c.peak.as_ref()),
        model.as_ref().map(|m| m.peak.clone()),
    );
    let off_peak = tier_or(
        custom.and_then(|c| c.off_peak.as_ref()),
        model.as_ref().map(|m| m.off_peak.clone()),
    );
    let currency = custom
        .and_then(|c| c.currency.clone())
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
                .map(|m| m.id.into())
                .unwrap_or_else(|| p.default_model.into()),
        },
        // 有自定义生效 / 无预置但自定义非空（is_empty 已在入口拦截空对象）
        _ => PricingSource::Custom,
    };

    Some(ResolvedPricing {
        timezone_offset_minutes: custom
            .and_then(|c| c.timezone_offset_minutes)
            .or_else(|| preset.as_ref().map(|p| p.timezone_offset_minutes)),
        windows,
        peak,
        off_peak,
        currency,
        model_label,
        source,
    })
}

// ---- 测试 ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
