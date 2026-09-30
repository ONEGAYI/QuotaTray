//! 桌面启动边界：系统自启参数、窗口初始可见性与自启意图同步。

pub(crate) const AUTOSTART_ARG: &str = "--autostart";

/// 主窗口创建期间也可能收到单实例消息；手动唤起必须等到 setup 就绪。
#[derive(Default)]
pub(crate) struct StartupActivation {
    state: std::sync::Mutex<ActivationState>,
}

#[derive(Default)]
struct ActivationState {
    ready: bool,
    pending: bool,
}

impl StartupActivation {
    /// 返回 true 时可立即显示；否则由 mark_ready 补发。
    pub(crate) fn request_manual(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.ready {
            true
        } else {
            state.pending = true;
            false
        }
    }

    /// setup 完成时返回是否有待处理的手动唤起请求。
    pub(crate) fn mark_ready(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        state.ready = true;
        std::mem::take(&mut state.pending)
    }
}

#[cfg(windows)]
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutostartAction {
    Enable,
    Refresh,
    Disable,
}

#[cfg(windows)]
fn delete_windows_run_entry(name: &str) -> std::io::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let (run, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    match run.delete_value(name) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub(crate) fn disable_autostart(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        delete_windows_run_entry(&app.package_info().name).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().disable().map_err(|e| e.to_string())
    }
}

/// 冷启动只刷新执行项；Windows 的 StartupApproved 属于系统侧用户选择。
#[cfg(windows)]
fn write_windows_run_entry(name: &str, exe: &std::path::Path) -> std::io::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let (run, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    let command = format!("\"{}\" {AUTOSTART_ARG}", exe.display());
    run.set_value(name, &command)
}

/// 安装版冷启动恢复已保存的自启意图；与用户主动开启自启的插件路径分开。
pub(crate) fn restore_autostart(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        write_windows_run_entry(&app.package_info().name, &exe).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().enable().map_err(|e| e.to_string())
    }
}

/// `args` 不含可执行文件名；数据目录参数的值不作为启动标记处理。
pub(crate) fn is_autostart<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "--data-dir" => {
                let _ = args.next();
            }
            AUTOSTART_ARG => return true,
            _ => {}
        }
    }
    false
}

/// 在 Tauri 创建窗口前配置；便携首启确认页必须可见。
pub(crate) fn configure_main_window<I, S>(
    config: &mut tauri::Config,
    args: I,
    requires_confirmation: bool,
) where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let visible = requires_confirmation || !is_autostart(args);
    if let Some(main) = config.app.windows.iter_mut().find(|w| w.label == "main") {
        main.visible = visible;
        main.focus = visible;
    }
}

/// 第二实例的手动启动唤起已有窗口，系统自启保持已有窗口状态。
pub(crate) fn activate_existing<I, S>(args: I, show: impl FnOnce())
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if !is_autostart(args) {
        show();
    }
}

