//! 托盘圆环图标：层计算与中心文字纯函数 + tiny-skia 绘制（32x32 RGBA）。
//!
//! 视觉规格与 `docs/design/tray-ring-demo.html` 定案一致：
//! - 百分比型（unit="%" 或 used/total 可算）→ 永远单弧，fill = 剩余比例；
//! - 不定额余额型 → `remaining / 每圈单位` 分层：整数部分逐层满圈、
//!   余数顶层弧；余额消耗到最后一圈（单层）才用阈值色；
//! - 多层 → 全部层走预设色循环（绿→蓝→紫→青→粉→靛，第 n 层取 (n-1)%6）；
//! - 层数上限 4，超出 → 溢出样式：满环取第 4 层循环色（青）+ 中心缩写；
//! - 阈值色：fill∈[0,1] 连续渐变 HSL(hue=fill*120, 78%, 48%)。
//!
//! 上半部纯函数由契约测试锁定形状；绘制部分行为由输出形状测试与烟测覆盖。

use std::collections::HashMap;

use quota_core::{AppConfig, UsageData};

use crate::settings::Settings;
use crate::state::{EntryState, now_ms};

/// 图标画布边长（像素）。
pub const ICON_SIZE: u32 = 32;
/// 层数上限：需要第 5 层即溢出（满环 + 中心缩写）。
pub const MAX_LAYERS: usize = 4;

// ---- 层计算（纯函数） -------------------------------------------------------

/// 圆环的数据来源分类。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RingInput {
    /// 百分比型：剩余百分比（0-100）。
    Percent { remaining_pct: f64 },
    /// 不定额余额型：剩余数值。
    Balance { remaining: f64 },
    /// 无可用数据（灰空环）。
    Empty,
}

/// 从单窗口用量数据取圆环输入：先试百分比（与 `tray::used_percent`
/// 同语义：unit="%" 或 used/total 可算），再试余额。
fn datum_ring_input(d: &UsageData) -> RingInput {
    if let Some(used) = crate::tray::used_percent(d) {
        return RingInput::Percent {
            remaining_pct: (100.0 - used).clamp(0.0, 100.0),
        };
    }
    match d.remaining {
        Some(rem) if rem.is_finite() => RingInput::Balance {
            remaining: rem.max(0.0),
        },
        _ => RingInput::Empty,
    }
}

/// 从多窗口数据取圆环输入：取第一个可用窗口（跳过 is_valid=false 与无值窗口）。
pub fn data_ring_input(data: &[UsageData]) -> RingInput {
    data.iter()
        .filter(|d| d.is_valid != Some(false))
        .map(datum_ring_input)
        .find(|input| !matches!(input, RingInput::Empty))
        .unwrap_or(RingInput::Empty)
}

/// 条目状态 → 圆环输入。展示门控与菜单行/红点共用
/// `tray::state_is_displayable`（确定性失败或超窗瞬时失败不展示旧值）。
pub fn entry_ring_input(st: &EntryState, now: u64) -> RingInput {
    if !crate::tray::state_is_displayable(st, now) {
        return RingInput::Empty;
    }
    st.data
        .as_deref()
        .map(data_ring_input)
        .unwrap_or(RingInput::Empty)
}

/// 余额分层（demo `layerize` 同算法）：返回（底→顶的 fill 数组, 是否溢出）。
///
/// 溢出条件：`full + (frac>0)` 超 [`MAX_LAYERS`]。value ≤ 0 → 单层 0 弧
/// （阈值红、弧不可见，中心写 0）；恰好整除 → 全满层。
pub fn layerize_balance(value: f64, per_ring: f64) -> (Vec<f64>, bool) {
    if !value.is_finite() || value <= 0.0 {
        return (vec![0.0], false);
    }
    let per_ring = per_ring.max(1.0);
    let full = (value / per_ring).floor();
    let frac = value / per_ring - full;
    let total = full as usize + usize::from(frac > 0.0);
    if total > MAX_LAYERS {
        return (Vec::new(), true);
    }
    let mut layers = vec![1.0; full as usize];
    if frac > 0.0 {
        layers.push(frac);
    }
    if layers.is_empty() {
        layers.push(1.0); // 兜底：正常路径不会到达（frac>0 已入数组）
    }
    (layers, false)
}

/// 圆环规格（纯数据，绘制与测试共用）。
#[derive(Debug, Clone, PartialEq)]
pub struct RingSpec {
    /// 底→顶每层 fill（0..1）；溢出时为空。
    pub layers: Vec<f64>,
    /// 层数超上限（绘制为满环 + 第 4 层色）。
    pub overflow: bool,
    /// 单层语义（阈值色）：百分比型恒真；余额型仅最后一圈为真。
    pub single: bool,
    /// 中心文字；None = 无数据灰空环（不写字）。
    pub center: Option<String>,
}

