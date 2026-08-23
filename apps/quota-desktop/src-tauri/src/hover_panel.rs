//! 托盘悬停面板：位置计算、显隐调度与窗口命令。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "tray-hover";
const PANEL_WIDTH: f64 = 374.0;
const PANEL_HEIGHT: f64 = 520.0;
const PANEL_GAP: i32 = 8;
const HIDE_DELAY_MS: u64 = 450;

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
    if let (Some(work_area), Ok(panel_size)) = (work_area_box(app, tray), window.outer_size()) {
        let position = panel_position(
            tray,
            work_area,
            panel_size.width,
            panel_size.height,
            PANEL_GAP,
        );
        let _ = window.set_position(position);
    }
    let _ = window.show();
}

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
}
