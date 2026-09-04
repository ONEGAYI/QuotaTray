//! 结构化事件打点（log facade 薄层）。
//!
//! core 不装配 logger（库 crate 无输出通道决策权），仅定义事件
//! message 的载荷约定：`"<事件名> <fields JSON 对象>"`——desktop/CLI
//! 侧的 flexi_logger 格式函数按首个空格切分，JSON 部分解析成功则嵌入
//! `fields` 对象，失败则整条按纯文本 `message` 降级（迁移自 eprintln
//! 的非结构化告警走降级路径）。
//!
//! 字段纪律（安全红线第 1 条）：事件字段为白名单制——条目 id/名称、
//! 查询通道、错误分类与已脱敏错误文案、耗时、代理 host:port、版本与
//! 运行形态；任何字段不得包含凭据明文或密钥材料。

/// 事件打点的统一 target（desktop/CLI 格式函数按此识别事件行）。
pub const EVENT_TARGET: &str = "qt";

/// 构造事件 message 载荷：`事件名 + 空格 + fields JSON`。
///
/// fields 必须是 JSON 对象（`json!({...})`）；序列化开销为每事件一次
/// 微秒级 `format!` + `to_string`，查询链路每分钟数条事件，无性能负担。
pub fn event_payload(event: &str, fields: serde_json::Value) -> String {
    format!("{event} {fields}")
}

/// 结构化事件打点宏。
///
/// ```ignore
/// qt_event!(info, "query_done", { "entry": id, "ok": true, "elapsed_ms": 812 });
/// ```
///
/// 展开为 `log::info!(target: "qt", "<事件名> <fields JSON>")`；
/// 进程未装配 logger 时为 no-op（log facade 特性），core 的单测因此
/// 不受打点影响。
#[macro_export]
macro_rules! qt_event {
    ($level:ident, $event:expr, { $($fields:tt)* }) => {
        ::log::$level!(
            target: $crate::logging::EVENT_TARGET,
            "{}",
            $crate::logging::event_payload($event, ::serde_json::json!({ $($fields)* }))
        )
    };
}

// ─────────────────────────────────────────────────────────────────────────
// 装配端实现（`flexi-logging` feature）：desktop/CLI 共享的 flexi_logger
// 装配（JSONL 滚动文件 + 7 天保留）。core 默认构建不编译本段，保持库
// 纯净；feature 仅由两端 bin 的依赖声明开启（feature 可加性使然，
// `cargo test --workspace` 时本段也会编译并运行测试）。
// ─────────────────────────────────────────────────────────────────────────
#[cfg(feature = "flexi-logging")]
pub mod rolling {
    //! JSONL 滚动日志装配：按天 + 单文件 5MB 双条件滚动，保留 7 天。
    //!
    //! 输出行为扁平 JSON（`ts` / `level` / `target` / 事件行为 `event` +
    //! `fields`、纯文本行为 `message`），每行一个 JSON 对象（JSONL），
    //! 可直接被 jq / 未来前端日志页消费。
    //!
    //! 初始化语义：日志是非关键基础设施——失败仅回传 `Err` 由调用方
    //! 告警并继续启动（对齐 settings.json 的非关键数据哲学），绝不
    //! 阻断应用；进程内幂等，二次调用为 no-op。

    use std::io;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    use flexi_logger::Age;
    use flexi_logger::Cleanup;
    use flexi_logger::Criterion;
    use flexi_logger::DeferredNow;
    use flexi_logger::Duplicate;
    use flexi_logger::FileSpec;
    use flexi_logger::Logger;
    use flexi_logger::Naming;
    use flexi_logger::Record;
    /// 本地时间 + UTC 偏移（`%Y-%m-%d %H:%M:%S%.6f %:z`），flexi_logger
    /// 唯一预定义格式常量，微秒精度对诊断时序足够。
    use flexi_logger::TS_DASHES_BLANK_COLONS_DOT_BLANK as TS_LOCAL;
    use flexi_logger::WriteMode;

