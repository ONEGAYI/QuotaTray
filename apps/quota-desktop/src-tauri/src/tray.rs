//! 托盘：菜单构建/重建、圆环图标渲染、悬停节流刷新。
//!
//! 展示文本生成是纯函数（本模块上半部，带 [`Lang`] 参数），由契约测试
//! 以中英双语锁定形状；Tauri 交互部分（下半部）依赖运行时，行为由烟测覆盖。
//! 前端 `src/display.ts` 是平行双实现（成对注释约定），文案语义保持成对。
//! 圆环图标本体在 [`crate::ring`]（视觉规格 docs/design/tray-ring-demo.html）。

use std::collections::{BTreeMap, HashMap};

use quota_core::pricing::{self, PeakKind};
use quota_core::{AppConfig, CustomModelDef, PlanKind, ProviderEntry, UsageData};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::i18n::Lang;
use crate::ring;
use crate::settings::Settings;
use crate::state::{AppState, EntryState, now_ms};

pub const TRAY_ID: &str = "main";

/// 错误文案截断长度（托盘菜单行宽有限）。
const MESSAGE_LIMIT: usize = 60;

/// 「图标显示」子菜单项 id：自动项固定值与条目项前缀分立，
/// 避免条目 id 恰为 "auto" 时与自动项混同。
const ICON_SRC_AUTO_ID: &str = "icon-src-auto";
const ICON_SRC_ENTRY_PREFIX: &str = "icon-src-e-";

/// 峰谷翻转广播事件（payload = 后端 epoch 毫秒）：任一条目峰/谷翻转时
/// 随托盘重建一并发给全部 WebView，前端以此为锚点重算峰谷标签。
pub const PEAK_FLIP_EVENT: &str = "peak-flip";

// ---- 展示纯函数 -----------------------------------------------------------

/// 已用百分比（0-100）。
///
/// 约定：unit 为 "%" 时 used 即已用百分比；否则按 used/total 换算；
/// 数据不足返回 None（不猜测）。
pub fn used_percent(d: &UsageData) -> Option<f64> {
    if d.unit.as_deref() == Some("%") {
        return d.used;
    }
    match (d.used, d.total) {
        (Some(used), Some(total)) if total > 0.0 => Some(used / total * 100.0),
        _ => None,
    }
}

/// 相对时间文案（分档见 [`Lang::relative_time`]，与前端 display.ts 成对）。
pub fn relative_time(at_ms: u64, now_ms: u64, lang: Lang) -> String {
    let secs = now_ms.saturating_sub(at_ms) / 1000;
    lang.relative_time(secs)
}

/// 数值文案：余额保留两位小数，百分比取整。
fn amount_text(v: f64) -> String {
    format!("{v:.2}")
}

fn percent_text(v: f64) -> String {
    format!("{v:.0}%")
}

/// keep-last-good 窗口（GUI-spec §3）：瞬时失败后保留旧值展示的时限，
/// 超窗后按网络波动态展示（旧值过旧，不再作为展示依据）。
/// 前端同值定义在 `src/types.ts` 的 `KEEP_LAST_GOOD_MS`——两端同步修改。
pub(crate) const KEEP_LAST_GOOD_MS: u64 = 10 * 60 * 1000;

/// 条目展示行（多窗口一窗口一行）。
///
/// 形状（GUI-spec §3）：
/// - 成功：`名称 · 剩余 62.97 CNY · 3 分钟前` 或 `名称 · 已用 42% · 3 分钟前`；
/// - 多窗口行带窗口名：`名称 · five_hour 已用 42% · 3 分钟前`；
/// - 瞬时失败且旧值在 keep-last-good 窗口内：正常行尾追加 `⟳ 暂不可达`；
/// - 瞬时失败但无旧值或已超窗：`名称 · ⟳ 网络波动`；
/// - 确定性失败：`名称 · ⚠ 错误摘要`（立即透出，不展示旧值）；
/// - `is_valid=false`：`名称 · ⚠ 已失效：原因`；
/// - 已用百分比 ≥ 阈值的行首加 `⚠ `（原生菜单不支持着色，符号近似）。
pub fn entry_lines(
    name: &str,
    state: &EntryState,
    threshold_percent: u8,
    now_ms: u64,
    lang: Lang,
) -> Vec<String> {
    let t = lang.texts();
    let warn = |line: String, over: bool| {
        if over { format!("⚠ {line}") } else { line }
    };
    let time_suffix = |line: String, at: Option<u64>| match at {
        Some(at) => format!("{line} · {}", relative_time(at, now_ms, lang)),
        None => line,
    };

    // 确定性失败立即透出（覆盖旧值展示）
    if let Some(err) = &state.error
        && err.kind == "deterministic"
    {
        let msg: String = err.message.chars().take(MESSAGE_LIMIT).collect();
        return vec![format!("{name} · ⚠ {msg}")];
    }
    // 凭据/套餐失效（数据取回但 is_valid=false）
    if let Some(data) = &state.data
        && let Some(d) = data.iter().find(|d| d.is_valid == Some(false))
    {
        let reason = d
            .invalid_message
            .clone()
            .unwrap_or_else(|| t.no_invalid_reason.into());
        let msg: String = reason.chars().take(MESSAGE_LIMIT).collect();
        return vec![format!("{name} · ⚠ {}{msg}", t.invalid_prefix)];
    }

    // 瞬时失败超窗：旧值过旧不再作为展示依据，按网络波动态展示
    if let Some(err) = &state.error
        && err.kind == "transient"
        && state
            .at
            .is_none_or(|at| now_ms.saturating_sub(at) > KEEP_LAST_GOOD_MS)
    {
        return vec![format!("{name} · ⟳ {}", t.network_fluctuation)];
    }

    let Some(data) = &state.data else {
        // 无旧值：瞬时错误或尚未查询
        return match &state.error {
            Some(err) if err.kind == "transient" => {
                vec![format!("{name} · ⟳ {}", t.network_fluctuation)]
            }
            _ => vec![format!("{name} · {}", t.no_data)],
        };
    };

    // 成功数据（瞬时失败但在窗口内 → keep-last-good 附加 ⟳）
    let transient_mark = match &state.error {
        Some(err) if err.kind == "transient" => format!(" · ⟳ {}", t.unreachable),
        _ => String::new(),
    };
    let mut lines = Vec::with_capacity(data.len().max(1));
    for (i, d) in data.iter().enumerate() {
        let window = match data.len() {
            1 => String::new(),
            _ => format!(
                "{} ",
                d.plan_name
                    .clone()
                    .unwrap_or_else(|| lang.window_name(i + 1))
            ),
        };
        let body = if let Some(pct) = used_percent(d) {
            format!("{name} · {window}{}", lang.used_text(&percent_text(pct)))
        } else if let (Some(rem), unit) = (d.remaining, d.unit.clone()) {
            match unit {
                Some(u) if !u.is_empty() => {
                    format!(
                        "{name} · {window}{}",
                        lang.remaining_text(&amount_text(rem), Some(&u))
                    )
                }
                Some(_) | None => {
                    format!(
                        "{name} · {window}{}",
                        lang.remaining_text(&amount_text(rem), None)
                    )
                }
            }
        } else {
            format!("{name} · {window}{}", t.fetched)
        };
        let over = used_percent(d).is_some_and(|p| p >= f64::from(threshold_percent));
        lines.push(warn(time_suffix(body, state.at), over) + &transient_mark);
    }
    if lines.is_empty() {
        lines.push(format!("{name} · {}", t.no_data));
    }
    lines
}

