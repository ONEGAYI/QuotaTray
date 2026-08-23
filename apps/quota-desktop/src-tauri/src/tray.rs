//! 托盘：菜单构建/重建、图标切换、悬停节流刷新。
//!
//! 展示文本生成是纯函数（本模块上半部），由契约测试锁定形状；
//! Tauri 交互部分（下半部）依赖运行时，行为由烟测覆盖。

use std::collections::HashMap;

use quota_core::{AppConfig, UsageData};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::settings::Settings;
use crate::state::{AppState, EntryState, now_ms};

pub const TRAY_ID: &str = "main";

/// 悬停刷新节流窗口（GUI-spec §3：10 秒，cc-switch 验证过的节奏）。
const HOVER_THROTTLE_MS: u64 = 10_000;

/// 错误文案截断长度（托盘菜单行宽有限）。
const MESSAGE_LIMIT: usize = 60;

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

/// 相对时间文案："刚刚" / "N 秒前" / "N 分钟前" / "N 小时前" / "N 天前"。
pub fn relative_time(at_ms: u64, now_ms: u64) -> String {
    let secs = now_ms.saturating_sub(at_ms) / 1000;
    match secs {
        0..=9 => "刚刚".into(),
        10..=59 => format!("{secs} 秒前"),
        60..=3599 => format!("{} 分钟前", secs / 60),
        3600..=86_399 => format!("{} 小时前", secs / 3600),
        _ => format!("{} 天前", secs / 86_400),
    }
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
const KEEP_LAST_GOOD_MS: u64 = 10 * 60 * 1000;

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
) -> Vec<String> {
    let warn = |line: String, over: bool| {
        if over { format!("⚠ {line}") } else { line }
    };
    let time_suffix = |line: String, at: Option<u64>| match at {
        Some(at) => format!("{line} · {}", relative_time(at, now_ms)),
        None => line,
    };

    // 确定性失败立即透出（覆盖旧值展示）
    if let Some(err) = &state.error {
        if err.kind == "deterministic" {
            let msg: String = err.message.chars().take(MESSAGE_LIMIT).collect();
            return vec![format!("{name} · ⚠ {msg}")];
        }
    }
    // 凭据/套餐失效（数据取回但 is_valid=false）
    if let Some(data) = &state.data {
        if let Some(d) = data.iter().find(|d| d.is_valid == Some(false)) {
            let reason = d
                .invalid_message
                .clone()
                .unwrap_or_else(|| "未说明原因".into());
            let msg: String = reason.chars().take(MESSAGE_LIMIT).collect();
            return vec![format!("{name} · ⚠ 已失效：{msg}")];
        }
    }

    // 瞬时失败超窗：旧值过旧不再作为展示依据，按网络波动态展示
    if let Some(err) = &state.error {
        if err.kind == "transient"
            && state
                .at
                .is_none_or(|at| now_ms.saturating_sub(at) > KEEP_LAST_GOOD_MS)
        {
            return vec![format!("{name} · ⟳ 网络波动")];
        }
    }

    let Some(data) = &state.data else {
        // 无旧值：瞬时错误或尚未查询
        return match &state.error {
            Some(err) if err.kind == "transient" => vec![format!("{name} · ⟳ 网络波动")],
            _ => vec![format!("{name} · 尚无数据")],
        };
    };

    // 成功数据（瞬时失败但在窗口内 → keep-last-good 附加 ⟳）
    let transient_mark = match &state.error {
        Some(err) if err.kind == "transient" => " · ⟳ 暂不可达",
        _ => "",
    };
    let mut lines = Vec::with_capacity(data.len().max(1));
    for (i, d) in data.iter().enumerate() {
        let window = match data.len() {
            1 => String::new(),
            _ => format!(
                "{} ",
                d.plan_name
                    .clone()
                    .unwrap_or_else(|| format!("窗口{}", i + 1))
            ),
        };
        let body = if let Some(pct) = used_percent(d) {
            format!("{name} · {window}已用 {}", percent_text(pct))
        } else if let (Some(rem), unit) = (d.remaining, d.unit.clone()) {
            match unit {
                Some(u) if !u.is_empty() => {
                    format!("{name} · {window}剩余 {} {u}", amount_text(rem))
                }
                Some(_) | None => format!("{name} · {window}剩余 {}", amount_text(rem)),
            }
        } else {
            format!("{name} · {window}已获取")
        };
        let over = used_percent(d).is_some_and(|p| p >= f64::from(threshold_percent));
        lines.push(warn(time_suffix(body, state.at), over) + transient_mark);
    }
    if lines.is_empty() {
        lines.push(format!("{name} · 尚无数据"));
    }
    lines
}

