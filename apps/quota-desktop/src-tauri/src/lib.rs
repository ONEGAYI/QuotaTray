//! quota-desktop 桌面端（Tauri 2）：主进程入口。
//!
//! 职责（GUI-spec §1：GUI 是薄层，业务在 core）：
//! - 组装 Tauri：单实例（首位注册）、自启、托盘、悬停浮窗、窗口事件、IPC 命令；
//! - 初始化 [`AppState`]（保险库 / 引擎 / 设置 / 快照恢复）；
//! - 窗口关闭 = 隐藏收托盘，退出只走托盘菜单（退出时清理托盘图标）。

mod commands;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod hover_panel;
#[cfg(any(target_os = "android", target_os = "ios"))]
#[path = "hover_panel_mobile.rs"]
mod hover_panel;
mod i18n;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod ring;
mod settings;
mod snapshot;
mod state;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod tray;
#[cfg(any(target_os = "android", target_os = "ios"))]
#[path = "tray_mobile.rs"]
mod tray;
mod update_ctl;

use quota_core::{RuntimeMode, SecretStore};
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

/// 便携显式参数：`--portable` 选择 exe 旁 `Data/`（与 `--data-dir` 同现
/// 时后者赢——烟测沙箱保持安装态，优先级契约见 core::runtime）。
fn parse_portable_flag() -> bool {
    std::env::args().skip(1).any(|arg| arg == "--portable")
}

/// 解析运行形态：exe 旁 `portable.marker` 自动检测（无显式参数时）。
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn resolve_runtime_mode() -> Result<RuntimeMode, String> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .ok_or_else(|| "无法定位可执行文件所在目录".to_string())?;
    Ok(quota_core::resolve_mode(
        parse_data_dir(),
        parse_portable_flag(),
        &exe_dir,
    ))
}

fn runtime_mode_for_app(
    app: &tauri::AppHandle,
    _startup_mode: &RuntimeMode,
) -> Result<RuntimeMode, String> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let root = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("无法定位应用私有数据目录：{e}"))?;
        Ok(RuntimeMode::Installed {
            data_dir: Some(root),
        })
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = app;
        Ok(_startup_mode.clone())
    }
}

/// 界面装配段：悬停窗、托盘与更新调度。**必须在主线程、且不在 WebView2
/// IPC 调用栈内执行**——Windows 上同步命令跑在主线程 IPC 栈里，
/// `run_on_main_thread` 对主线程调用方又是同步直执（非异步入队），
/// 栈内同步 build WebView2 等待一个需要主线程泵消息的初始化即死锁
/// （实测 P0：便携确认后 invoke 永不返回、确认页按钮灰死）。
/// 两个入口均满足该约束：setup 回调（事件循环启动前的干净主线程上下
/// 文）；confirm_portable_init 为 async 命令，从线程池经
/// run_on_main_thread 异步入队，主线程退栈回泵后才执行本函数。
fn setup_surfaces(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        hover_panel::create(app).map_err(|e| format!("悬停面板初始化失败：{e}"))?;
        // 托盘首屏即渲染快照（消除重启空窗）
        let state = app.state::<state::AppState>();
        tray::create(app, &state).map_err(|e| format!("托盘初始化失败：{e}"))?;
        // 更新检测调度：启动后一分钟的首次 wake 即覆盖「启动时检测」
        update_ctl::spawn_scheduler(app.clone());
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let _ = app;
    Ok(())
}