/// 数据 + 每圈单位 → 圆环规格。
pub fn ring_spec(input: RingInput, per_ring: f64) -> RingSpec {
    match input {
        RingInput::Percent { remaining_pct } => RingSpec {
            layers: vec![remaining_pct / 100.0],
            overflow: false,
            single: true,
            center: Some(center_text_percent(remaining_pct)),
        },
        RingInput::Balance { remaining } => {
            let (layers, overflow) = layerize_balance(remaining, per_ring);
            let single = !overflow && layers.len() == 1;
            RingSpec {
                single,
                overflow,
                center: Some(center_text_balance(remaining)),
                layers,
            }
        }
        RingInput::Empty => RingSpec {
            layers: Vec::new(),
            overflow: false,
            single: false,
            center: None,
        },
    }
}

// ---- 颜色（纯函数） ---------------------------------------------------------

/// 预设色循环（第 n 层取 PRESET[(n-1)%6]）：绿、蓝、紫、青、粉、靛。
pub const PRESET: [(u8, u8, u8); 6] = [
    (0x22, 0xc5, 0x5e), // 绿
    (0x3b, 0x82, 0xf6), // 蓝
    (0x8b, 0x5c, 0xf6), // 紫
    (0x06, 0xb6, 0xd4), // 青
    (0xec, 0x48, 0x99), // 粉
    (0x63, 0x66, 0xf1), // 靛
];

/// HSL → RGB（h 度、s/l ∈ 0..1）。
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h / 60.0).clamp(0.0, 5.999);
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

/// 阈值色：fill∈[0,1] 连续渐变（1=绿 hue120、0=红 hue0；demo `thresholdColor`）。
pub fn threshold_color(fill: f64) -> (u8, u8, u8) {
    hsl_to_rgb(fill.clamp(0.0, 1.0) * 120.0, 0.78, 0.48)
}

/// 第 idx 层（0-based，底→顶）的绘制色。
///
/// 溢出 → 第 4 层循环色（青）；单层 → 阈值色；多层 → 预设色循环。
pub fn layer_color(spec: &RingSpec, idx: usize) -> (u8, u8, u8) {
    if spec.overflow {
        PRESET[3]
    } else if spec.single {
        threshold_color(spec.layers.first().copied().unwrap_or(0.0))
    } else {
        PRESET[idx % PRESET.len()]
    }
}

// ---- 中心文字（纯函数） -----------------------------------------------------

/// 余额中心文字（≤4 字符）：<10000 直接整数；k/M/B 缩写，≥10 取整、
/// <10 保留 1 位小数（向下截断，与 demo `centerText` 一致）。
pub fn center_text_balance(value: f64) -> String {
    let v = value.round();
    if !v.is_finite() || v <= 0.0 {
        return "0".into();
    }
    if v < 10_000.0 {
        return format!("{v:.0}");
    }
    if v < 1e6 {
        scaled(v, 1e3, 'k')
    } else if v < 1e9 {
        scaled(v, 1e6, 'M')
    } else {
        scaled(v, 1e9, 'B')
    }
}

/// 数值按 `unit` 缩写：`12k`（≥10 取整）/ `1.2k`（<10 一位小数向下截断，
/// 恰为整数时不带小数点——与 demo 的 JS 数字 toString 一致：`2B`）。
fn scaled(v: f64, unit: f64, suffix: char) -> String {
    let q = v / unit;
    if q >= 10.0 {
        format!("{}{suffix}", q.round() as u64)
    } else {
        let t = (q * 10.0).floor() / 10.0;
        if t == t.trunc() {
            format!("{}{suffix}", t as u64)
        } else {
            format!("{t:.1}{suffix}")
        }
    }
}

/// 百分比中心文字：`45%`（round 后 0-100 封顶）。
pub fn center_text_percent(remaining_pct: f64) -> String {
    format!("{}%", remaining_pct.round().clamp(0.0, 100.0) as u32)
}

// ---- 绘制（32x32 RGBA 直通） -----------------------------------------------

/// 环几何参数（与 demo 的 viewBox64/stroke14 等比缩放到 32px）。
const CENTER: f32 = 16.0;
const RADIUS: f32 = 12.5; // (32 - 7) / 2，环宽 7px
const STROKE: f32 = 7.0;

