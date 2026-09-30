//! 单实例插件初始化早于应用 setup；同一状态托管入口必须在此之前生效。
#![cfg(not(any(target_os = "android", target_os = "ios")))]

#[path = "../src/startup_builder.rs"]
mod startup_builder;

use tauri::Manager;
use tauri::test::{mock_builder, mock_context, noop_assets};

struct InteractionState;

#[test]
fn interaction_state_is_available_to_the_first_plugin() {
    startup_builder::prepare(mock_builder(), InteractionState)
        .plugin(
            tauri::plugin::Builder::<_, ()>::new("early-state-contract")
                .setup(|app, _| {
                    assert!(
                        app.try_state::<InteractionState>().is_some(),
                        "单实例插件初始化阶段必须已托管交互状态"
                    );
                    Ok(())
                })
                .build(),
        )
        .build(mock_context(noop_assets()))
        .unwrap();
}