/// 条目的峰谷信息行（最多两行，挂在「当前展示条目」名下）：
/// - 行 1：`⚡ 高峰 · V4 Flash`（当前判定 + 模型标签）；
/// - 行 2：当前档三价 `命中 0.3 · 未命中 9 · 输出 27 CNY/Mtok`
///   （缺价字段跳过；当前档整体缺失只显示行 1）。
///
/// 未配置峰谷定价（无预置且未自定义）返回空——不追加任何行。
#[cfg(test)]
pub fn pricing_lines(entry: &ProviderEntry, now_ms: u64, lang: Lang) -> Vec<String> {
    pricing_lines_with(entry, &Default::default(), None, now_ms, lang)
}

fn pricing_lines_with(
    entry: &ProviderEntry,
    custom_models: &BTreeMap<String, Vec<CustomModelDef>>,
    currency_hint: Option<&str>,
    now_ms: u64,
    lang: Lang,
) -> Vec<String> {
    let Some(resolved) = pricing::resolve_in_currency(entry, custom_models, currency_hint) else {
        return vec![];
    };
    let kind = resolved.kind(now_ms);
    let line1 = lang.peak_status_line(kind == PeakKind::Peak, resolved.model_label.as_deref());
    if resolved.plan == PlanKind::Subscription {
        return vec![line1, lang.subscription_pricing_line().into()];
    }
    let tier = match kind {
        PeakKind::Peak => resolved.peak.as_ref(),
        PeakKind::OffPeak => resolved.off_peak.as_ref(),
    };
    let Some(tier) = tier else {
        return vec![line1];
    };
    let fmt = |v: &Option<f64>| v.map(pricing::format_price);
    let line2 = lang.peak_prices_line(
        fmt(&tier.cache_hit_input).as_deref(),
        fmt(&tier.cache_miss_input).as_deref(),
        fmt(&tier.output).as_deref(),
        resolved.currency.as_deref(),
    );
    if line2.is_empty() {
        vec![line1]
    } else {
        vec![line1, line2]
    }
}

/// 条目的旧值是否仍可作为展示依据（keep-last-good 门控，菜单行/圆环/红点
/// 三方共用同一谓词）：确定性失败立即透出错误、瞬时失败超窗后旧值被否定，
/// 两种状态都不得再驱动任何展示结论。
pub(crate) fn state_is_displayable(st: &EntryState, now: u64) -> bool {
    match st.error.as_ref().map(|e| e.kind.as_str()) {
        Some("deterministic") => false,
        Some("transient") => st
            .at
            .is_some_and(|at| now.saturating_sub(at) <= KEEP_LAST_GOOD_MS),
        _ => true,
    }
}

/// 是否有条目超过低额度阈值（圆环右上角红点的依据）。
///
/// 门控与圆环/菜单行一致（`state_is_displayable`）：确定性失败或超窗瞬时
/// 失败的条目，其旧值不再作为告警依据。
pub fn any_alert(
    cfg: &AppConfig,
    results: &HashMap<String, EntryState>,
    settings: &Settings,
    now: u64,
) -> bool {
    cfg.providers
        .iter()
        .filter(|p| p.enabled)
        .filter_map(|p| results.get(&p.id))
        .filter(|st| state_is_displayable(st, now))
        .filter_map(|st| st.data.as_ref())
        .any(|data| {
            data.iter().filter(|d| d.is_valid != Some(false)).any(|d| {
                used_percent(d)
                    .is_some_and(|p| p >= f64::from(settings.low_balance_threshold_percent))
            })
        })
}

// ---- Tauri 交互 -----------------------------------------------------------