/// 4x6 位图字模（每行 4 bit，MSB 在左）：0-9 k M B % . 共 15 字形。
/// 不引入字体库——32px 图标中心区约 18px 宽，1x 像素字模刚好容纳 4 字符。
const GLYPHS: [(char, [u8; 6]); 15] = [
    ('0', [0b0110, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110]),
    ('1', [0b0010, 0b0110, 0b0010, 0b0010, 0b0010, 0b0110]),
    ('2', [0b0110, 0b1001, 0b0001, 0b0010, 0b0100, 0b1111]),
    ('3', [0b1110, 0b0001, 0b0010, 0b0001, 0b0001, 0b1110]),
    ('4', [0b0001, 0b0010, 0b0100, 0b1111, 0b0001, 0b0001]),
    ('5', [0b1111, 0b1000, 0b1110, 0b0001, 0b1001, 0b0110]),
    ('6', [0b0110, 0b1000, 0b1110, 0b1001, 0b1001, 0b0110]),
    ('7', [0b1111, 0b0001, 0b0010, 0b0100, 0b0100, 0b0100]),
    ('8', [0b0110, 0b1001, 0b0110, 0b1001, 0b1001, 0b0110]),
    ('9', [0b0110, 0b1001, 0b1001, 0b0111, 0b0001, 0b0110]),
    ('k', [0b1000, 0b1000, 0b1001, 0b1010, 0b1100, 0b1001]),
    ('M', [0b1001, 0b1111, 0b1111, 0b1001, 0b1001, 0b1001]),
    ('B', [0b1110, 0b1001, 0b1110, 0b1001, 0b1001, 0b1110]),
    ('%', [0b1001, 0b0010, 0b0100, 0b1000, 0b0100, 0b1001]),
    ('.', [0b0000, 0b0000, 0b0000, 0b0000, 0b0000, 0b0110]),
];

fn glyph(ch: char) -> Option<&'static [u8; 6]> {
    GLYPHS.iter().find(|(c, _)| *c == ch).map(|(_, rows)| rows)
}

/// 渲染为 32x32 直通 RGBA 字节（长度 32*32*4）。
///
/// 颜色按解析后主题取两套：dark 用浅色文字（浅任务栏反之）；
/// 底槽两套灰度均来自 demo 的 rgba(128,140,160,0.28) 量级。
pub fn render_rgba(spec: &RingSpec, dark: bool, alert: bool) -> Vec<u8> {
    let mut pm = tiny_skia::Pixmap::new(ICON_SIZE, ICON_SIZE).expect("32x32 画布分配失败");

    // 1. 环底槽（未填充部分）：分主题两套（已回写 demo「已定案」清单）——
    //    light 略加深以保证浅背景上的空环可见
    let slot = if dark {
        tiny_skia::Color::from_rgba8(128, 140, 160, 71) // ≈0.28 alpha
    } else {
        tiny_skia::Color::from_rgba8(100, 116, 139, 89) // ≈0.35 alpha
    };
    stroke_circle(&mut pm, 1.0, slot);

    // 2. 数据弧：溢出 → 满环第 4 层色；否则底→顶叠弧（12 点起顺时针）
    if spec.overflow {
        stroke_circle(&mut pm, 1.0, opaque(PRESET[3]));
    } else {
        for (i, fill) in spec.layers.iter().enumerate() {
            if *fill <= 0.0 {
                continue; // 0 弧不可见（余额归零：只剩底槽 + 中心 0）
            }
            stroke_circle(&mut pm, *fill as f32, opaque(layer_color(spec, i)));
        }
    }

    // 3. 告警红点（badge 惯例：压在环右上角）
    if alert {
        fill_alert_dot(&mut pm);
    }

    // 4. 中心文字（1x 字模像素，不抗锯齿）
    if let Some(text) = &spec.center {
        let color = if dark {
            (0xe6, 0xea, 0xf2) // demo dark: #e6eaf2
        } else {
            (0x1e, 0x29, 0x3b) // demo light: #1e293b
        };
        draw_center_text(&mut pm, text, color);
    }

    premultiplied_to_straight(pm.data())
}

fn opaque((r, g, b): (u8, u8, u8)) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(r, g, b, 255)
}

fn solid_paint(color: tiny_skia::Color) -> tiny_skia::Paint<'static> {
    tiny_skia::Paint {
        anti_alias: true,
        shader: tiny_skia::Shader::SolidColor(color),
        ..Default::default()
    }
}