    /// GUI 基名。与 CLI 基名必须互不为前缀——清理按基名/时间戳 infix
    /// 模式匹配自身轮转文件，前缀包含会让另一端的清理误删本端日志。
    /// 实际文件名形如 `quotatray_r2026-09-04_15-23-55.jsonl`
    /// （`TimestampsDirect` 的 `_r` 前缀 + 秒级时间戳 infix）。
    pub const LOG_BASENAME_DESKTOP: &str = "quotatray";
    /// CLI（watch 模式）基名，与 CLI bin 名一致。
    pub const LOG_BASENAME_CLI: &str = "quota-cli";
    /// 保留时间窗口（天）：`Cleanup::KeepForDays` 按文件 **mtime** 判定
    /// 删除（flexi_logger 实测行为，不解析文件名时间戳——语义略宽松：
    /// 近 7 天内最后写入的文件都保留）。
    pub const LOG_RETENTION_DAYS: usize = 7;
    /// 单文件大小上限（字节）：错误风暴防单日文件暴涨，超出即滚动。
    pub const LOG_MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
    /// 默认日志规格；`RUST_LOG` 环境变量可临时调深（如 `debug`）。
    pub const LOG_DEFAULT_SPEC: &str = "info";

    /// 进程级幂等守卫：Android 前台 setup 与 WorkManager 冷启动
    /// background_refresh_once 两条路径可能先后（甚至同进程）初始化。
    /// pub(crate)：幂等契约测试直接置位模拟二次调用。
    pub(crate) static INITED: AtomicBool = AtomicBool::new(false);

    /// JSONL 行格式函数（flexi_logger FormatFunction 签名）。
    ///
    /// 消费 [`crate::logging`] 的载荷约定：`qt` target 的事件行拆出
    /// `event` + `fields`；其余（迁移自 eprintln 的纯文本告警）整体
    /// 作为 `message` 字段。拆分失败一律降级为 `message`，永不 panic。
    pub fn jsonl_format(
        w: &mut dyn io::Write,
        now: &mut DeferredNow,
        record: &Record<'_>,
    ) -> io::Result<()> {
        let mut line = serde_json::Map::new();
        line.insert(
            "ts".into(),
            serde_json::Value::String(now.format(TS_LOCAL).to_string()),
        );
        line.insert(
            "level".into(),
            serde_json::Value::String(record.level().as_str().to_string()),
        );
        line.insert(
            "target".into(),
            serde_json::Value::String(record.target().to_string()),
        );
        let message = record.args().to_string();
        match parse_event_message(&message) {
            Some((event, fields)) => {
                line.insert("event".into(), serde_json::Value::String(event));
                line.insert("fields".into(), fields);
            }
            None => {
                line.insert("message".into(), serde_json::Value::String(message));
            }
        }
        // flexi_logger 在格式输出后自动补行终止符，此处不可再 writeln
        // （双换行会在 JSONL 中产生空行，破坏逐行解析）
        write!(w, "{}", serde_json::Value::Object(line))
    }

    /// 按 `事件名 + 空格 + JSON 对象` 约定拆解事件行；
    /// 非 `qt` target、无空格、JSON 非法或非对象 → None（降级）。
    fn parse_event_message(message: &str) -> Option<(String, serde_json::Value)> {
        let (event, rest) = message.split_once(' ')?;
        let fields = serde_json::from_str::<serde_json::Value>(rest).ok()?;
        if fields.is_object() {
            Some((event.to_string(), fields))
        } else {
            None
        }
    }