/// 保存设置及冷启动共用的注册边界；`apply` 写入当前程序路径与启动参数。
pub(crate) fn sync_autostart<E>(
    previous: bool,
    enabled: bool,
    apply: impl FnOnce(AutostartAction) -> Result<(), E>,
) -> Result<(), E> {
    if enabled || previous != enabled {
        apply(if previous && enabled {
            AutostartAction::Refresh
        } else if enabled {
            AutostartAction::Enable
        } else {
            AutostartAction::Disable
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> tauri::Config {
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap()
    }

    #[test]
    fn system_autostart_creates_main_hidden_and_unfocused() {
        let mut config = config();
        configure_main_window(&mut config, [AUTOSTART_ARG], false);
        let main = &config.app.windows[0];
        assert!(!main.visible, "系统自启不能创建可见主窗口");
        assert!(!main.focus, "系统自启不能抢占焦点");
    }

    #[test]
    fn manual_launch_keeps_main_visible() {
        let mut config = config();
        configure_main_window(&mut config, std::iter::empty::<&str>(), false);
        assert!(config.app.windows[0].visible);
        assert!(config.app.windows[0].focus);
    }

    #[test]
    fn portable_confirmation_stays_visible_even_with_autostart_argument() {
        let mut config = config();
        configure_main_window(&mut config, [AUTOSTART_ARG], true);
        assert!(config.app.windows[0].visible);
        assert!(config.app.windows[0].focus);
    }

    #[test]
    fn system_autostart_does_not_activate_existing_instance() {
        let mut activations = 0;
        activate_existing([AUTOSTART_ARG], || activations += 1);
        assert_eq!(activations, 0, "自启第二实例不能唤起主窗口");
    }

    #[test]
    fn manual_second_launch_activates_existing_instance_once() {
        let mut activations = 0;
        activate_existing(std::iter::empty::<&str>(), || activations += 1);
        assert_eq!(activations, 1);
    }

    #[test]
    fn early_manual_request_is_delivered_once_when_setup_becomes_ready() {
        let activation = StartupActivation::default();
        assert!(!activation.request_manual());
        assert!(!activation.request_manual());
        assert!(activation.mark_ready());
        assert!(!activation.mark_ready());
    }

    #[test]
    fn manual_request_after_setup_can_activate_immediately() {
        let activation = StartupActivation::default();
        assert!(!activation.mark_ready());
        assert!(activation.request_manual());
        assert!(!activation.mark_ready());
    }

    #[test]
    fn early_system_autostart_does_not_queue_a_window_activation() {
        let activation = StartupActivation::default();
        activate_existing([AUTOSTART_ARG], || {
            let _ = activation.request_manual();
        });
        assert!(!activation.mark_ready());
    }

    #[test]
    fn autostart_argument_is_exact_and_not_a_data_directory_value() {
        assert!(is_autostart([AUTOSTART_ARG]));
        assert!(is_autostart(["--data-dir", "sandbox", AUTOSTART_ARG]));
        assert!(!is_autostart(["--data-dir", AUTOSTART_ARG]));
        assert!(!is_autostart(["--autostart=false"]));
    }

    #[test]
    fn saved_enabled_intent_repairs_a_missing_system_registration() {
        let mut registered = false;
        sync_autostart(true, true, |action| {
            registered = action != AutostartAction::Disable;
            Ok::<_, String>(())
        })
        .unwrap();
        assert!(registered, "开关仍为开启时也必须补回缺失执行项");
    }

    #[test]
    fn unchanged_enabled_intent_refreshes_the_registration() {
        let mut applications = 0;
        sync_autostart(true, true, |action| {
            assert_ne!(action, AutostartAction::Disable);
            applications += 1;
            Ok::<_, String>(())
        })
        .unwrap();
        assert_eq!(applications, 1, "升级后注册必须携带当前路径与参数");
    }

    #[test]
    fn explicit_disable_removes_registration_but_unchanged_off_does_nothing() {
        let mut registered = true;
        sync_autostart(true, false, |action| {
            registered = action != AutostartAction::Disable;
            Ok::<_, String>(())
        })
        .unwrap();
        assert!(!registered);
        sync_autostart(false, false, |_| -> Result<(), String> {
            panic!("未启用自启时不能注册或删除系统项")
        })
        .unwrap();
    }

    #[test]
    fn registration_failure_remains_retryable_without_toggling_the_switch() {
        let result = sync_autostart(true, true, |_| Err("registration denied"));
        assert_eq!(result, Err("registration denied"));
        let mut registered = false;
        sync_autostart(true, true, |action| {
            registered = action != AutostartAction::Disable;
            Ok::<_, &str>(())
        })
        .unwrap();
        assert!(registered);
    }

    #[test]
    fn unchanged_enabled_intent_refreshes_without_explicitly_enabling_again() {
        sync_autostart(true, true, |action| {
            assert_eq!(action, AutostartAction::Refresh);
            Ok::<_, String>(())
        })
        .unwrap();
        sync_autostart(false, true, |action| {
            assert_eq!(action, AutostartAction::Enable);
            Ok::<_, String>(())
        })
        .unwrap();
    }

    #[cfg(windows)]
    struct RegistrationFixture {
        name: String,
        run: winreg::RegKey,
        approved: winreg::RegKey,
    }

    #[cfg(windows)]
    impl RegistrationFixture {
        fn new() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            use winreg::{RegKey, enums::HKEY_CURRENT_USER};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let name = format!(
                "QuotaTray-startup-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            Self {
                name,
                run: hkcu.create_subkey(RUN_KEY).unwrap().0,
                approved: hkcu
                    .create_subkey(
                        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run",
                    )
                    .unwrap()
                    .0,
            }
        }
    }

    #[cfg(windows)]
    impl Drop for RegistrationFixture {
        fn drop(&mut self) {
            let _ = self.run.delete_value(&self.name);
            let _ = self.approved.delete_value(&self.name);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_missing_registration_is_restored_with_quoted_path_and_autostart_argument() {
        let fixture = RegistrationFixture::new();
        let exe = std::path::Path::new(r"C:\Program Files\额度监视器\quota-desktop.exe");
        write_windows_run_entry(&fixture.name, exe).unwrap();
        let command: String = fixture.run.get_value(&fixture.name).unwrap();
        assert_eq!(
            command,
            r#""C:\Program Files\额度监视器\quota-desktop.exe" --autostart"#
        );
        assert!(fixture.approved.get_raw_value(&fixture.name).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_disable_missing_registration_is_idempotent() {
        let fixture = RegistrationFixture::new();
        delete_windows_run_entry(&fixture.name).unwrap();
        fixture
            .run
            .set_value(&fixture.name, &"old command")
            .unwrap();
        delete_windows_run_entry(&fixture.name).unwrap();
        delete_windows_run_entry(&fixture.name).unwrap();
        assert!(fixture.run.get_value::<String, _>(&fixture.name).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_refreshes_legacy_command_without_changing_system_disable_record() {
        use winreg::{RegValue, enums::RegType::REG_BINARY};
        let fixture = RegistrationFixture::new();
        fixture
            .run
            .set_value(&fixture.name, &r"C:\Old\quota-desktop.exe")
            .unwrap();
        let disabled = RegValue {
            vtype: REG_BINARY,
            bytes: vec![3, 0, 0, 0, 42, 1, 0, 0, 0, 0, 0, 0],
        };
        fixture
            .approved
            .set_raw_value(&fixture.name, &disabled)
            .unwrap();
        write_windows_run_entry(
            &fixture.name,
            std::path::Path::new(r"C:\New\quota-desktop.exe"),
        )
        .unwrap();
        let command: String = fixture.run.get_value(&fixture.name).unwrap();
        assert_eq!(command, r#""C:\New\quota-desktop.exe" --autostart"#);
        assert_eq!(
            fixture.approved.get_raw_value(&fixture.name).unwrap(),
            disabled
        );
    }
}
