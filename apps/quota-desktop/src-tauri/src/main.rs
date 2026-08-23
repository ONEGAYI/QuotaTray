// release 构建隐藏控制台窗口（debug 保留，便于烟测观察日志）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    quota_desktop_lib::run()
}