    /// 进程级幂等初始化滚动 JSONL 日志。
    ///
    /// 失败语义：返回 `Err` 并复位守卫（下次调用可重试），调用方
    /// 告警后继续运行——诊断设施的缺失不该挡业务。debug 构建额外
    /// duplicate 到 stderr，便于 `pnpm desktop:dev` 即时观察。
    pub fn init_logging(dir: &Path, basename: &str) -> Result<(), String> {
        // CAS 抢占：并发双路径（Android 前台/冷启动）仅一方真正装配
        if INITED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(());
        }
        let result = (|| {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("日志目录创建失败（{}）：{e}", dir.display()))?;
            let mut logger = Logger::try_with_env_or_str(LOG_DEFAULT_SPEC)
                .map_err(|e| format!("日志规格解析失败：{e}"))?
                .log_to_file(
                    FileSpec::default()
                        .directory(dir)
                        .basename(basename)
                        .suffix("jsonl"),
                )
                .rotate(
                    Criterion::AgeOrSize(Age::Day, LOG_MAX_FILE_BYTES),
                    Naming::TimestampsDirect,
                    Cleanup::KeepForDays(LOG_RETENTION_DAYS),
                )
                .format(jsonl_format)
                .write_mode(WriteMode::Direct);
            #[cfg(debug_assertions)]
            {
                logger = logger.duplicate_to_stderr(Duplicate::All);
            }
            logger.start().map_err(|e| format!("日志初始化失败：{e}"))
        })();
        if let Err(e) = result {
            INITED.store(false, Ordering::SeqCst); // 失败复位，允许下次重试
            return Err(e);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：载荷 = 事件名 + 空格 + fields JSON 对象（紧凑序列化）。
    #[test]
    fn event_payload_is_event_name_then_json_object() {
        let payload = event_payload("query_done", serde_json::json!({ "ok": true }));
        assert_eq!(payload, "query_done {\"ok\":true}");
    }

    /// 契约：载荷可被消费端按约定逆向解析——首个空格切分事件名，
    /// 其余部分解析回 JSON 对象。desktop/CLI 的 jsonl 格式函数依赖
    /// 此约定，任何一侧变更都必须同步（本测试是双侧共同的锚点）。
    #[test]
    fn event_payload_round_trips_via_split_once() {
        let payload = event_payload(
            "query_done",
            serde_json::json!({ "entry": "AB2C3D", "ok": false, "elapsed_ms": 15_000 }),
        );
        let (event, rest) = payload.split_once(' ').expect("载荷必含空格分隔符");
        assert_eq!(event, "query_done");
        let fields: serde_json::Value =
            serde_json::from_str(rest).expect("其余部分必须是合法 JSON");
        assert_eq!(fields["entry"], "AB2C3D");
        assert_eq!(fields["ok"], false);
        assert_eq!(fields["elapsed_ms"], 15_000);
    }

    /// 契约：qt_event! 宏发出的记录 target 为 `qt`，message 符合载荷
    /// 约定。捕获 logger 经全局 set 安装：core 仅有本处安装点，并行
    /// 测试下断言采用存在性判定（而非精确唯一），对安装顺序不敏感。
    #[test]
    fn qt_event_emits_to_event_target() {
        use std::sync::Mutex;

        static CAPTURED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
        struct Capture;
        impl log::Log for Capture {
            fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
                true
            }
            fn log(&self, record: &log::Record<'_>) {
                if record.target() == EVENT_TARGET {
                    CAPTURED
                        .lock()
                        .unwrap()
                        .push((record.target().to_string(), record.args().to_string()));
                }
            }
            fn flush(&self) {}
        }
        // set 失败 = 已被本测试的另一并行实例安装，捕获器同款，无碍
        let _ = log::set_boxed_logger(Box::new(Capture));
        log::set_max_level(log::LevelFilter::Info);

        crate::qt_event!(info, "macro_probe", { "k": "v" });

        let captured = CAPTURED.lock().unwrap();
        let hit = captured.iter().any(|(target, message)| {
            target == EVENT_TARGET && message.starts_with("macro_probe {\"k\":\"v\"}")
        });
        assert!(hit, "应捕获到 qt target 且载荷符合约定：{captured:?}");
    }
}

#[cfg(all(test, feature = "flexi-logging"))]
mod rolling_tests {
    use super::rolling::*;
    use crate::EVENT_TARGET;
    use flexi_logger::DeferredNow;
    use log::Level;
    use log::Record;