/// 是否有条目超过低额度阈值（切换警示图标的依据）。
pub fn any_alert(
    cfg: &AppConfig,
    results: &HashMap<String, EntryState>,
    settings: &Settings,
) -> bool {
    cfg.providers
        .iter()
        .filter(|p| p.enabled)
        .filter_map(|p| results.get(&p.id))
        .filter_map(|st| st.data.as_ref())
        .any(|data| {
            data.iter().filter(|d| d.is_valid != Some(false)).any(|d| {
                used_percent(d)
                    .is_some_and(|p| p >= f64::from(settings.low_balance_threshold_percent))
            })
        })
}

// ---- Tauri 交互 -----------------------------------------------------------

/// 警示托盘图标（构建期嵌入，运行时零开销切换）。
static ALERT_ICON: std::sync::LazyLock<tauri::image::Image<'static>> =
    std::sync::LazyLock::new(|| {
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray-alert.png"))
            .expect("嵌入的警示图标解码失败")
    });

/// 创建托盘（setup 阶段调用一次）。
///
/// 首次启动配置文件不存在是正常路径（load 返回空配置，非 Err）；
/// 真正读盘失败时给诚实提示菜单而非空菜单。
pub fn create(app: &AppHandle, state: &AppState) -> tauri::Result<()> {
    let menu = match snapshot_views(state) {
        Some((cfg, results, settings)) => build_menu(app, &cfg, &results, &settings)?,
        None => {
            let menu = Menu::new(app)?;
            menu.append(&MenuItem::with_id(
                app,
                "info-config-error",
                "配置读取失败，请检查 config.json",
                false,
                None::<&str>,
            )?)?;
            menu.append(&PredefinedMenuItem::separator(app)?)?;
            menu.append(&MenuItem::with_id(
                app,
                "show",
                "打开主窗口",
                true,
                None::<&str>,
            )?)?;
            menu.append(&MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?)?;
            menu
        }
    };
    tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().expect("窗口图标缺失").clone())
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
    let menu = match build_menu(app, &cfg, &results, &settings) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("托盘菜单重建失败：{e}");
            return;
        }
    };
    if let Err(e) = tray.set_menu(Some(menu)) {
        eprintln!("托盘菜单应用失败：{e}");
    }
    let icon = if any_alert(&cfg, &results, &settings) {
        ALERT_ICON.clone()
    } else {
        app.default_window_icon()
            .cloned()
            .unwrap_or_else(|| ALERT_ICON.clone())
    };
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

fn build_menu(
    app: &AppHandle,
    cfg: &AppConfig,
    results: &HashMap<String, EntryState>,
    settings: &Settings,
) -> tauri::Result<Menu<Wry>> {
    let now = now_ms();
    let menu = Menu::new(app)?;
    let entries: Vec<_> = cfg.providers.iter().filter(|p| p.enabled).collect();
    if entries.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "info-empty",
            "暂无启用的供应商",
            false,
            None::<&str>,
        )?)?;
    }
    for entry in entries {
        let lines = match results.get(&entry.id) {
            Some(st) => entry_lines(&entry.name, st, settings.low_balance_threshold_percent, now),
            None => vec![format!("{} · 尚无数据", entry.name)],
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
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "refresh",
        "立即刷新",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "show",
        "打开主窗口",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?)?;
    Ok(menu)
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
        _ => {}
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
        // 悬停：节流触发全量刷新
        TrayIconEvent::Enter { .. } => hover_refresh(app),
        _ => {}
    }
}

fn hover_refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = now_ms();
    let last = state
        .last_hover_refresh_ms
        .load(std::sync::atomic::Ordering::Relaxed);
    if now.saturating_sub(last) < HOVER_THROTTLE_MS {
        return;
    }
    if state
        .last_hover_refresh_ms
        .compare_exchange(
            last,
            now,
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
        )
        .is_err()
    {
        return;
    }
    emit_refresh(app);
}

fn emit_refresh(app: &AppHandle) {
    let _ = app.emit("refresh-now", ());
}

/// 显示并聚焦主窗口（托盘左键 / 单实例回调 / 菜单项共用）。
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

