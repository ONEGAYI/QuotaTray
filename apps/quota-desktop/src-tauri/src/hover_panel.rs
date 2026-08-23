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

/// 托盘事件 rect 是否可作为定位锚点。
///
/// 判定依据是布局事实而非 rect 数值本身（Windows 对隐藏托盘 overflow
/// 图元上报的 rect 或是任务栏 chevron、或是 flyout 内图标坐标，版本
/// 间行为不一，都不可依赖）：任务栏图标必然悬停于**工作区之外**
/// （任务栏占用的区域已从工作区扣除），而隐藏托盘 flyout 弹出在
/// **工作区内部**——光标严格落在工作区内即说明悬停的是 flyout 内
/// 图标。恰落在工作区边界按任务栏处理（DPI 取整毛刺落在边界上）。
fn rect_is_trusted(cursor: PhysicalPosition<i32>, work_area: PhysicalBox) -> bool {
    let inside = cursor.x > work_area.x
        && cursor.x < work_area.x + work_area.width as i32
        && cursor.y > work_area.y
        && cursor.y < work_area.y + work_area.height as i32;
    !inside
}

/// rect 不可信（隐藏托盘场景）的兜底定位：面板出现在光标（即 flyout 内
/// 图标）**上方**，水平以光标为中心；光标上方空间不足时回退到下方。
///
/// 垂直避让 `icon_extent`（取自 rect 高度）：光标悬停在图标内任意位置
/// 时，图标顶部最多高出光标一个图标高——面板底边与光标间垂直让开
/// `icon_extent` + gap，保证**图标整体**不被面板遮挡（仅避开光标点时
/// 图标上半截仍会被盖住）。上方放得下面板高 + 避让 + gap 就放上方
/// （y = 光标 − 避让 − 面板高 − gap），否则放下方（y = 光标 + 避让 +
/// gap）；水平垂直均 clamp 进工作区兜底。
fn cursor_anchored_position(
    cursor: PhysicalPosition<i32>,
    work_area: PhysicalBox,
    panel_width: u32,
    panel_height: u32,
    icon_extent: i32,
    gap: i32,
) -> PhysicalPosition<i32> {
    let work_right = work_area.x + work_area.width as i32;
    let work_bottom = work_area.y + work_area.height as i32;
    let panel_width = panel_width as i32;
    let panel_height = panel_height as i32;

    let x = cursor.x - panel_width / 2;
    let y = if cursor.y - work_area.y >= panel_height + icon_extent + gap {
        cursor.y - icon_extent - panel_height - gap
    } else {
        cursor.y + icon_extent + gap
    };
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

fn work_area_at(app: &AppHandle, x: f64, y: f64) -> Option<PhysicalBox> {
    let monitor = app
        .monitor_from_point(x, y)
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
    // 锚定显示器：优先光标（悬停时光标必在图标/flyout 上），取不到时
    // 退回 rect 中心
    let (anchor_x, anchor_y) = cursor
        .map(|c| (c.x as f64, c.y as f64))
        .unwrap_or_else(|| {
            (
                tray.x as f64 + f64::from(tray.width) / 2.0,
                tray.y as f64 + f64::from(tray.height) / 2.0,
            )
        });
    if let (Some(work_area), Ok(panel_size)) =
        (work_area_at(app, anchor_x, anchor_y), window.outer_size())
    {
        // 垂直避让量：图标高度估计（rect 高度，下限 16 防 rect 尺寸异常）
        let icon_extent = (tray.height as i32).max(16);
        let position = match cursor {
            // 光标严格在工作区内部 = 悬停的是隐藏托盘 flyout 内的图标
            // （此时无论 rect 报 chevron 还是 flyout 坐标都不可作锚），
            // 面板出现在图标上方（垂直让开整个图标高度，不遮挡图标）
            Some(c) if !rect_is_trusted(c, work_area) => cursor_anchored_position(
                c,
                work_area,
                panel_size.width,
                panel_size.height,
                icon_extent,
                PANEL_GAP,
            ),
            // 正常任务栏（光标在工作区之外）；光标取不到时保守按 rect
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

    /// 契约：rect 可信判定——任务栏图标必然悬停于工作区之外（可信，
    /// 走四边定位）；隐藏托盘 flyout 弹出在工作区内部，悬停其内图标时
    /// 光标严格落在工作区内（不可信，无论 rect 报 chevron 还是 flyout
    /// 坐标都改走光标锚定）。恰落在工作区边界按任务栏处理（DPI 毛刺）。
    #[test]
    fn rect_trust_follows_cursor_work_area_containment() {
        let work = PhysicalBox {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        // 底部/顶部/左侧任务栏图标（工作区之外）
        assert!(rect_is_trusted(PhysicalPosition::new(1882, 1052), work));
        assert!(rect_is_trusted(PhysicalPosition::new(960, -12), work));
        assert!(rect_is_trusted(PhysicalPosition::new(-5, 500), work));
        // 工作区边界：按任务栏处理
        assert!(rect_is_trusted(PhysicalPosition::new(0, 500), work));
        assert!(rect_is_trusted(PhysicalPosition::new(1920, 500), work));
        // flyout 内图标 / 工作区内部任意点
        assert!(!rect_is_trusted(PhysicalPosition::new(1800, 900), work));
        assert!(!rect_is_trusted(PhysicalPosition::new(100, 100), work));
    }

    /// 契约：光标锚定兜底定位——面板出现在光标（图标）上方、水平以光标
    /// 为中心（clamp 进工作区）；垂直让开整个图标高度（不遮挡图标本身，
    /// 光标悬停在图标内任意位置时其顶部最多高出光标一个图标高）；光标
    /// 上方空间不足回退到下方（顶边同样让开图标高度）。
    #[test]
    fn cursor_anchor_places_panel_above_icon_without_covering_it() {
        let work = PhysicalBox {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };
        let icon = 32; // 图标避让高度

        // 典型隐藏托盘：光标在屏幕右下的 flyout 图标上（上方空间 900 ≥
        // 520+32+8），水平 clamp 到工作区（1800-187=1613 > 1546）
        let cursor = PhysicalPosition::new(1800, 900);
        let pos = cursor_anchored_position(cursor, work, 374, 520, icon, 8);
        assert_eq!(pos, PhysicalPosition::new(1546, 900 - 32 - 520 - 8));

        // 上方恰好放得下（边界 == 面板高 + 避让 + gap）：贴工作区顶部
        let cursor = PhysicalPosition::new(960, 560);
        let pos = cursor_anchored_position(cursor, work, 374, 520, icon, 8);
        assert_eq!(pos, PhysicalPosition::new(960 - 374 / 2, 0));

        // 上方空间不足（400 < 560）：回退到图标下方（顶边让开图标高度）
        let cursor = PhysicalPosition::new(960, 400);
        let pos = cursor_anchored_position(cursor, work, 374, 520, icon, 8);
        assert_eq!(pos, PhysicalPosition::new(960 - 374 / 2, 400 + 32 + 8));

        // 各场景面板垂直范围均避开图标带（光标 ± 图标高度）
        for (cursor, pos) in [
            (PhysicalPosition::new(1800, 900), PhysicalPosition::new(1546, 340)),
            (PhysicalPosition::new(960, 560), PhysicalPosition::new(773, 0)),
            (PhysicalPosition::new(960, 400), PhysicalPosition::new(773, 440)),
        ] {
            let panel_top = pos.y;
            let panel_bottom = pos.y + 520;
            assert!(
                panel_bottom <= cursor.y - icon || panel_top >= cursor.y + icon,
                "面板({pos:?})垂直范围 [{panel_top},{panel_bottom}] 不得覆盖图标带（光标 {cursor:?} ± {icon}）"
            );
        }
    }
}