/// 从 12 点顺时针画 `frac` 比例的弧（frac=1 即整圆）。
fn stroke_circle(pm: &mut tiny_skia::Pixmap, frac: f32, color: tiny_skia::Color) {
    let mut pb = tiny_skia::PathBuilder::new();
    let steps = (frac * 96.0).ceil().max(2.0) as usize;
    for i in 0..=steps {
        let theta =
            -std::f32::consts::FRAC_PI_2 + (i as f32 / steps as f32) * frac * std::f32::consts::TAU;
        let (sin, cos) = theta.sin_cos();
        let (x, y) = (CENTER + RADIUS * cos, CENTER + RADIUS * sin);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    let Some(path) = pb.finish() else { return };
    let stroke = tiny_skia::Stroke {
        width: STROKE,
        ..Default::default()
    };
    pm.stroke_path(
        &path,
        &solid_paint(color),
        &stroke,
        tiny_skia::Transform::identity(),
        None,
    );
}

fn fill_alert_dot(pm: &mut tiny_skia::Pixmap) {
    let (cx, cy, r) = (26.5f32, 5.5f32, 2.5f32);
    let mut pb = tiny_skia::PathBuilder::new();
    for i in 0..=24 {
        let theta = (i as f32 / 24.0) * std::f32::consts::TAU;
        let (sin, cos) = theta.sin_cos();
        let (x, y) = (cx + r * cos, cy + r * sin);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    let Some(path) = pb.finish() else { return };
    pm.fill_path(
        &path,
        &solid_paint(tiny_skia::Color::from_rgba8(0xef, 0x44, 0x44, 255)),
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        None,
    );
}

/// 中心文字：1x 像素字模逐点覆盖（不透明，无字模的字符跳过）。
fn draw_center_text(pm: &mut tiny_skia::Pixmap, text: &str, (r, g, b): (u8, u8, u8)) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let total_w = 4 * n + n.saturating_sub(1);
    let x0 = ICON_SIZE as i32 - total_w as i32;
    let x0 = (x0 / 2).max(0) as u32;
    let y0 = 13u32; // 6 行高，中心 16 → 顶行 13
    let px =
        tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, 255).expect("不透明色的预乘形式恒合法");
    for (ci, ch) in chars.iter().enumerate() {
        let Some(rows) = glyph(*ch) else { continue };
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..4u32 {
                if bits & (0b1000 >> col) != 0 {
                    let x = x0 + ci as u32 * 5 + col;
                    let y = y0 + row as u32;
                    if x < ICON_SIZE && y < ICON_SIZE {
                        pm.pixels_mut()[(y * ICON_SIZE + x) as usize] = px;
                    }
                }
            }
        }
    }
}

/// tiny-skia 输出预乘 RGBA，托盘 Image 需要直通：逐像素除回 alpha。
fn premultiplied_to_straight(src: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    let (src_pixels, _) = src.as_chunks::<4>();
    let (out_pixels, _) = out.as_chunks_mut::<4>();
    for (s, d) in src_pixels.iter().zip(out_pixels.iter_mut()) {
        let a = u32::from(s[3]);
        if a == 0 || a == 255 {
            d[..3].copy_from_slice(&s[..3]);
            d[3] = s[3];
            continue;
        }
        d[0] = ((u32::from(s[0]) * 255 + a / 2) / a) as u8;
        d[1] = ((u32::from(s[1]) * 255 + a / 2) / a) as u8;
        d[2] = ((u32::from(s[2]) * 255 + a / 2) / a) as u8;
        d[3] = s[3];
    }
    out
}

// ---- 托盘集成入口 -----------------------------------------------------------

/// 图标数据源条目：settings 指定且仍 enabled；None / id 失效 → 第一个 enabled。
pub fn icon_entry<'a>(
    cfg: &'a AppConfig,
    settings: &Settings,
) -> Option<&'a quota_core::ProviderEntry> {
    let specified = settings
        .tray_icon_entry_id
        .as_deref()
        .and_then(|id| cfg.providers.iter().find(|p| p.id == id && p.enabled));
    specified.or_else(|| cfg.providers.iter().find(|p| p.enabled))
}

/// 渲染托盘圆环图标（数据源 = icon_entry 的最近查询结果）。
///
/// 安全红线：只消费已脱敏的结果表数据（余额/百分比），不触碰凭据。
pub fn icon_image(
    cfg: &AppConfig,
    results: &HashMap<String, EntryState>,
    settings: &Settings,
    dark: bool,
    alert: bool,
) -> tauri::image::Image<'static> {
    let now = now_ms();
    let input = icon_entry(cfg, settings)
        .and_then(|e| results.get(&e.id))
        .map(|st| entry_ring_input(st, now))
        .unwrap_or(RingInput::Empty);
    let spec = ring_spec(input, settings.ring_units_per_circle);
    let rgba = render_rgba(&spec, dark, alert);
    tauri::image::Image::new_owned(rgba, ICON_SIZE, ICON_SIZE)
}

