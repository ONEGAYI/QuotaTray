//! 托盘悬停面板：位置计算、显隐调度与窗口命令。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "tray-hover";
const PANEL_WIDTH: f64 = 374.0;
const PANEL_HEIGHT: f64 = 520.0;
const PANEL_GAP: i32 = 8;
const HIDE_DELAY_MS: u64 = 450;

/// rect 可信判定容差（物理像素）：光标落在 rect 外扩此范围内即认为
/// rect 描述的是真实图标位置（正常悬停时光标必在图标内）。
const TRUST_SLACK: i32 = 8;

#[derive(Default)]
pub struct HoverPanelState {
    generation: AtomicU64,
    tray_inside: AtomicBool,
    panel_inside: AtomicBool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PhysicalBox {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn panel_position(
    tray: PhysicalBox,
    work_area: PhysicalBox,
    panel_width: u32,
    panel_height: u32,
    gap: i32,
) -> PhysicalPosition<i32> {
    #[derive(Clone, Copy)]
    enum Edge {
        Top,
        Right,
        Bottom,
        Left,
    }

    let work_right = work_area.x + work_area.width as i32;
    let work_bottom = work_area.y + work_area.height as i32;
    let tray_right = tray.x + tray.width as i32;
    let tray_bottom = tray.y + tray.height as i32;
    let tray_center_x = tray.x + tray.width as i32 / 2;
    let tray_center_y = tray.y + tray.height as i32 / 2;

    let edge = if tray_bottom <= work_area.y {
        Edge::Top
    } else if tray.y >= work_bottom {
        Edge::Bottom
    } else if tray_right <= work_area.x {
        Edge::Left
    } else if tray.x >= work_right {
        Edge::Right
    } else {
        let candidates = [
            ((tray_center_y - work_area.y).abs(), Edge::Top),
            ((work_right - tray_center_x).abs(), Edge::Right),
            ((work_bottom - tray_center_y).abs(), Edge::Bottom),
            ((tray_center_x - work_area.x).abs(), Edge::Left),
        ];
        candidates
            .into_iter()
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, edge)| edge)
            .unwrap_or(Edge::Bottom)
    };

    let panel_width = panel_width as i32;
    let panel_height = panel_height as i32;
    let clamp_x = |x: i32| {
        let max = (work_right - panel_width).max(work_area.x);
        x.clamp(work_area.x, max)
    };
    let clamp_y = |y: i32| {
        let max = (work_bottom - panel_height).max(work_area.y);
        y.clamp(work_area.y, max)
    };

    let (x, y) = match edge {
        Edge::Top => (clamp_x(tray_center_x - panel_width / 2), work_area.y + gap),
        Edge::Bottom => (
            clamp_x(tray_center_x - panel_width / 2),
            work_bottom - panel_height - gap,
        ),
        Edge::Left => (work_area.x + gap, clamp_y(tray_center_y - panel_height / 2)),
        Edge::Right => (
            work_right - panel_width - gap,
            clamp_y(tray_center_y - panel_height / 2),
        ),
    };
    PhysicalPosition::new(x, y)
}

/// 托盘事件 rect 是否描述真实图标位置。
///
/// Windows 对隐藏托盘（overflow flyout）内的图标会把事件 rect 映射为
/// 任务栏 chevron 位置——此时光标（悬停在 flyout 内的图标上）必然落在
/// rect 之外，以此区分两种场景。
fn rect_is_trusted(tray: PhysicalBox, cursor: PhysicalPosition<i32>) -> bool {
    let (x, y) = (cursor.x, cursor.y);
    x >= tray.x - TRUST_SLACK
        && x <= tray.x + tray.width as i32 + TRUST_SLACK
        && y >= tray.y - TRUST_SLACK
        && y <= tray.y + tray.height as i32 + TRUST_SLACK
}

/// rect 不可信（隐藏托盘场景）的兜底定位：面板贴光标侧方，
/// 垂直以光标为中心并 clamp 进工作区。
///
/// 硬约束：面板不得覆盖光标点——覆盖会顶掉托盘图标区的鼠标命中，
/// 触发 Leave/Enter 连锁闪烁。水平方向选空间充足的一侧：右侧放得下
/// 面板宽 + gap 就放右，否则放左（两侧都放不下时 clamp 兜底，
/// 光标远离工作区边缘的实际场景不会发生）。
fn cursor_anchored_position(
    cursor: PhysicalPosition<i32>,
    work_area: PhysicalBox,
    panel_width: u32,
    panel_height: u32,
    gap: i32,
) -> PhysicalPosition<i32> {
    let work_right = work_area.x + work_area.width as i32;
    let work_bottom = work_area.y + work_area.height as i32;
    let panel_width = panel_width as i32;
    let panel_height = panel_height as i32;

    let x = if work_right - cursor.x >= panel_width + gap {
        cursor.x + gap
    } else {
        cursor.x - panel_width - gap
    };
    let y = cursor.y - panel_height / 2;
    let max_x = (work_right - panel_width).max(work_area.x);
    let max_y = (work_bottom - panel_height).max(work_area.y);
    PhysicalPosition::new(
        x.clamp(work_area.x, max_x),
        y.clamp(work_area.y, max_y),
    )
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?view=tray-hover".into()),
    )
    .title("QuotaTray")
    .inner_size(PANEL_WIDTH, PANEL_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(true)
    .focused(false)
    .visible(false)
    .build()?;
    Ok(())
}

