//! 从真实 CLI 入口验证只读定价命令不触发任何隐式联网。
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

#[test]
fn pricing_show_and_model_list_expose_official_sources() {
    let root = std::env::temp_dir().join(format!("qt-cli-sources-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("settings.json"),
        r#"{"update_check_enabled":false}"#,
    )
    .unwrap();
    let mut config = quota_core::AppConfig::default();
    config.providers.push(serde_json::from_value(serde_json::json!({
        "id":"p1", "name":"account", "kind":{"type":"native","provider":"deepseek"}, "enabled":true
    })).unwrap());
    config.save(&root.join("config.json")).unwrap();
    for args in [
        vec!["pricing", "show", "p1", "--json"],
        vec!["pricing", "model", "list", "deepseek", "--json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_quota"))
            .arg("--config")
            .arg(root.join("config.json"))
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success());
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let value = if args[1] == "show" {
            &json
        } else {
            &json["models"][0]
        };
        assert_eq!(
            value["source_urls"][0],
            "https://api-docs.deepseek.com/zh-cn/quick_start/pricing"
        );
        assert_eq!(value["verified_at"], "2026-09-09");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn readonly_catalog_commands_do_not_run_global_update_hook() {
    let root = std::env::temp_dir().join(format!("qt-cli-readonly-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicUsize::new(0));
    let stopped = stop.clone();
    let observed = count.clone();
    let server = std::thread::spawn(move || {
        while !stopped.load(Ordering::Relaxed) {
            if let Ok((mut socket, _)) = listener.accept() {
                observed.fetch_add(1, Ordering::Relaxed);
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut buffer = [0; 4096];
                let _ = socket.read(&mut buffer);
                let _ = socket.write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    });
    let mut settings_changed = false;
    for args in [
        vec!["pricing", "catalog", "status", "--json"],
        vec!["pricing", "model", "list", "deepseek", "--json"],
    ] {
        std::fs::write(
            root.join("settings.json"),
            r#"{"update_check_enabled":true,"auto_update_pricing_catalog":false}"#,
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_quota"));
        command
            .arg("--config")
            .arg(root.join("config.json"))
            .args(args);
        for key in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ] {
            command.env(key, &proxy);
        }
        let output = command
            .env("NO_PROXY", "")
            .env("no_proxy", "")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
        let settings: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("settings.json")).unwrap()).unwrap();
        settings_changed |= settings.get("update_last_check").is_some();
    }
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert_eq!(
        count.load(Ordering::Relaxed),
        0,
        "只读命令不应触发应用升级检测"
    );
    assert!(!settings_changed, "只读命令不写更新检测时间");
}
