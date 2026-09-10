//! 定价目录调度器（T-06）：每分钟 tick——
//! ① 磁盘 revision 重载（接收 CLI 等他进程写入的更高版本；仅本地
//!    读取零网络，GUI 聚焦与常驻节奏统一走此路径）；
//! ② 到期自动检查（6 小时成功间隔 / 30 分钟失败退避，注入时钟的
//!    纯函数 `quota_core::catalog_should_auto_check` 判定；开关关闭
//!    只保留磁盘重载）。
//!
//! 不引入独立后台服务：循环随应用进程生命周期（Android 后台被杀即停，
//! 回前台经 `set_app_foreground` 触发补检）。自动成功只更新状态与视图
//! （事件失效 native-metas + 托盘重建），不弹系统通知、不要求重启。

use tauri::{AppHandle, Emitter, Manager};

use crate::commands;
use crate::state::{AppState, now_ms};

async fn run_polling<F, Fut>(mobile: bool, foreground: impl Fn() -> bool, mut action: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    loop {
        if !mobile || foreground() {
            action().await;
        }
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}

/// 桌面常驻检查；Android 仅在前台时执行本地重载和到期联网。
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(run_polling(
        cfg!(any(target_os = "android", target_os = "ios")),
        || crate::state::APP_FOREGROUND.load(std::sync::atomic::Ordering::Relaxed),
        move || {
            let app = app.clone();
            async move { tick(&app).await }
        },
    ));
}

/// 单次判定与执行（调度循环与回前台补检共用）。
pub async fn tick(app: &AppHandle) {
    if cfg!(any(target_os = "android", target_os = "ios"))
        && !crate::state::APP_FOREGROUND.load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }
    let state = app.state::<AppState>();
    // ① 磁盘重载：他进程写入更高 revision → 广播 + 托盘重建（零网络）
    if state.catalog_reload_if_newer() {
        let _ = app.emit(commands::CATALOG_CHANGED_EVENT, ());
        commands::after_state_change(app, &state);
    }
    // ② 到期自动检查：读磁盘信封元数据（与 CLI 共享同一节流状态）
    let enabled = state.settings.read().unwrap().auto_update_pricing_catalog;
    let (attempt, success, _) = commands::read_catalog_envelope_meta(state.paths.root());
    if quota_core::catalog_should_auto_check(enabled, attempt, success, now_ms()) {
        // Updated 时 run_catalog_update 内部已重载快照、广播并重建托盘；
        // 其余结果静默（自动成功无需系统通知或重启确认，spec §7）
        let _ = commands::run_catalog_update(app, &state).await;
    }
}

/// 回前台补检（Android/桌面聚焦共用）：本地磁盘重载立即执行；网络
/// 到期检查作为后台任务发起（不阻塞前台命令返回）。
pub fn on_foreground(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tick(&app).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    #[tokio::test(start_paused = true)]
    async fn mobile_polls_while_foreground_and_pauses_in_background() {
        let foreground = Arc::new(AtomicBool::new(true));
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let gate = foreground.clone();
        let task = tokio::spawn(run_polling(
            true,
            move || gate.load(Ordering::Relaxed),
            move || {
                observed.fetch_add(1, Ordering::Relaxed);
                std::future::ready(())
            },
        ));
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        tokio::time::advance(std::time::Duration::from_secs(6 * 3600)).await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        foreground.store(false, Ordering::Relaxed);
        tokio::time::advance(std::time::Duration::from_secs(60)).await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        foreground.store(true, Ordering::Relaxed);
        tokio::time::advance(std::time::Duration::from_secs(60)).await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::Relaxed), 3);
        task.abort();
    }
}