/// setup 完成段：正常启动路径（setup 回调内、已在主线程）同步装配。
fn finish_setup(
    app: &tauri::AppHandle,
    state: state::AppState,
) -> Result<(), Box<dyn std::error::Error>> {
    app.manage(state);
    app.manage(hover_panel::HoverPanelState::default());
    setup_surfaces(app).map_err(Into::into)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let mode = match resolve_runtime_mode() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let mode = RuntimeMode::Installed { data_dir: None };
    // 便携形态：WebView2 用户数据（缓存/DOM Storage）定向到 Data/WebView2，
    // 主窗与悬停窗共用——进程级环境变量必须在任何 WebView 创建前设置，
    // 否则数据落到 %LOCALAPPDATA%（便携数据外溢）。目录创建失败仍设置
    // 变量（WebView2 可能自建成功）；若 WebView2 环境最终无法在该目录
    // 创建（如只读介质），窗口创建失败、启动失败——fail-fast 优于把
    // 便携数据静默外溢到系统目录
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if let RuntimeMode::Portable { root } = &mode {
        let webview_dir = root.join("WebView2");
        if let Err(e) = std::fs::create_dir_all(&webview_dir) {
            eprintln!("WebView2 数据目录创建失败（将由运行时自建或回退系统默认）：{e}");
        }
        // 安全性：此刻位于 run() 最开头、Builder 构造之前，Tauri 运行时
        // 尚未 spawn 任何线程，进程内无并发读环境变量者——set_var 的
        // 唯一调用窗口，且必须先于任何 WebView 创建
        unsafe {
            std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview_dir);
        }
    }
    // 便携首启门控：密钥缺失（未初始化）时延后全部状态初始化，
    // 前端确认页通过 confirm_portable_init 补齐；密钥损坏（Err）不进
    // 门控，走正常 init 把带处置指引的错误透给用户
    let pending_portable_init = match &mode {
        RuntimeMode::Portable { root } => {
            quota_core::FileStore::new(quota_core::portable_key_path(root))
                .get()
                .map(|key| key.is_none())
                .unwrap_or(false)
        }
        RuntimeMode::Installed { .. } => false,
    };
    let gate_mode = if pending_portable_init {
        Some(mode.clone())
    } else {
        None
    };
    let builder = tauri::Builder::default();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let builder = builder
        // 单实例必须首位注册：第二实例启动即回调后退出。
        // 取舍：插件 Windows 实现是会话命名空间 mutex（{identifier}-sim），
        // 非 spec 提及的 Global\ 跨会话形态——同机同用户单 GUI 的目标场景下
        // 语义等价（官方跨平台实现，D4 决策），跨登录会话双开不在防御范围。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 聚焦已有实例并向前端广播：让用户明白「为什么新点的没打开」
            tray::show_main(app);
            let _ = tauri::Emitter::emit(app, "instance-already-running", ());
        }))
        .plugin(tauri_plugin_autostart::Builder::new().build());
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let builder = builder.plugin(tauri_plugin_fs::init());
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let mode = runtime_mode_for_app(app.handle(), &mode)?;
            if let Some(mode) = gate_mode {
                // 便携首启：仅托管门控，AppState/托盘/调度器待确认后补齐。
                // HoverPanelState 必须此刻托管：single-instance 回调（确认页
                // 期间二次启动 exe 即触发）在主线程调 tray::show_main →
                // hover_panel::hide，未托管会 panic 直接崩掉首实例
                app.manage(state::BootGate {
                    pending: std::sync::Mutex::new(Some(mode)),
                });
                app.manage(hover_panel::HoverPanelState::default());
                return Ok(());
            }
            let state = match state::AppState::init(mode.clone()) {
                Ok(state) => state,
                Err(e) => {
                    // release 无控制台，eprintln 用户不可见：致命错误弹窗
                    // 透出（密钥损坏等场景的处置指引写在 FileStore 文案里）
                    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                    app.dialog()
                        .message(format!(
                            "QuotaTray 启动失败：
{e}"
                        ))
                        .kind(MessageDialogKind::Error)
                        .title("QuotaTray")
                        .blocking_show();
                    return Err(e.into());
                }
            };
            finish_setup(app.handle(), state)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 便携首启门控期间无托盘：放行关闭 = 真退出
                // （否则隐藏后成无图标僵尸进程，只能再启动一次唤回）
                let gating = window
                    .app_handle()
                    .try_state::<state::BootGate>()
                    .is_some_and(|gate| gate.pending.lock().unwrap().is_some());
                if gating {
                    window.app_handle().exit(0);
                    return;
                }
                // 关闭按钮 = 隐藏收托盘；真正退出只走托盘菜单
                let _ = window.hide();
                api.prevent_close();
            }
            #[cfg(any(target_os = "android", target_os = "ios"))]
            let _ = (window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::write_assist_package,
            commands::resolve_quota_cli_path,
            commands::export_configuration,
            commands::import_configuration,
            commands::upsert_provider,
            commands::remove_provider,
            commands::reorder_providers,
            commands::clear_all_data,
            commands::list_native_metas,
            commands::validate_template,
            commands::test_template,
            commands::validate_script,
            commands::test_script,
            commands::query_provider,
            commands::get_provider_state,
            commands::get_history,
            commands::get_settings,
            commands::save_settings,
            commands::patch_settings,
            commands::set_resolved_theme,
            commands::get_snapshots,
            commands::get_boot_state,
            commands::confirm_portable_init,
            commands::cancel_portable_init,
            commands::open_update_dir,
            commands::get_update_state,
            commands::check_update_now,
            commands::download_update,
            commands::install_update,
            hover_panel::set_hover_panel_pointer_inside,
            hover_panel::hide_hover_panel,
            hover_panel::open_main_window,
        ])
        .run(tauri::generate_context!())
        .expect("QuotaTray 启动失败");
}
