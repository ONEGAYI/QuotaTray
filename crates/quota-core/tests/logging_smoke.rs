//! 滚动日志装配的端到端冒烟（独立进程，不污染 lib 单测的全局状态）。
//!
//! 验证链：init_logging 真实启动 → flexi_logger 落盘 → 文件名符合
//! `基名.日期.jsonl` 约定 → 事件行/纯文本行均可被标准 JSON 工具解析。
//! 仅在 `flexi-logging` feature 下编译（`cargo test --workspace` 时由
//! 依赖方开启；裸 `cargo test -p quota-core` 跳过）。

#![cfg(feature = "flexi-logging")]

use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "qt-log-smoke-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 冒烟契约：真实 init 后，事件与纯文本告警都落到 `基名.日期.jsonl`
/// 文件且每行是合法 JSON（事件行带 event/fields，纯文本行带 message）。
#[test]
fn init_logging_writes_parseable_jsonl_file() {
    let dir = temp_dir("e2e");
    quota_core::logging::rolling::init_logging(
        &dir,
        quota_core::logging::rolling::LOG_BASENAME_CLI,
    )
    .expect("真实初始化应成功");

    quota_core::qt_event!(info, "smoke_event", { "k": "v", "n": 7 });
    log::warn!(target: "quota_test", "纯文本冒烟告警");

    // flexi_logger Direct 模式同步写，落盘即时可见
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 1, "当天只有一个滚动文件：{files:?}");
    let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
    // TimestampsDirect 实际格式：`基名_rYYYY-MM-DD_HH-MM-SS.jsonl`——
    // 按天轮转每天一个文件；同日大小触发轮转时时间戳天然区分，且
    // 文件名时间戳正是 Cleanup::KeepForDays 的判断依据
    assert!(
        name.starts_with("quota-cli_r") && name.ends_with(".jsonl"),
        "文件名符合 基名_r时间戳.jsonl 约定：{name}"
    );

    let content = std::fs::read_to_string(&files[0]).unwrap();
    let lines: Vec<serde_json::Value> = content
        .lines()
        .map(|line| serde_json::from_str(line).expect("每行必须是合法 JSON"))
        .collect();
    let event_line = lines
        .iter()
        .find(|v| v.get("event").is_some_and(|e| e == "smoke_event"))
        .expect("应有 smoke_event 事件行");
    assert_eq!(event_line["fields"]["k"], "v");
    assert_eq!(event_line["fields"]["n"], 7);
    assert!(event_line["target"] == "qt");
    assert!(
        lines
            .iter()
            .any(|v| v.get("message").is_some_and(|m| m == "纯文本冒烟告警")),
        "纯文本告警以 message 字段落盘"
    );

    // 二次 init：进程级幂等（返回 Ok 且不产生第二个写入通道）
    assert!(quota_core::logging::rolling::init_logging(&dir, "other").is_ok());

    std::fs::remove_dir_all(&dir).ok();
}