// ---- 契约测试 ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use quota_core::PlanVariant;

    fn feq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn assert_layers(layers: &[f64], expected: &[f64]) {
        assert_eq!(layers.len(), expected.len(), "层数不符：{layers:?}");
        for (i, (a, b)) in layers.iter().zip(expected).enumerate() {
            assert!(feq(*a, *b), "第 {i} 层 fill：{a} != {b}");
        }
    }

    fn balance_data(remaining: f64) -> UsageData {
        UsageData {
            remaining: Some(remaining),
            ..Default::default()
        }
    }

    /// 契约：余额分层——单层/多层/4 层上限/溢出（demo 数值例）。
    #[test]
    fn balance_layerize_rules() {
        let (l, ov) = layerize_balance(60.0, 100.0);
        assert!(!ov);
        assert_layers(&l, &[0.6]);

        let (l, ov) = layerize_balance(180.0, 100.0);
        assert!(!ov);
        assert_layers(&l, &[1.0, 0.8]);

        let (l, ov) = layerize_balance(250.0, 100.0);
        assert!(!ov);
        assert_layers(&l, &[1.0, 1.0, 0.5]);

        let (l, ov) = layerize_balance(380.0, 100.0);
        assert!(!ov, "¥380 = 3 满 + 0.8 顶层 = 恰好 4 层上限");
        assert_layers(&l, &[1.0, 1.0, 1.0, 0.8]);

        // 注：任务清单写「¥390→溢出」，但 demo layerize（视觉规格基准）
        // 中 390 = 3 满 + 0.9 顶层 = 4 层不溢出；溢出从需要第 5 层起。
        let (l, ov) = layerize_balance(390.0, 100.0);
        assert!(!ov);
        assert_layers(&l, &[1.0, 1.0, 1.0, 0.9]);

        let (l, ov) = layerize_balance(400.0, 100.0);
        assert!(!ov, "整除 4 圈 = 4 满层，仍未溢出");
        assert_layers(&l, &[1.0, 1.0, 1.0, 1.0]);

        for v in [400.01, 401.0, 500.0, 1250.0, 311_000.0] {
            let (l, ov) = layerize_balance(v, 100.0);
            assert!(ov, "{v} 应溢出");
            assert!(l.is_empty());
        }

        // 每圈单位可调：¥250 每圈 500 → 单层 0.5
        let (l, ov) = layerize_balance(250.0, 500.0);
        assert!(!ov);
        assert_layers(&l, &[0.5]);

        // 0 / 负数 → 单层 0 弧
        let (l, ov) = layerize_balance(0.0, 100.0);
        assert!(!ov);
        assert_layers(&l, &[0.0]);
    }

    /// 契约：百分比型永远单层阈值色，fill = 剩余比例。
    #[test]
    fn percent_spec_single_layer() {
        let s = ring_spec(
            RingInput::Percent {
                remaining_pct: 55.0,
            },
            100.0,
        );
        assert!(s.single);
        assert!(!s.overflow);
        assert_layers(&s.layers, &[0.55]);
        assert_eq!(s.center.as_deref(), Some("55%"));
    }

    /// 契约：颜色规则——单层阈值色随 fill 变化、多层预设循环、溢出青。
    #[test]
    fn color_rules() {
        // 阈值色端点：fill=1 绿（G 分量占优）、fill=0 红（R 分量占优）
        let (r1, g1, b1) = threshold_color(1.0);
        assert!(g1 > r1 && g1 > b1, "fill=1 应偏绿：{r1},{g1},{b1}");
        let (r0, g0, b0) = threshold_color(0.0);
        assert!(r0 > g0 && r0 > b0, "fill=0 应偏红：{r0},{g0},{b0}");
        // 中点应黄（R≈G 且都高于 B）
        let (rm, gm, bm) = threshold_color(0.5);
        assert!((i32::from(rm) - i32::from(gm)).abs() < 8, "fill=0.5 应偏黄");
        assert!(rm > bm && gm > bm);

        // 预设色循环：第 n 层（1-based）取 (n-1)%6
        assert_eq!(PRESET[0], (0x22, 0xc5, 0x5e));
        assert_eq!(PRESET[3], (0x06, 0xb6, 0xd4));
        let multi = ring_spec(RingInput::Balance { remaining: 380.0 }, 100.0);
        assert!(!multi.single);
        assert_eq!(layer_color(&multi, 0), PRESET[0], "第 1 层绿");
        assert_eq!(layer_color(&multi, 1), PRESET[1], "第 2 层蓝");
        assert_eq!(layer_color(&multi, 2), PRESET[2], "第 3 层紫");
        assert_eq!(layer_color(&multi, 3), PRESET[3], "第 4 层青");
        // 第 7 层（若放宽上限）回到绿——循环语义
        assert_eq!(layer_color(&multi, 6), PRESET[0]);

        // 溢出：满环取第 4 层循环色（青）
        let over = ring_spec(RingInput::Balance { remaining: 1250.0 }, 100.0);
        assert!(over.overflow);
        assert_eq!(layer_color(&over, 0), PRESET[3]);

        // 单层余额（最后一圈）→ 阈值色
        let last = ring_spec(RingInput::Balance { remaining: 60.0 }, 100.0);
        assert!(last.single);
        assert_eq!(layer_color(&last, 0), threshold_color(0.6));
    }

    /// 契约：中心文字缩写全分支（demo 数值例）。
    #[test]
    fn center_text_abbreviations() {
        assert_eq!(center_text_balance(9999.0), "9999");
        assert_eq!(center_text_balance(10000.0), "10k");
        assert_eq!(center_text_balance(12499.0), "12k", "12.499k ≥10 取整");
        assert_eq!(
            center_text_balance(1250.0),
            "1250",
            "<10000 直接整数（4 字符内）"
        );
        // 注：demo 的 "1.2k" 分支（k<10）实际不可达——该值域已走 <10000 整数档；
        // 一位小数缩写只在 M/B 档出现（1.2M 等），下方断言覆盖。
        assert_eq!(center_text_balance(311_000.0), "311k");
        assert_eq!(center_text_balance(1.2e6), "1.2M");
        assert_eq!(center_text_balance(2e9), "2B");
        assert_eq!(center_text_balance(62.97), "63", "四舍五入为整数");
        assert_eq!(center_text_balance(0.0), "0");
        assert_eq!(center_text_percent(45.4), "45%");
        assert_eq!(center_text_percent(100.0), "100%");
        assert_eq!(center_text_percent(120.0), "100%", "封顶 100");
        assert_eq!(center_text_percent(-3.0), "0%");
    }

    /// 契约：数据 → 圆环输入分类（百分比优先、余额兜底、失效窗口跳过）。
    #[test]
    fn ring_input_classification() {
        let mut d = UsageData {
            used: Some(45.0),
            unit: Some("%".into()),
            ..Default::default()
        };
        assert_eq!(
            data_ring_input(&[d.clone()]),
            RingInput::Percent {
                remaining_pct: 55.0
            }
        );

        d = UsageData {
            used: Some(40.0),
            total: Some(200.0),
            ..Default::default()
        };
        assert_eq!(
            data_ring_input(&[d]),
            RingInput::Percent {
                remaining_pct: 80.0
            },
            "used/total 可算 → 百分比型（40/200 = 20% 已用 → 80% 剩余）"
        );

        assert_eq!(
            data_ring_input(&[balance_data(180.0)]),
            RingInput::Balance { remaining: 180.0 }
        );

        // 多窗口：失效窗口跳过，取第一个可用
        let invalid = UsageData {
            is_valid: Some(false),
            remaining: Some(999.0),
            ..Default::default()
        };
        assert_eq!(
            data_ring_input(&[invalid, balance_data(60.0)]),
            RingInput::Balance { remaining: 60.0 }
        );

        // 无值窗口跳过后无可用 → Empty
        assert_eq!(data_ring_input(&[UsageData::default()]), RingInput::Empty);
    }

    /// 契约：条目状态门控——确定性失败/超窗瞬时失败不展示旧值。
    #[test]
    fn entry_ring_input_gating() {
        let now = 1_755_000_000_000u64;
        let good = EntryState {
            data: Some(vec![balance_data(180.0)]),
            at: Some(now - 60_000),
            error: None,
        };
        assert_eq!(
            entry_ring_input(&good, now),
            RingInput::Balance { remaining: 180.0 }
        );

        let det = EntryState {
            error: Some(crate::state::ErrorInfo {
                kind: "deterministic".into(),
                message: "401".into(),
                detail: None,
            }),
            ..good.clone()
        };
        assert_eq!(entry_ring_input(&det, now), RingInput::Empty);

        let mut transient = good.clone();
        transient.error = Some(crate::state::ErrorInfo {
            kind: "transient".into(),
            message: "timeout".into(),
            detail: None,
        });
        assert_eq!(
            entry_ring_input(&transient, now),
            RingInput::Balance { remaining: 180.0 },
            "窗口内 keep-last-good 仍展示旧值"
        );
        assert_eq!(
            entry_ring_input(&transient, now + 11 * 60_000),
            RingInput::Empty,
            "超窗后旧值不再展示"
        );
    }

    /// 契约：图标配色与解析后主题相关（dark 浅文字 / light 深文字）。
    #[test]
    fn render_theme_variants() {
        let spec = ring_spec(RingInput::Balance { remaining: 60.0 }, 100.0);
        let dark = render_rgba(&spec, true, false);
        let light = render_rgba(&spec, false, false);
        assert_eq!(dark.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert_ne!(dark, light, "明暗主题应产出不同像素");
    }

    /// 契约：RGBA 输出形状——32*32*4、非全透明（弧/槽可见）。
    #[test]
    fn render_output_shape() {
        for input in [
            RingInput::Balance { remaining: 60.0 },
            RingInput::Balance { remaining: 1250.0 }, // 溢出
            RingInput::Percent {
                remaining_pct: 45.0,
            },
            RingInput::Empty, // 灰空环：槽仍可见
        ] {
            let spec = ring_spec(input, 100.0);
            let rgba = render_rgba(&spec, true, false);
            assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
            assert!(
                rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
                "{input:?} 应有可见像素"
            );
            // 直通格式的输出契约：a==0 时 RGB 亦为 0（全透明像素无颜色信息）
            for p in rgba.as_chunks::<4>().0 {
                if p[3] == 0 {
                    assert_eq!(&p[..3], &[0, 0, 0], "全透明像素 RGB 应为 0");
                }
            }
        }
    }

    /// 契约：alert 红点改变右上角区域像素。
    #[test]
    fn render_alert_dot() {
        let spec = ring_spec(RingInput::Balance { remaining: 60.0 }, 100.0);
        let plain = render_rgba(&spec, true, false);
        let alert = render_rgba(&spec, true, true);
        assert_ne!(plain, alert);
        // 红点中心 (26.5, 5.5) → 像素 (26, 5) 应为红色系
        let idx = ((5 * ICON_SIZE + 26) * 4) as usize;
        assert_eq!(&alert[idx..idx + 3], &[0xef, 0x44, 0x44]);
    }

    /// 契约：icon_entry 回退规则——指定 id 优先（须 enabled），失效回第一个。
    #[test]
    fn icon_entry_fallback() {
        use quota_core::{ProviderEntry, ProviderKind};
        let entry = |id: &str, enabled: bool| ProviderEntry {
            id: id.into(),
            name: id.into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled,
            api_key_enc: None,
            api_key2_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        };
        let cfg = AppConfig {
            custom_models: Default::default(),
            providers: vec![entry("a", true), entry("b", true), entry("c", false)],
        };
        let mut settings = Settings::default();

        assert_eq!(
            icon_entry(&cfg, &settings).map(|e| e.id.as_str()),
            Some("a")
        );

        settings.tray_icon_entry_id = Some("b".into());
        assert_eq!(
            icon_entry(&cfg, &settings).map(|e| e.id.as_str()),
            Some("b")
        );

        settings.tray_icon_entry_id = Some("c".into());
        assert_eq!(
            icon_entry(&cfg, &settings).map(|e| e.id.as_str()),
            Some("a"),
            "disabled 条目不作数据源，回退第一个 enabled"
        );

        settings.tray_icon_entry_id = Some("gone".into());
        assert_eq!(
            icon_entry(&cfg, &settings).map(|e| e.id.as_str()),
            Some("a"),
            "已删除条目的 stale id 回退"
        );
    }

    /// 契约：多层叠弧的覆盖关系——最上层弧覆盖区间取本层色，
    /// 未覆盖的尾部露出下层色（380 = 绿满+蓝满+紫满+青 80%：
    /// 12 点起顺时针 80% 为青，尾部 1/4 露紫）。
    #[test]
    fn render_layer_stack_reveals_lower_layer() {
        let spec = ring_spec(RingInput::Balance { remaining: 380.0 }, 100.0);
        let rgba = render_rgba(&spec, true, false);
        let px = |x: u32, y: u32| -> (i32, i32, i32) {
            let i = ((y * ICON_SIZE + x) * 4) as usize;
            (rgba[i] as i32, rgba[i + 1] as i32, rgba[i + 2] as i32)
        };
        let near = |got: (i32, i32, i32), want: (u8, u8, u8), what: &str| {
            let d = |a: i32, b: u8| (a - i32::from(b)).abs();
            assert!(
                d(got.0, want.0) < 40 && d(got.1, want.1) < 40 && d(got.2, want.2) < 40,
                "{what}：got {got:?} want {want:?}（抗锯齿容差 40）"
            );
        };
        // 12 点方向（青弧起点）：像素 (16, 4) ≈ (16, 16-12.5)
        near(px(16, 4), (0x06, 0xb6, 0xd4), "顶层青应覆盖弧区");
        // 3 点方向（弧中段）：像素 (28, 16) ≈ (16+12.5, 16)
        near(px(28, 16), (0x06, 0xb6, 0xd4), "弧中段应为顶层青");
        // 尾部中点（θ=234°）：像素 ≈ (16-7.35, 16-10.11) = (8.65, 5.89)
        near(px(9, 6), (0x8b, 0x5c, 0xf6), "尾部应露出下层紫");
    }

    /// 手动视觉检查工具（默认 ignore）：把多组余额/百分比在明暗两种主题下的
    /// 圆环渲染拼成一张 PNG（模拟任务栏底色），供人工对照
    /// docs/design/tray-ring-demo.html 的画廊核对视觉。
    /// 运行：`cargo test -p quota-desktop --lib render_ring_preview_png -- --ignored --nocapture`
    #[test]
    #[ignore = "手动视觉检查工具，CI 不跑"]
    fn render_ring_preview_png() {
        const CELL: u32 = 44; // 32 图标 + 12 边距
        let balance_cases = [60.0, 30.0, 80.0, 180.0, 250.0, 380.0, 1250.0, 311_000.0];
        let percent_cases = [85.0, 45.0, 12.0];
        let cols = (balance_cases.len() + percent_cases.len()) as u32;
        let width = CELL * cols;
        let height = CELL * 2; // 两行：dark / light 任务栏底色
        let mut canvas = tiny_skia::Pixmap::new(width, height).expect("预览画布分配失败");
        let fill_bg = |pm: &mut tiny_skia::Pixmap, dark: bool| {
            let (r, g, b) = if dark {
                (0x1b, 0x1f, 0x27)
            } else {
                (0xee, 0xf1, 0xf6)
            };
            pm.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
        };
        for (row, dark) in [true, false].into_iter().enumerate() {
            fill_bg(&mut canvas, dark);
            let mut draw = |col: usize, input: RingInput| {
                let spec = ring_spec(input, 100.0);
                let rgba = render_rgba(&spec, dark, false);
                // 直通 → 预乘（Pixmap::from_vec 要求）
                let mut premult = rgba.clone();
                let (premult_pixels, _) = premult.as_chunks_mut::<4>();
                let (rgba_pixels, _) = rgba.as_chunks::<4>();
                for (d, s) in premult_pixels.iter_mut().zip(rgba_pixels.iter()) {
                    let a = u32::from(s[3]);
                    d[0] = ((u32::from(s[0]) * a + 127) / 255) as u8;
                    d[1] = ((u32::from(s[1]) * a + 127) / 255) as u8;
                    d[2] = ((u32::from(s[2]) * a + 127) / 255) as u8;
                    d[3] = s[3];
                }
                let icon = tiny_skia::Pixmap::from_vec(
                    premult,
                    tiny_skia::IntSize::from_wh(ICON_SIZE, ICON_SIZE).expect("尺寸合法"),
                )
                .expect("图标画布重建失败");
                let x = (col as u32 * CELL + (CELL - ICON_SIZE) / 2) as i32;
                let y = (row as u32 * CELL + (CELL - ICON_SIZE) / 2) as i32;
                canvas.draw_pixmap(
                    x,
                    y,
                    icon.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::identity(),
                    None,
                );
            };
            for (col, v) in balance_cases.iter().enumerate() {
                draw(col, RingInput::Balance { remaining: *v });
            }
            for (col, p) in percent_cases.iter().enumerate() {
                draw(
                    balance_cases.len() + col,
                    RingInput::Percent { remaining_pct: *p },
                );
            }
        }
        let png = canvas.encode_png().expect("预览 PNG 编码失败");
        let path = std::env::temp_dir().join("quotatray-ring-preview.png");
        std::fs::write(&path, png).expect("预览 PNG 写盘失败");
        println!("圆环预览已输出：{}", path.display());
    }

    /// 契约：字模表覆盖 15 字形且中心文字只含字模字符。
    #[test]
    fn glyph_table_covers_center_texts() {
        let chars: Vec<char> = GLYPHS.iter().map(|(c, _)| *c).collect();
        assert_eq!(chars.len(), 15);
        for text in [
            center_text_balance(9999.0),
            center_text_balance(12_499.0),
            center_text_balance(1.2e6),
            center_text_percent(45.0),
            "0123456789kMB%.".to_string(),
        ] {
            for ch in text.chars() {
                assert!(glyph(ch).is_some(), "字模缺字符 {ch:?}");
            }
        }
    }
}