// ---- 契约测试 -------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
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

    /// 契约：余额型行——`名称 · 剩余 62.97 CNY · 3 分钟前`。
    #[test]
    fn balance_line_shape() {
        let st = ok_state(vec![data(Some(62.97), Some("CNY"))], NOW - 180_000);
        assert_eq!(
            entry_lines("DeepSeek", &st, 80, NOW),
            vec!["DeepSeek · 剩余 62.97 CNY · 3 分钟前"]
        );
    }

    /// 契约：百分比型行——unit="%" 时 used 即已用百分比。
    #[test]
    fn percent_line_shape() {
        let st = ok_state(vec![percent_data(Some(42.0))], NOW - 180_000);
        assert_eq!(
            entry_lines("GLM", &st, 80, NOW),
            vec!["GLM · 已用 42% · 3 分钟前"]
        );
    }

    /// 契约：多窗口一窗口一行，窗口名取 planName，缺省回退"窗口N"。
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
        assert_eq!(
            entry_lines("GLM", &st, 80, NOW),
            vec![
                "GLM · five_hour 已用 42% · 5 分钟前",
                "GLM · 窗口2 已用 10% · 5 分钟前",
            ]
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
            }),
        };
        assert_eq!(
            entry_lines("X", &st, 80, NOW),
            vec!["X · 剩余 88.00 CNY · 2 分钟前 · ⟳ 暂不可达"]
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
            }),
        };
        assert_eq!(
            entry_lines("X", &st(in_window), 80, NOW),
            vec!["X · 剩余 88.00 CNY · 9 分钟前 · ⟳ 暂不可达"],
            "窗口内应保留旧值"
        );
        assert_eq!(
            entry_lines("X", &st(exactly), 80, NOW),
            vec!["X · 剩余 88.00 CNY · 10 分钟前 · ⟳ 暂不可达"],
            "恰 10 分钟应在窗口内（前端同语义）"
        );
        assert_eq!(
            entry_lines("X", &st(beyond), 80, NOW),
            vec!["X · ⟳ 网络波动"],
            "超窗应丢弃旧值"
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
            }),
            ..Default::default()
        };
        let line = &entry_lines("X", &st, 80, NOW)[0];
        // X · ⚠ + 60 字符
        assert!(line.chars().count() < 70, "应截断：{line}");
        assert!(!line.contains(&"错".repeat(61)), "不得超出截断上限");
    }

    /// 契约：瞬时失败无旧值 → ⟳ 网络波动；确定性失败 → ⚠ 立即透出（覆盖旧值）。
    #[test]
    fn error_without_data_and_deterministic() {
        let transient = EntryState {
            error: Some(crate::state::ErrorInfo {
                kind: "transient".into(),
                message: "网络中断".into(),
            }),
            ..Default::default()
        };
        assert_eq!(
            entry_lines("X", &transient, 80, NOW),
            vec!["X · ⟳ 网络波动"]
        );

        let deterministic = EntryState {
            data: Some(vec![data(Some(5.0), Some("CNY"))]),
            at: Some(NOW),
            error: Some(crate::state::ErrorInfo {
                kind: "deterministic".into(),
                message: "HTTP 401: Unauthorized".into(),
            }),
        };
        assert_eq!(
            entry_lines("X", &deterministic, 80, NOW),
            vec!["X · ⚠ HTTP 401: Unauthorized"]
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
        assert_eq!(
            entry_lines("X", &st, 80, NOW),
            vec!["X · ⚠ 已失效：key 已过期"]
        );
    }

    /// 契约：超阈值行首加 ⚠（恰等于阈值触发）。
    #[test]
    fn threshold_adds_warning_prefix() {
        let st = ok_state(vec![percent_data(Some(80.0))], NOW);
        assert_eq!(
            entry_lines("X", &st, 80, NOW),
            vec!["⚠ X · 已用 80% · 刚刚"]
        );
        let below = ok_state(vec![percent_data(Some(79.9))], NOW);
        assert_eq!(
            entry_lines("X", &below, 80, NOW),
            vec!["X · 已用 80% · 刚刚"]
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

    /// 契约：相对时间分档。
    #[test]
    fn relative_time_buckets() {
        assert_eq!(relative_time(NOW - 5_000, NOW), "刚刚");
        assert_eq!(relative_time(NOW - 30_000, NOW), "30 秒前");
        assert_eq!(relative_time(NOW - 180_000, NOW), "3 分钟前");
        assert_eq!(relative_time(NOW - 7_200_000, NOW), "2 小时前");
        assert_eq!(relative_time(NOW - 172_800_000, NOW), "2 天前");
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
            base_url: None,
        };
        let cfg = AppConfig {
            providers: vec![entry("a", true), entry("b", false)],
        };
        let settings = Settings::default(); // 阈值 80

        let mut results = HashMap::new();
        results.insert("a".into(), ok_state(vec![percent_data(Some(85.0))], NOW));
        results.insert("b".into(), ok_state(vec![percent_data(Some(95.0))], NOW));
        assert!(
            any_alert(&cfg, &results, &settings),
            "enabled 条目超阈值应告警"
        );

        results.insert("a".into(), ok_state(vec![percent_data(Some(50.0))], NOW));
        assert!(
            !any_alert(&cfg, &results, &settings),
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
            !any_alert(&cfg, &results, &settings),
            "失效条目不触发额度告警"
        );
    }
}
