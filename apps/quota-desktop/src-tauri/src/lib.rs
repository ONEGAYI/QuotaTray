//! quota-desktop 桌面端（Tauri 2）：主进程入口。
//!
//! 职责（GUI-spec §1：GUI 是薄层，业务在 core）：
//! - 组装 Tauri：单实例（首位注册）、自启、托盘、窗口事件、IPC 命令；
//! - 初始化 [`AppState`]（保险库 / 引擎 / 设置 / 快照恢复）；
//! - 窗口关闭 = 隐藏收托盘，退出只走托盘菜单（退出时清理托盘图标）。

mod commands;
mod settings;
mod snapshot;
mod state;
mod tray;

use tauri::Manager;

/// 数据目录调试参数：`--data-dir <path>` 覆盖 `~/.quotatray`（烟测隔离用）。
fn parse_data_dir() -> Option<std::path::PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--data-dir" {
            return args.next().map(std::path::PathBuf::from);
        }
    }
    None
}

pub fn run() {
    let data_dir = parse_data_dir();
    tauri::Builder::default()
        // 单实例必须首位注册：第二实例启动即回调后退出
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .setup(move |app| {
            let state = state::AppState::init(data_dir.clone())?;
            app.manage(state);
            // 托盘首屏即渲染快照（消除重启空窗）
            let state = app.state::<state::AppState>();
            tray::create(app.handle(), &state).map_err(|e| format!("托盘初始化失败：{e}"))?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 关闭按钮 = 隐藏收托盘；真正退出只走托盘菜单
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::upsert_provider,
            commands::remove_provider,
            commands::list_native_metas,
            commands::validate_template,
            commands::test_template,
            commands::query_provider,
            commands::get_settings,
            commands::save_settings,
            commands::get_snapshots,
        ])
        .run(tauri::generate_context!())
        .expect("QuotaTray 启动失败");
}