fn physical_box(rect: Rect) -> PhysicalBox {
    let position = rect.position.to_physical::<i32>(1.0);
    let size = rect.size.to_physical::<u32>(1.0);
    PhysicalBox {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }
}

fn work_area_box(app: &AppHandle, tray: PhysicalBox) -> Option<PhysicalBox> {
    let center_x = tray.x as f64 + f64::from(tray.width) / 2.0;
    let center_y = tray.y as f64 + f64::from(tray.height) / 2.0;
    let monitor = app
        .monitor_from_point(center_x, center_y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())?;
    let area = monitor.work_area();
    Some(PhysicalBox {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width,
        height: area.size.height,
    })
}

pub fn tray_enter(app: &AppHandle, rect: Rect) {
    let state = app.state::<HoverPanelState>();
    state.tray_inside.store(true, Ordering::SeqCst);
    state.generation.fetch_add(1, Ordering::SeqCst);

    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let tray = physical_box(rect);
    let cursor = app
        .cursor_position()
        .ok()
        .map(|p| PhysicalPosition::new(p.x as i32, p.y as i32));
    if let (Some(work_area), Ok(panel_size)) = (work_area_box(app, tray), window.outer_size()) {
        let position = match cursor {
            // 隐藏托盘：rect 是任务栏 chevron，改以光标（flyout 内图标上）锚定
            Some(c) if !rect_is_trusted(tray, c) => cursor_anchored_position(
                c,
                work_area,
                panel_size.width,
                panel_size.height,
                PANEL_GAP,
            ),
            // 正常任务栏，或光标取不到时保守信任 rect（与历史行为一致）
            _ => panel_position(
                tray,
                work_area,
                panel_size.width,
                panel_size.height,
                PANEL_GAP,
            ),
        };
        let _ = window.set_position(position);
    }
    let _ = window.show();
    raise_to_topmost(&window);
}

