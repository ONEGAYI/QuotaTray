//! 在插件初始化前注册启动期间可访问的交互状态。

pub(crate) fn prepare<R: tauri::Runtime, S: Send + Sync + 'static>(
    builder: tauri::Builder<R>,
    state: S,
) -> tauri::Builder<R> {
    builder.manage(state)
}