/// 创建托盘（setup 阶段调用一次）。
///
/// 首次启动配置文件不存在是正常路径（load 返回空配置，非 Err）；
/// 真正读盘失败时给诚实提示菜单而非空菜单。
pub fn create(app: &AppHandle, state: &AppState) -> tauri::Result<()> {
    let update_version = state
        .update_ctl
        .read()
        .unwrap()
        .info
        .as_ref()
        .map(|i| i.version.clone());
    let menu = match snapshot_views(state) {
        Some((cfg, results, settings)) => {
            let lang = Lang::parse(&settings.language);
            build_menu(
                app,
                &cfg,
                &results,
                &settings,
                lang,
                update_version.as_deref(),
            )?
        }
        None => {
            // 配置读盘失败，但设置仍可读（语言跟随用户选择）
            let t = Lang::parse(&state.settings.read().unwrap().language).texts();
            let menu = Menu::new(app)?;
            menu.append(&MenuItem::with_id(
                app,
                "info-config-error",
                t.config_error,
                false,
                None::<&str>,
            )?)?;
            menu.append(&PredefinedMenuItem::separator(app)?)?;
            menu.append(&MenuItem::with_id(
                app,
                "show",
                t.open_main,
                true,
                None::<&str>,
            )?)?;
            menu.append(&MenuItem::with_id(app, "quit", t.quit, true, None::<&str>)?)?;
            menu
        }
    };
    // 首屏图标：无数据时为灰空环（快照数据由后续 rebuild 反映）
    let dark = *state.resolved_theme.read().unwrap();
    let icon = match snapshot_views(state) {
        Some((cfg, results, settings)) => {
            let alert = any_alert(&cfg, &results, &settings, now_ms());
            ring::icon_image(&cfg, &results, &settings, dark, alert)
        }
        None => ring::icon_image(
            &AppConfig::default(),
            &HashMap::new(),
            &Settings::default(),
            dark,
            false,
        ),
    };
    tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("QuotaTray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| handle_tray_event(tray.app_handle(), &event))
        .build(app)?;
    Ok(())
}

/// 重建托盘菜单与图标（每次查询/配置/设置变更后调用）。
///
/// 重建失败不打断业务（托盘停留旧状态），但记录日志便于排查；
/// 配置读盘失败同样保留旧菜单（读盘失败 ≠ 无供应商，清空会误导）。
pub fn rebuild(app: &AppHandle, state: &AppState) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let Some((cfg, results, settings)) = snapshot_views(state) else {
        eprintln!("配置读取失败，托盘保留既有菜单");
        return;
    };
    let lang = Lang::parse(&settings.language);
    let update_version = state
        .update_ctl
        .read()
        .unwrap()
        .info
        .as_ref()
        .map(|i| i.version.clone());
    let menu = match build_menu(
        app,
        &cfg,
        &results,
        &settings,
        lang,
        update_version.as_deref(),
    ) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("托盘菜单重建失败：{e}");
            return;
        }
    };
    if let Err(e) = tray.set_menu(Some(menu)) {
        eprintln!("托盘菜单应用失败：{e}");
    }
    let dark = *state.resolved_theme.read().unwrap();
    let alert = any_alert(&cfg, &results, &settings, now_ms());
    let icon = ring::icon_image(&cfg, &results, &settings, dark, alert);
    if let Err(e) = tray.set_icon(Some(icon)) {
        eprintln!("托盘图标切换失败：{e}");
    }
}

/// 读当前 config / results / settings（重建托盘的一致性视图）。
///
/// 配置读盘失败返回 None——调用方应保留既有菜单而非构建空菜单
/// （读盘失败 ≠ 无供应商，清空会误导用户）。
fn snapshot_views(state: &AppState) -> Option<(AppConfig, HashMap<String, EntryState>, Settings)> {
    let cfg = AppConfig::load(&state.paths.config()).ok()?;
    let results = state.results.read().unwrap().clone();
    let settings = state.settings.read().unwrap().clone();
    Some((cfg, results, settings))
}

fn pricing_currency_hint<'a>(
    entry: &'a ProviderEntry,
    state: Option<&'a EntryState>,
) -> Option<&'a str> {
    state
        .and_then(|value| value.data.as_ref())
        .and_then(|data| data.first())
        .and_then(|data| data.unit.as_deref())
        .or_else(|| {
            entry
                .pricing
                .as_ref()
                .and_then(|pricing| pricing.currency.as_deref())
        })
}

/// 全部启用条目的峰谷判定快照（id → 峰/谷）。
/// 仅收录 resolve 出生效峰谷配置的条目——禁用或无峰谷配置的条目
/// 无标签可翻转，不参与 [`rebuild_on_peak_flip`] 的缓存比对。
fn peak_map(
    cfg: &AppConfig,
    results: &HashMap<String, EntryState>,
    now: u64,
) -> HashMap<String, PeakKind> {
    cfg.providers
        .iter()
        .filter(|entry| entry.enabled)
        .filter_map(|entry| {
            let currency_hint = pricing_currency_hint(entry, results.get(&entry.id));
            pricing::resolve_in_currency(entry, &cfg.custom_models, currency_hint)
                .map(|resolved| (entry.id.clone(), resolved.kind(now)))
        })
        .collect()
}

/// 每分钟调度调用：任一启用条目的峰谷状态与上次检测不一致（含首次）→
/// 重建托盘并向全部 WebView 广播 [`PEAK_FLIP_EVENT`]（payload = 后端
/// epoch 毫秒）。前端常驻面板/卡片以事件为锚点重算峰谷标签——与托盘
/// 菜单同 tick 同源，修复悬停面板标签停留在上次渲染判定的问题（#15）；
/// 读盘失败静默保留缓存（下次再比）。
pub fn rebuild_on_peak_flip(app: &AppHandle, state: &AppState) {
    let Some((cfg, results, _settings)) = snapshot_views(state) else {
        return;
    };
    let now = now_ms();
    let current = peak_map(&cfg, &results, now);
    let mut last = state.last_peak.write().unwrap();
    if *last == current {
        return;
    }
    *last = current;
    drop(last);
    // 广播失败不打断托盘重建（窗口退出途中无监听者是正常态）
    let _ = app.emit(PEAK_FLIP_EVENT, now);
    rebuild(app, state);
}

