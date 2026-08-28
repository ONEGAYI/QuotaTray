//! 移动端无系统托盘：保留命令侧调用形状，所有重建均为显式 no-op。

use tauri::AppHandle;

use crate::state::AppState;

pub fn rebuild(_app: &AppHandle, _state: &AppState) {}

pub fn rebuild_on_peak_flip(_app: &AppHandle, _state: &AppState) {}