    fn format_record(target: &str, message: &str) -> String {
        let mut buf: Vec<u8> = Vec::new();
        let mut now = DeferredNow::new();
        // Record 内联进调用表达式：format_args! 的临时值必须活到调用结束
        jsonl_format(
            &mut buf,
            &mut now,
            &Record::builder()
                .args(format_args!("{message}"))
                .level(Level::Info)
                .target(target)
                .build(),
        )
        .unwrap();
        String::from_utf8(buf).expect("JSONL 行必须是合法 UTF-8")
    }

    /// 契约：事件行（qt target + 载荷约定）拆出 event 与 fields，
    /// 整行是可被 jq/前端工具消费的合法 JSON 对象（行终止符由
    /// flexi_logger 装配层追加，格式函数本身不写换行）。
    #[test]
    fn event_line_produces_jsonl_with_event_and_fields() {
        let line = format_record(EVENT_TARGET, "query_done {\"ok\":true,\"elapsed_ms\":812}");
        assert!(!line.contains('\n'), "格式函数不写行终止符（装配层负责）");
        let parsed: serde_json::Value =
            serde_json::from_str(line.trim()).expect("整行必须是合法 JSON");
        assert_eq!(parsed["target"], EVENT_TARGET);
        assert_eq!(parsed["event"], "query_done");
        assert_eq!(parsed["fields"]["ok"], true);
        assert_eq!(parsed["fields"]["elapsed_ms"], 812);
        assert!(parsed.get("message").is_none(), "事件行不带 message 字段");
    }

    /// 契约：非 qt target 的纯文本行（迁移自 eprintln 的告警）整体作为
    /// message 字段，含中文与空格的文案不丢字。
    #[test]
    fn plain_text_line_degrades_to_message_field() {
        let line = format_record(
            "quota_desktop_lib::settings",
            "settings.json 解析失败，回退默认设置",
        );
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["message"], "settings.json 解析失败，回退默认设置");
        assert!(parsed.get("event").is_none(), "纯文本行不拆事件");
    }

    /// 契约：qt target 但载荷不符合约定（无空格 / JSON 非法 / JSON 非
    /// 对象）→ 一律降级 message，永不 panic、永不丢行。
    #[test]
    fn malformed_event_payload_degrades_without_panic() {
        for bad in ["无空格纯文本", "查询超时 不是JSON", "事件 [1,2] 数组非对象"]
        {
            let line = format_record(EVENT_TARGET, bad);
            let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(parsed["message"], bad, "降级必须保留完整原文：{bad}");
        }
    }

    /// 契约：GUI 与 CLI 的日志基名互不为前缀——清理按基名/时间戳
    /// infix 模式匹配自身轮转文件，前缀包含会让另一端清理误删本端
    /// 日志（真实文件名形如 `quotatray_r2026-09-04_15-23-55.jsonl` /
    /// `quota-cli_r2026-09-04_15-23-55.jsonl`，`quota-` 与 `quotat`
    /// 第 6 字符即分叉）。
    #[test]
    fn desktop_and_cli_basenames_do_not_prefix_each_other() {
        assert!(!LOG_BASENAME_DESKTOP.starts_with(LOG_BASENAME_CLI));
        assert!(!LOG_BASENAME_CLI.starts_with(LOG_BASENAME_DESKTOP));
    }

    /// 契约：守卫已置位时二次调用为 no-op——直接返回 Ok，不触磁盘、
    /// 不重复装配全局 logger（Android 前台与冷启动双路径并存的场景）。
    /// 真实装配的端到端行为由 desktop/CLI 的 dev 冒烟验证，本测试不
    /// 真启动 logger，避免污染同进程其他测试的全局捕获器。
    #[test]
    fn init_logging_is_noop_once_guard_is_set() {
        INITED.store(true, std::sync::atomic::Ordering::SeqCst);
        // 传入不存在的目录也不得报错：守卫在目录操作之前拦截
        assert!(
            init_logging(
                std::path::Path::new("Z:/definitely/not/used"),
                LOG_BASENAME_DESKTOP
            )
            .is_ok(),
            "守卫置位后应静默返回 Ok"
        );
        // 复位守卫：残留 true 会让同进程后续真实 init 的测试莫名 no-op
        INITED.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}
