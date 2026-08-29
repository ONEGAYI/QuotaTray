//! Android 没有托盘悬停面板；IPC 形状保留为兼容 no-op。

#[derive(Default)]
pub struct HoverPanelState {
    /// 占位字段：桌面版为多字段 Atomic 状态；保持非 unit 形状使
    /// lib.rs 的 HoverPanelState::default() 调用不触发 unit-struct
    /// clippy（default_constructed_unit_structs）
    _placeholder: (),
}

/// 桌面版由托盘 show_main 路径调用；android 调用链不达（无托盘），
/// 保留为跨端形状替身。
#[allow(dead_code)]
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
