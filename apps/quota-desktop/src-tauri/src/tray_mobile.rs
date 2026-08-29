//! 移动端无系统托盘：保留命令侧调用形状，所有重建均为显式 no-op。

use tauri::AppHandle;

use crate::state::AppState;

pub fn rebuild(_app: &AppHandle, _state: &AppState) {}

/// 桌面版由常驻调度器（spawn_scheduler）调用；android 无调度器，
/// 保留为跨端形状替身。
#[allow(dead_code)]
pub fn rebuild_on_peak_flip(_app: &AppHandle, _state: &AppState) {}