/// 重新置顶（不激活）：面板窗口随应用启动创建（隐藏不销毁），Explorer
/// 的隐藏托盘 flyout 后弹出时 z-order 更高会遮挡面板——show 后重插
/// topmost 组顶夺回。SWP_NOACTIVATE 防止抢走 flyout 前台使其收起。
#[cfg(windows)]
fn raise_to_topmost(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    if let Ok(hwnd) = window.hwnd() {
        unsafe {
            SetWindowPos(
                hwnd.0,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

#[cfg(not(windows))]
fn raise_to_topmost(_window: &tauri::WebviewWindow) {}

pub fn tray_leave(app: &AppHandle) {
    let state = app.state::<HoverPanelState>();
    state.tray_inside.store(false, Ordering::SeqCst);
    schedule_hide(app.clone());
}

fn schedule_hide(app: AppHandle) {
    let generation = app
        .state::<HoverPanelState>()
        .generation
        .fetch_add(1, Ordering::SeqCst)
        + 1;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(HIDE_DELAY_MS)).await;
        let state = app.state::<HoverPanelState>();
        if should_hide(
            generation,
            state.generation.load(Ordering::SeqCst),
            state.tray_inside.load(Ordering::SeqCst),
            state.panel_inside.load(Ordering::SeqCst),
        ) {
            hide(&app);
        }
    });
}

fn should_hide(
    scheduled_generation: u64,
    current_generation: u64,
    tray_inside: bool,
    panel_inside: bool,
) -> bool {
    scheduled_generation == current_generation && !tray_inside && !panel_inside
}

pub fn hide(app: &AppHandle) {
    let state = app.state::<HoverPanelState>();
    state.tray_inside.store(false, Ordering::SeqCst);
    state.panel_inside.store(false, Ordering::SeqCst);
    state.generation.fetch_add(1, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn set_hover_panel_pointer_inside(app: AppHandle, inside: bool) {
    let state = app.state::<HoverPanelState>();
    state.panel_inside.store(inside, Ordering::SeqCst);
    if inside {
        state.generation.fetch_add(1, Ordering::SeqCst);
    } else {
        schedule_hide(app);
    }
}

#[tauri::command]
pub fn hide_hover_panel(app: AppHandle) {
    hide(&app);
}

#[tauri::command]
pub fn open_main_window(app: AppHandle) {
    hide(&app);
    crate::tray::show_main(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bottom_taskbar_places_panel_above_and_clamps_to_work_area() {
        let tray = PhysicalBox {
            x: 1870,
            y: 1040,
            width: 24,
            height: 24,
        };
        let work = PhysicalBox {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };

        assert_eq!(
            panel_position(tray, work, 374, 520, 8),
            PhysicalPosition::new(1546, 512),
        );
    }

    #[test]
    fn top_and_vertical_taskbars_use_the_available_side() {
        let top_tray = PhysicalBox {
            x: 900,
            y: 0,
            width: 24,
            height: 24,
        };
        let top_work = PhysicalBox {
            x: 0,
            y: 40,
            width: 1920,
            height: 1040,
        };
        assert_eq!(
            panel_position(top_tray, top_work, 374, 520, 8),
            PhysicalPosition::new(725, 48),
        );

        let left_tray = PhysicalBox {
            x: 0,
            y: 500,
            width: 24,
            height: 24,
        };
        let left_work = PhysicalBox {
            x: 40,
            y: 0,
            width: 1880,
            height: 1080,
        };
        assert_eq!(
            panel_position(left_tray, left_work, 374, 520, 8),
            PhysicalPosition::new(48, 252),
        );
    }

    #[test]
    fn delayed_hide_is_cancelled_by_new_hover_or_panel_entry() {
        assert!(should_hide(7, 7, false, false));
        assert!(!should_hide(7, 8, false, false));
        assert!(!should_hide(7, 7, true, false));
        assert!(!should_hide(7, 7, false, true));
    }

    /// 契约：rect 可信判定——光标在 rect（含 8px 容差）内即可信
    /// （正常任务栏悬停）；光标远离 rect（隐藏托盘：rect 是 chevron，
    /// 光标在 flyout 图标上）不可信。
    #[test]
    fn rect_trust_follows_cursor_containment() {
        let tray = PhysicalBox {
            x: 1870,
            y: 1040,
            width: 24,
            height: 24,
        };
        // rect 范围 x∈[1870,1894] y∈[1040,1064]，容差后 x∈[1862,1902] y∈[1032,1072]
        assert!(rect_is_trusted(tray, PhysicalPosition::new(1882, 1052))); // 图标中心
        assert!(rect_is_trusted(tray, PhysicalPosition::new(1898, 1066))); // 容差边界内
        assert!(!rect_is_trusted(tray, PhysicalPosition::new(1898, 1080))); // y 超容差
        assert!(!rect_is_trusted(tray, PhysicalPosition::new(1820, 990))); // flyout 内图标
    }

    /// 契约：光标锚点兜底定位——面板贴光标侧方（右侧空间不足放左）、
    /// 垂直以光标为中心 clamp 进工作区，且面板矩形不覆盖光标点。
    #[test]
    fn cursor_anchor_places_panel_beside_cursor_without_covering_it() {
        let work = PhysicalBox {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };

        // 典型隐藏托盘：光标在屏幕右下的 flyout 图标上（右侧空间 120 < 374+8）
        let cursor = PhysicalPosition::new(1800, 900);
        let pos = cursor_anchored_position(cursor, work, 374, 520, 8);
        assert_eq!(pos, PhysicalPosition::new(1800 - 374 - 8, 1040 - 520));

        // 右侧空间充足（1720 ≥ 382）：放光标右侧
        let cursor = PhysicalPosition::new(200, 900);
        let pos = cursor_anchored_position(cursor, work, 374, 520, 8);
        assert_eq!(pos, PhysicalPosition::new(208, 1040 - 520));

        // 垂直居中无需 clamp；且各场景面板矩形均不包含光标点
        let cursor = PhysicalPosition::new(1800, 780);
        let pos = cursor_anchored_position(cursor, work, 374, 520, 8);
        assert_eq!(pos, PhysicalPosition::new(1800 - 374 - 8, 780 - 520 / 2));
        for (cursor, pos) in [
            (PhysicalPosition::new(1800, 900), PhysicalPosition::new(1418, 520)),
            (PhysicalPosition::new(200, 900), PhysicalPosition::new(208, 520)),
            (PhysicalPosition::new(1800, 780), PhysicalPosition::new(1418, 520)),
        ] {
            let covers_x = pos.x <= cursor.x && cursor.x < pos.x + 374;
            let covers_y = pos.y <= cursor.y && cursor.y < pos.y + 520;
            assert!(
                !(covers_x && covers_y),
                "面板({pos:?})不得覆盖光标({cursor:?})"
            );
        }
    }
}