fn build_menu(
    app: &AppHandle,
    cfg: &AppConfig,
    results: &HashMap<String, EntryState>,
    settings: &Settings,
    lang: Lang,
    update_version: Option<&str>,
) -> tauri::Result<Menu<Wry>> {
    let t = lang.texts();
    let now = now_ms();
    let menu = Menu::new(app)?;
    // 数据/峰谷行只挂「当前展示条目」（圆环数据源，与「图标显示」子菜单
    // 同一回退语义）：托盘菜单是快捷入口，其余条目在主窗口查看，
    // 避免灰色信息行随条目数线性膨胀
    match ring::icon_entry(cfg, settings) {
        Some(entry) => {
            let lines = match results.get(&entry.id) {
                Some(st) => entry_lines(
                    &entry.name,
                    st,
                    settings.low_balance_threshold_percent,
                    now,
                    lang,
                ),
                None => vec![format!("{} · {}", entry.name, t.no_data)],
            };
            for (i, line) in lines.iter().enumerate() {
                menu.append(&MenuItem::with_id(
                    app,
                    format!("info-{}-{i}", entry.id),
                    line,
                    false,
                    None::<&str>,
                )?)?;
            }
            // 峰谷行（disabled，id 独立前缀避免与数据行混同）
            let currency_hint = pricing_currency_hint(entry, results.get(&entry.id));
            for (i, line) in pricing_lines_with(entry, &cfg.custom_models, currency_hint, now, lang)
                .iter()
                .enumerate()
            {
                menu.append(&MenuItem::with_id(
                    app,
                    format!("info-pricing-{i}"),
                    line,
                    false,
                    None::<&str>,
                )?)?;
            }
        }
        None => {
            menu.append(&MenuItem::with_id(
                app,
                "info-empty",
                t.no_enabled_providers,
                false,
                None::<&str>,
            )?)?;
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    // 新版本信息行（disabled：操作入口在设置页「更新」分页）
    if let Some(v) = update_version {
        menu.append(&MenuItem::with_id(
            app,
            "info-update",
            lang.update_available(v),
            false,
            None::<&str>,
        )?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append(&MenuItem::with_id(
        app,
        "refresh",
        t.refresh_now,
        true,
        None::<&str>,
    )?)?;

    // 「图标显示」子菜单：当前生效数据源打勾（stale id 回退项也如实反映）
    let icon_submenu = build_icon_source_submenu(app, cfg, settings, lang)?;
    menu.append(&icon_submenu)?;

    menu.append(&MenuItem::with_id(
        app,
        "show",
        t.open_main,
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(app, "quit", t.quit, true, None::<&str>)?)?;
    Ok(menu)
}

/// 构造「图标显示」子菜单：自动项 + 全部 enabled 条目。
///
/// 打勾依据是「当前实际生效的数据源」（`ring::icon_entry` 的回退结果），
/// 与图标渲染同源——stale id（条目已删除）时自动项打勾，选择不落盘清除，
/// 用户重新选择时自然覆盖。
fn build_icon_source_submenu(
    app: &AppHandle,
    cfg: &AppConfig,
    settings: &Settings,
    lang: Lang,
) -> tauri::Result<Submenu<Wry>> {
    let t = lang.texts();
    let submenu = Submenu::with_id(app, "icon-source", t.icon_source, true)?;
    let effective = ring::icon_entry(cfg, settings).map(|e| e.id.clone());
    // 自动项打勾：未指定，或指定 id 已失效（实际生效的是回退结果）
    let auto_checked = match &settings.tray_icon_entry_id {
        None => true,
        Some(specified) => effective.as_deref() != Some(specified.as_str()),
    };
    submenu.append(&CheckMenuItem::with_id(
        app,
        ICON_SRC_AUTO_ID,
        t.icon_source_auto,
        true,
        auto_checked,
        None::<&str>,
    )?)?;
    let has_entries = cfg.providers.iter().any(|p| p.enabled);
    if has_entries {
        submenu.append(&PredefinedMenuItem::separator(app)?)?;
        for entry in cfg.providers.iter().filter(|p| p.enabled) {
            let checked = effective.as_deref() == Some(entry.id.as_str());
            submenu.append(&CheckMenuItem::with_id(
                app,
                format!("{ICON_SRC_ENTRY_PREFIX}{}", entry.id),
                entry.name.clone(),
                true,
                checked,
                None::<&str>,
            )?)?;
        }
    }
    Ok(submenu)
}

/// 切换图标数据源：先落盘成功再写回内存并重建托盘（磁盘权威，
/// 与 `commands::save_settings` 同一顺序——落盘失败时内存不动，
/// 磁盘/内存/UI 三方不分裂）。
fn set_icon_entry(app: &AppHandle, id: Option<String>) {
    let state = app.state::<AppState>();
    let mut updated = state.settings.read().unwrap().clone();
    updated.tray_icon_entry_id = id;
    if let Err(e) = updated.save(&state.paths.settings()) {
        eprintln!("图标显示设置写入失败：{e}");
        return;
    }
    *state.settings.write().unwrap() = updated; // 写锁即取即释，rebuild 内部读不冲突
    rebuild(app, &state);
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "refresh" => emit_refresh(app),
        "show" => show_main(app),
        "quit" => {
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_visible(false);
            }
            app.exit(0);
        }
        _ => {
            if id == ICON_SRC_AUTO_ID {
                set_icon_entry(app, None);
            } else if let Some(entry) = id.strip_prefix(ICON_SRC_ENTRY_PREFIX)
                && !entry.is_empty()
            {
                set_icon_entry(app, Some(entry.to_string()));
            }
        }
    }
}

fn handle_tray_event(app: &AppHandle, event: &TrayIconEvent) {
    match event {
        // 左键单击：打开主窗口（Windows 惯例；菜单由右键出）
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => show_main(app),
        // 悬停：仅显示详情浮层。数据新鲜度由后台轮询与面板手动刷新按钮
        // 兜底——悬停是纯显示操作，不触发任何 Provider 查询。
        TrayIconEvent::Enter { rect, .. } => {
            crate::hover_panel::tray_enter(app, *rect);
        }
        TrayIconEvent::Leave { .. } => crate::hover_panel::tray_leave(app),
        // Windows 上游偶发漏发 Leave 后，下一次经过图标只会收到 Move；
        // 本地状态已隐藏/离开时将其作为恢复性 Enter（同样只管浮层）。
        TrayIconEvent::Move { rect, .. } if crate::hover_panel::tray_move(app, *rect) => {}
        _ => {}
    }
}

fn emit_refresh(app: &AppHandle) {
    let _ = app.emit("refresh-now", ());
}

/// 显示并聚焦主窗口（托盘左键 / 单实例回调 / 菜单项共用）。
pub fn show_main(app: &AppHandle) {
    crate::hover_panel::hide(app);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

// ---- 契约测试（中英双语参数化） -------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use quota_core::PlanVariant;
    use quota_core::UsageData;

    fn data(remaining: Option<f64>, unit: Option<&str>) -> UsageData {
        UsageData {
            remaining,
            unit: unit.map(String::from),
            ..Default::default()
        }
    }

    fn percent_data(used: Option<f64>) -> UsageData {
        UsageData {
            used,
            unit: Some("%".into()),
            ..Default::default()
        }
    }

    fn ok_state(data: Vec<UsageData>, at: u64) -> EntryState {
        EntryState {
            data: Some(data),
            at: Some(at),
            error: None,
        }
    }

    const NOW: u64 = 1_755_000_000_000;

    /// 双语断言辅助：同一状态在 zh/en 下各自匹配期望行。
    fn assert_both(name: &str, st: &EntryState, zh: Vec<&str>, en: Vec<&str>) {
        assert_eq!(
            entry_lines(name, st, 80, NOW, Lang::Zh),
            zh.into_iter().map(String::from).collect::<Vec<_>>()
        );
        assert_eq!(
            entry_lines(name, st, 80, NOW, Lang::En),
            en.into_iter().map(String::from).collect::<Vec<_>>()
        );
    }

    /// 契约：余额型行——`名称 · 剩余 62.97 CNY · 3 分钟前`。
    #[test]
    fn balance_line_shape() {
        let st = ok_state(vec![data(Some(62.97), Some("CNY"))], NOW - 180_000);
        assert_both(
            "DeepSeek",
            &st,
            vec!["DeepSeek · 剩余 62.97 CNY · 3 分钟前"],
            vec!["DeepSeek · Left 62.97 CNY · 3m ago"],
        );
    }

    // ---- 峰谷信息行（与 core pricing 测试同款时间锚点） ----

    /// 北京时间 2026-08-19（周三）09:30（DeepSeek 高峰内）。
    const PEAK_NOW: u64 = 1_787_103_000_000;
    /// 北京时间 2026-08-19（周三）04:30（夜间空闲）。
    const OFF_NOW: u64 = 1_787_085_000_000;

    fn entry_with(pricing: Option<quota_core::PricingConfig>) -> ProviderEntry {
        ProviderEntry {
            id: "p1".into(),
            name: "DeepSeek".into(),
            kind: quota_core::ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            api_key2_enc: None,
            base_url: None,
            pricing,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        }
    }

    /// 契约：预置 DeepSeek——峰内两行（类型+模型 / 当前档三价），双语。
    #[test]
    fn pricing_lines_preset_peak() {
        let e = entry_with(None);
        assert_eq!(
            pricing_lines(&e, PEAK_NOW, Lang::Zh),
            vec![
                "⚡ 高峰 · V4 Flash",
                "命中 0.1 · 未命中 3 · 输出 9 CNY/Mtok"
            ]
        );
        assert_eq!(
            pricing_lines(&e, PEAK_NOW, Lang::En),
            vec!["⚡ Peak · V4 Flash", "Hit 0.1 · Miss 3 · Out 9 CNY/Mtok"]
        );
    }

    /// 契约：谷内显示空闲档价（空闲 = 高峰一半）。
    #[test]
    fn pricing_lines_preset_off_peak() {
        let e = entry_with(None);
        assert_eq!(
            pricing_lines(&e, OFF_NOW, Lang::Zh),
            vec![
                "空闲 · V4 Flash",
                "命中 0.05 · 未命中 1.5 · 输出 4.5 CNY/Mtok"
            ]
        );
    }

    /// 契约：model 选择切换价格档（预置 pro）。
    #[test]
    fn pricing_lines_model_selection() {
        let e = entry_with(Some(quota_core::PricingConfig {
            model: Some("pro".into()),
            ..Default::default()
        }));
        assert_eq!(
            pricing_lines(&e, PEAK_NOW, Lang::Zh),
            vec!["⚡ 高峰 · V4 Pro", "命中 0.3 · 未命中 9 · 输出 27 CNY/Mtok"]
        );
    }

    #[test]
    fn pricing_lines_follow_currency_and_custom_model_library() {
        let e = entry_with(None);
        assert_eq!(
            pricing_lines_with(&e, &Default::default(), Some("USD"), OFF_NOW, Lang::Zh),
            vec![
                "空闲 · V4 Flash",
                "命中 0.007 · 未命中 0.22 · 输出 0.66 USD/Mtok"
            ]
        );

        let mut models = std::collections::BTreeMap::new();
        models.insert(
            "deepseek".into(),
            vec![quota_core::CustomModelDef {
                id: "flash".into(),
                display: "V4 Flash（自算）".into(),
                peak: Some(quota_core::PriceTier {
                    output: Some(9.1),
                    ..Default::default()
                }),
                ..Default::default()
            }],
        );
        let e = entry_with(Some(quota_core::PricingConfig {
            model: Some("flash".into()),
            ..Default::default()
        }));
        assert_eq!(
            pricing_lines_with(&e, &models, None, PEAK_NOW, Lang::Zh),
            vec!["⚡ 高峰 · V4 Flash（自算）", "输出 9.1 CNY/Mtok"]
        );
    }

    #[test]
    fn pricing_currency_hint_prefers_query_unit_then_entry_override() {
        let e = entry_with(Some(quota_core::PricingConfig {
            currency: Some("CNY".into()),
            ..Default::default()
        }));
        let state = ok_state(vec![data(Some(8.0), Some("USD"))], NOW);
        assert_eq!(pricing_currency_hint(&e, Some(&state)), Some("USD"));
        assert_eq!(pricing_currency_hint(&e, None), Some("CNY"));
    }

    #[test]
    fn pricing_lines_describe_subscription_plan() {
        let mut e = entry_with(Some(quota_core::PricingConfig {
            model: Some("coding-plan".into()),
            ..Default::default()
        }));
        e.kind = quota_core::ProviderKind::Native {
            provider: "zhipu".into(),
        };
        let wed_1500_bj = 1_787_122_800_000;
        assert_eq!(
            pricing_lines_with(&e, &Default::default(), Some("%"), wed_1500_bj, Lang::Zh,),
            vec!["⚡ 高峰 · GLM Coding Plan（订阅积分）", "订阅积分制"]
        );
    }

    /// 契约：无峰谷配置的条目不追加任何行。
    #[test]
    fn pricing_lines_absent_without_config() {
        let mut e = entry_with(None);
        e.kind = quota_core::ProviderKind::Native {
            provider: "siliconflow".into(),
        };
        assert_eq!(pricing_lines(&e, PEAK_NOW, Lang::Zh), Vec::<String>::new());
        assert_eq!(pricing_lines(&e, PEAK_NOW, Lang::En), Vec::<String>::new());
    }

    /// 契约：翻转检测的判定快照 peak_map 只收「启用且 resolve 出生效峰谷
    /// 配置」的条目（id → 峰/谷）——禁用或无峰谷配置的条目不参与，
    /// 锚点时刻跨过翻转边界时判定随之时变（rebuild_on_peak_flip 与
    /// last_peak 缓存比对的数据源）。
    #[test]
    fn peak_map_tracks_enabled_priced_entries_only() {
        let mut plain = entry_with(None);
        plain.id = "p2".into();
        plain.kind = quota_core::ProviderKind::Native {
            provider: "siliconflow".into(),
        };
        let mut disabled = entry_with(None);
        disabled.id = "p3".into();
        disabled.enabled = false;
        let cfg = AppConfig {
            providers: vec![entry_with(None), plain, disabled],
            ..Default::default()
        };

        let map = peak_map(&cfg, &HashMap::new(), PEAK_NOW);
        assert_eq!(map.len(), 1, "只有带峰谷配置的启用条目参与（p1）：{map:?}");
        assert_eq!(map.get("p1"), Some(&PeakKind::Peak));

        // 同一条目跨过翻转边界 → 判定翻转（调用方据比对结果广播+重建）
        let map_off = peak_map(&cfg, &HashMap::new(), OFF_NOW);
        assert_eq!(map_off.get("p1"), Some(&PeakKind::OffPeak));

        // 条目清空 → 空 map（条目增删触发一次比对差异，无害）
        let empty = AppConfig {
            providers: vec![],
            ..Default::default()
        };
        assert!(peak_map(&empty, &HashMap::new(), PEAK_NOW).is_empty());
    }

    /// 契约：当前档价格全缺时只显示类型行；部分缺价跳过该字段。
    #[test]
    fn pricing_lines_partial_tier() {
        let e = entry_with(Some(quota_core::PricingConfig {
            model: Some("pro".into()),
            peak: Some(quota_core::PriceTier {
                cache_hit_input: Some(0.3),
                ..Default::default()
            }),
            ..Default::default()
        }));
        // 峰内且自定义 peak 只给命中价 → 只显示命中；谷价回退预置 pro 全档
        assert_eq!(
            pricing_lines(&e, PEAK_NOW, Lang::Zh),
            vec!["⚡ 高峰 · V4 Pro", "命中 0.3 CNY/Mtok"]
        );
        assert_eq!(
            pricing_lines(&e, OFF_NOW, Lang::Zh),
            vec![
                "空闲 · V4 Pro",
                "命中 0.15 · 未命中 4.5 · 输出 13.5 CNY/Mtok"
            ]
        );
    }

    /// 契约：百分比型行——unit="%" 时 used 即已用百分比。
    #[test]
    fn percent_line_shape() {
        let st = ok_state(vec![percent_data(Some(42.0))], NOW - 180_000);
        assert_both(
            "GLM",
            &st,
            vec!["GLM · 已用 42% · 3 分钟前"],
            vec!["GLM · Used 42% · 3m ago"],
        );
    }

    /// 契约：多窗口一窗口一行，窗口名取 planName，缺省回退「窗口N」。
    #[test]
    fn multi_window_lines() {
        let d1 = UsageData {
            plan_name: Some("five_hour".into()),
            used: Some(42.0),
            unit: Some("%".into()),
            ..Default::default()
        };
        let d2 = UsageData {
            plan_name: None,
            used: Some(10.0),
            unit: Some("%".into()),
            ..Default::default()
        };
        let st = ok_state(vec![d1, d2], NOW - 300_000);
        assert_both(
            "GLM",
            &st,
            vec![
                "GLM · five_hour 已用 42% · 5 分钟前",
                "GLM · 窗口2 已用 10% · 5 分钟前",
            ],
            vec![
                "GLM · five_hour Used 42% · 5m ago",
                "GLM · Window 2 Used 10% · 5m ago",
            ],
        );
    }

    /// 契约：瞬时失败 + 旧值 → 旧值行尾附加 ⟳（keep-last-good）。
    #[test]
    fn transient_error_keeps_last_good() {
        let st = EntryState {
            data: Some(vec![data(Some(88.0), Some("CNY"))]),
            at: Some(NOW - 120_000),
            error: Some(crate::state::ErrorInfo {
                kind: "transient".into(),
                message: "timeout".into(),
                detail: None,
            }),
        };
        assert_both(
            "X",
            &st,
            vec!["X · 剩余 88.00 CNY · 2 分钟前 · ⟳ 暂不可达"],
            vec!["X · Left 88.00 CNY · 2m ago · ⟳ Unreachable"],
        );
    }

    /// 契约：瞬时失败超过 keep-last-good 窗口（10 分钟）→ 旧值不再展示，
    /// 按网络波动态显示（GUI-spec §3 的窗口语义）；恰 10 分钟仍在窗口内
    /// （Rust `>` 与前端 `<=` 的边界对称由本断言锁定）。
    #[test]
    fn transient_error_beyond_window_drops_stale_data() {
        let in_window = NOW - 9 * 60 * 1000;
        let exactly = NOW - 600_000; // 恰 10 分钟：窗口内
        let beyond = NOW - 11 * 60 * 1000;
        let st = |at: u64| EntryState {
            data: Some(vec![data(Some(88.0), Some("CNY"))]),
            at: Some(at),
            error: Some(crate::state::ErrorInfo {
                kind: "transient".into(),
                message: "timeout".into(),
                detail: None,
            }),
        };
        assert_both(
            "X",
            &st(in_window),
            vec!["X · 剩余 88.00 CNY · 9 分钟前 · ⟳ 暂不可达"],
            vec!["X · Left 88.00 CNY · 9m ago · ⟳ Unreachable"],
        );
        assert_both(
            "X",
            &st(exactly),
            vec!["X · 剩余 88.00 CNY · 10 分钟前 · ⟳ 暂不可达"],
            vec!["X · Left 88.00 CNY · 10m ago · ⟳ Unreachable"],
        );
        assert_both(
            "X",
            &st(beyond),
            vec!["X · ⟳ 网络波动"],
            vec!["X · ⟳ Network issue"],
        );
    }

    /// 契约：错误文案截断到 MESSAGE_LIMIT（托盘菜单行宽有限）。
    #[test]
    fn long_error_message_is_truncated() {
        let long = "错".repeat(200);
        let st = EntryState {
            error: Some(crate::state::ErrorInfo {
                kind: "deterministic".into(),
                message: long.clone(),
                detail: None,
            }),
            ..Default::default()
        };
        for lang in [Lang::Zh, Lang::En] {
            let line = &entry_lines("X", &st, 80, NOW, lang)[0];
            // X · ⚠ + 60 字符
            assert!(line.chars().count() < 70, "应截断：{line}");
            assert!(!line.contains(&"错".repeat(61)), "不得超出截断上限");
        }
    }

    /// 契约：瞬时失败无旧值 → ⟳ 网络波动；确定性失败 → ⚠ 立即透出（覆盖旧值）。
    #[test]
    fn error_without_data_and_deterministic() {
        let transient = EntryState {
            error: Some(crate::state::ErrorInfo {
                kind: "transient".into(),
                message: "网络中断".into(),
                detail: None,
            }),
            ..Default::default()
        };
        assert_both(
            "X",
            &transient,
            vec!["X · ⟳ 网络波动"],
            vec!["X · ⟳ Network issue"],
        );

        let deterministic = EntryState {
            data: Some(vec![data(Some(5.0), Some("CNY"))]),
            at: Some(NOW),
            error: Some(crate::state::ErrorInfo {
                kind: "deterministic".into(),
                message: "HTTP 401: Unauthorized".into(),
                detail: None,
            }),
        };
        assert_both(
            "X",
            &deterministic,
            vec!["X · ⚠ HTTP 401: Unauthorized"],
            vec!["X · ⚠ HTTP 401: Unauthorized"],
        );
    }

    /// 契约：is_valid=false → 失效行（额度告警不触发）。
    #[test]
    fn invalid_credentials_line() {
        let d = UsageData {
            is_valid: Some(false),
            invalid_message: Some("key 已过期".into()),
            ..Default::default()
        };
        let st = ok_state(vec![d], NOW);
        assert_both(
            "X",
            &st,
            vec!["X · ⚠ 已失效：key 已过期"],
            vec!["X · ⚠ Invalid: key 已过期"],
        );
    }

    /// 契约：超阈值行首加 ⚠（恰等于阈值触发）。
    #[test]
    fn threshold_adds_warning_prefix() {
        let st = ok_state(vec![percent_data(Some(80.0))], NOW);
        assert_both(
            "X",
            &st,
            vec!["⚠ X · 已用 80% · 刚刚"],
            vec!["⚠ X · Used 80% · just now"],
        );
        let below = ok_state(vec![percent_data(Some(79.9))], NOW);
        assert_both(
            "X",
            &below,
            vec!["X · 已用 80% · 刚刚"],
            vec!["X · Used 80% · just now"],
        );
    }

    /// 契约：已用百分比换算——used/total 与 unit="%" 直读。
    #[test]
    fn used_percent_calculation() {
        let mut d = UsageData {
            used: Some(42.0),
            total: Some(200.0),
            ..Default::default()
        };
        assert_eq!(used_percent(&d), Some(21.0));
        d.unit = Some("%".into());
        assert_eq!(used_percent(&d), Some(42.0));
        // total 为 0 / 缺 used → None（不猜测）
        d.total = Some(0.0);
        d.unit = None;
        assert_eq!(used_percent(&d), None);
        assert_eq!(used_percent(&UsageData::default()), None);
    }

    /// 契约：相对时间分档（双语委托 i18n，此处锁端到端形状）。
    #[test]
    fn relative_time_buckets() {
        assert_eq!(relative_time(NOW - 5_000, NOW, Lang::Zh), "刚刚");
        assert_eq!(relative_time(NOW - 30_000, NOW, Lang::Zh), "30 秒前");
        assert_eq!(relative_time(NOW - 180_000, NOW, Lang::Zh), "3 分钟前");
        assert_eq!(relative_time(NOW - 7_200_000, NOW, Lang::Zh), "2 小时前");
        assert_eq!(relative_time(NOW - 172_800_000, NOW, Lang::Zh), "2 天前");
        assert_eq!(relative_time(NOW - 5_000, NOW, Lang::En), "just now");
        assert_eq!(relative_time(NOW - 30_000, NOW, Lang::En), "30s ago");
        assert_eq!(relative_time(NOW - 180_000, NOW, Lang::En), "3m ago");
        assert_eq!(relative_time(NOW - 7_200_000, NOW, Lang::En), "2h ago");
        assert_eq!(relative_time(NOW - 172_800_000, NOW, Lang::En), "2d ago");
    }

    /// 契约：any_alert——enabled + 超阈值才触发；disabled / 失效条目不触发。
    #[test]
    fn any_alert_rules() {
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
            providers: vec![entry("a", true), entry("b", false)],
        };
        let settings = Settings::default(); // 阈值 80

        let mut results = HashMap::new();
        results.insert("a".into(), ok_state(vec![percent_data(Some(85.0))], NOW));
        results.insert("b".into(), ok_state(vec![percent_data(Some(95.0))], NOW));
        assert!(
            any_alert(&cfg, &results, &settings, NOW),
            "enabled 条目超阈值应告警"
        );

        results.insert("a".into(), ok_state(vec![percent_data(Some(50.0))], NOW));
        assert!(
            !any_alert(&cfg, &results, &settings, NOW),
            "disabled 条目超阈值不告警"
        );

        let invalid = UsageData {
            used: Some(99.0),
            unit: Some("%".into()),
            is_valid: Some(false),
            ..Default::default()
        };
        results.insert("a".into(), ok_state(vec![invalid], NOW));
        assert!(
            !any_alert(&cfg, &results, &settings, NOW),
            "失效条目不触发额度告警"
        );
    }

    /// 契约：any_alert 与菜单行/圆环同一 keep-last-good 门控——
    /// 确定性失败、超窗瞬时失败的旧值不再驱动红点；窗口内瞬时仍触发。
    #[test]
    fn any_alert_respects_keep_last_good_gating() {
        use quota_core::{ProviderEntry, ProviderKind};
        let entry = |id: &str| ProviderEntry {
            id: id.into(),
            name: id.into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            api_key2_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        };
        let cfg = AppConfig {
            custom_models: Default::default(),
            providers: vec![entry("a")],
        };
        let settings = Settings::default(); // 阈值 80

        let over = ok_state(vec![percent_data(Some(85.0))], NOW);
        let mut results = HashMap::new();

        // 确定性失败：错误立即覆盖旧值展示语义，旧值不得再驱动红点
        let mut deterministic = over.clone();
        deterministic.error = Some(crate::state::ErrorInfo {
            kind: "deterministic".into(),
            message: "401".into(),
            detail: None,
        });
        results.insert("a".into(), deterministic);
        assert!(
            !any_alert(&cfg, &results, &settings, NOW),
            "确定性失败条目不应触发红点"
        );

        // 瞬时失败：窗口内（含恰 10 分钟）旧值仍是展示依据 → 红点保留
        let mut transient = over.clone();
        transient.error = Some(crate::state::ErrorInfo {
            kind: "transient".into(),
            message: "timeout".into(),
            detail: None,
        });
        transient.at = Some(NOW - 600_000); // 恰 10 分钟：窗口内
        results.insert("a".into(), transient.clone());
        assert!(
            any_alert(&cfg, &results, &settings, NOW),
            "窗口内瞬时失败的旧值仍应驱动红点"
        );

        // 超窗（>10 分钟）：旧值被否定 → 红点不触发
        let mut stale = transient;
        stale.at = Some(NOW - 601_000);
        results.insert("a".into(), stale);
        assert!(
            !any_alert(&cfg, &results, &settings, NOW),
            "超窗瞬时失败的旧值不应再驱动红点"
        );
    }
}
