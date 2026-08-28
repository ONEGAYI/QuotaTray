//! Android 没有托盘悬停面板；IPC 形状保留为兼容 no-op。

#[derive(Default)]
pub struct HoverPanelState;

pub fn hide(_app: &tauri::AppHandle) {}

#[tauri::command]
pub fn set_hover_panel_pointer_inside(_app: tauri::AppHandle, inside: bool) {
    let _ = inside;
}

#[tauri::command]
pub fn hide_hover_panel(_app: tauri::AppHandle) {}

#[tauri::command]
pub fn open_main_window(app: tauri::AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
