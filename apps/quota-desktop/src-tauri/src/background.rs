//! Android 后台刷新编排核（C 项，WorkManager Worker 经 JNI 调入）。
//!
//! 无 AppHandle/tauri runtime——WorkManager 冷启动拉起已死进程时
//! MainActivity 与 tauri run() 均不执行，本模块必须完全自足：按传入
//! dataDir 现开 vault/engine/settings/history；结果只落 history.db
//! （不碰内存 results 与 cache.json——避免与前台实例双写竞态，卡片
//! 数值回前台由既有轮询/聚焦刷新追上）；低余额边沿判定与前台命令
//! 路径共享 [`crate::state::LOW_BALANCE_NOTIFIED`] 全局静态（否则冷热
//! 两路各自首次达标会双份通知）；通知不直接发送，以 JSON 返回由
//! Kotlin Worker 直发（渠道元数据随返回值携带，Rust 是渠道 id/名称
//! 的单一数据源，Kotlin 幂等建渠道）。
//!
//! 决策与组装纯函数全平台编译（host 单测）；IO 编排与 JNI 导出仅
//! android 编译（模拟器/CI android-preview 验收）。

use serde::Serialize;

/// Worker JNI 返回的 JSON 顶层形状（Kotlin 侧 org.json 同名解析）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BackgroundRefreshResult {
    /// 通知渠道元数据：恒携带（Kotlin 幂等建渠道，与通知开关解耦——
    /// 渠道先建好，开关只拦发送）。
    pub channel: ChannelInfo,
    /// 待直发的系统通知列表（空 = 本轮无通知：未达标/已登记过/通知
    /// 开关关/应用前台——前台时通知留给前端轮询路径产生红点）。
    pub notifications: Vec<NotificationItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NotificationItem {
    pub title: String,
    pub body: String,
}

/// 后台刷新单轮决策（纯函数，host 单测；生产消费方在 android 模块，
/// host 侧仅测试使用故 allow(dead_code)，先例同 MESSAGES_CHANNEL_ID）：
/// - `refresh`：`background_refresh_enabled` 关闭时整体跳过（用户关
///   后台刷新的核心诉求是别偷跑流量，查询本身也不做）；
/// - `notify`：通知路径成立条件 = 系统通知开 && 应用后台（调用侧已把
///   「未校准」坍缩为后台）。Worker 可能在应用前台时被调度（15 分钟
///   周期到了而用户正开着 app）——查询照做写历史（无打扰），通知与
///   低余额边沿判定都留给前端轮询路径（红点归前台，否则 Worker 抢先
///   登记全局会吞掉前台的首次达标提醒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundDecision {
    pub refresh: bool,
    pub notify: bool,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn decide_background_refresh(
    background_refresh_enabled: bool,
    notifications_enabled: bool,
    app_foreground: bool,
) -> BackgroundDecision {
    BackgroundDecision {
        refresh: background_refresh_enabled,
        notify: notifications_enabled && !app_foreground,
    }
}

/// 组装 Worker 返回 JSON 的数据部分（纯函数，host 单测，allow 口径同
/// 上）：`notify` 关闭时丢弃通知项仅保留渠道元数据（渠道恒建，见
/// [`ChannelInfo`]）。
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn build_result(
    channel: ChannelInfo,
    notify: bool,
    notifications: Vec<NotificationItem>,
) -> BackgroundRefreshResult {
    BackgroundRefreshResult {
        channel,
        notifications: if notify { notifications } else { Vec::new() },
    }
}

// ---- Android IO 编排与 JNI 导出（host 不编译，模拟器/CI 验收） ----------

#[cfg(target_os = "android")]
pub(crate) use android::schedule_background_work;

#[cfg(target_os = "android")]
mod android {
    use super::{
        BackgroundRefreshResult, ChannelInfo, NotificationItem, build_result,
        decide_background_refresh,
    };
    use crate::commands::{LowBalanceEdge, low_balance_breach, low_balance_edge};
    use crate::i18n::Lang;
    use crate::settings::Settings;
    use crate::state::{DataPaths, LOW_BALANCE_NOTIFIED};
    use crate::update_ctl::MESSAGES_CHANNEL_ID;
    use quota_core::{AppConfig, ProviderKind, UsageData};
    use std::path::Path;

    /// 单轮后台刷新（Worker 调入）：任何失败仅日志并以「仅渠道元数据」
    /// 收场——Worker 无 UI 可透错，静默口径与桌面调度器一致。
    pub fn background_refresh_once(data_dir: &Path) -> BackgroundRefreshResult {
        let empty = |name: String| BackgroundRefreshResult {
            channel: ChannelInfo {
                id: MESSAGES_CHANNEL_ID.into(),
                name,
            },
            notifications: Vec::new(),
        };
        let Ok(paths) = DataPaths::new(Some(data_dir.to_path_buf())) else {
            return empty(String::new());
        };
        let settings = Settings::load(&paths.settings());
        let lang = Lang::parse(&settings.language);
        let bail = || empty(lang.notification_channel_name());
        // 前台态三态坍缩：未校准（WorkManager 冷启动，前端从未跑过）视为
        // 后台——乐观初值 true 的语义只对「前端会跑起来」成立，冷启动
        // 恰是本功能主场景，不坍缩则通知恒不发
        let calibrated =
            crate::state::APP_FOREGROUND_CALIBRATED.load(std::sync::atomic::Ordering::Relaxed);
        let app_foreground =
            calibrated && crate::state::APP_FOREGROUND.load(std::sync::atomic::Ordering::Relaxed);
        let decision = decide_background_refresh(
            settings.background_refresh_enabled,
            settings.notifications_enabled,
            app_foreground,
        );
        if !decision.refresh {
            return bail();
        }
        // 自足构造查询上下文：vault（Keystore 经 ndk-context，Worker 侧
        // Kotlin 已先补调 initializeNdkContext）、引擎（代理主机/端口随
        // 设置）、历史库（WAL + busy_timeout 兜底与前台并发）
        let Ok(vault) = quota_core::Vault::open(&quota_core::KeyringStore::new()) else {
            eprintln!("后台刷新：保险库打开失败，本轮跳过");
            return bail();
        };
        let Ok(engine) = crate::state::build_engine(
            settings.update_proxy_host.as_deref(),
            settings.update_proxy_port,
        ) else {
            eprintln!("后台刷新：查询引擎构造失败，本轮跳过");
            return bail();
        };
        let Ok(history) = quota_core::HistoryStore::open(&paths.history()) else {
            eprintln!("后台刷新：历史库打开失败，本轮跳过");
            return bail();
        };
        let Ok(cfg) = AppConfig::load(&paths.config()) else {
            eprintln!("后台刷新：配置读取失败，本轮跳过");
            return bail();
        };
        let mut fresh = Vec::new();
        for entry in cfg.providers.iter().filter(|p| p.enabled) {
            // 桌面 CLI 凭据条目在 Android 无凭据来源，跳过（同前台口径）
            if let ProviderKind::Native { provider } = &entry.kind
                && crate::commands::mobile_cli_provider_blocked("android", provider)
            {
                continue;
            }
            match tauri::async_runtime::block_on(engine.query(&vault, entry)) {
                Ok(data) => {
                    let at = crate::state::now_ms();
                    if let Err(e) = history.record(&entry.id, &data, at) {
                        eprintln!("后台刷新：历史写入失败（{}）：{e}", entry.id);
                    }
                    // 边沿判定仅在通知路径成立时做：前台时跳过——否则
                    // Worker 抢先登记全局会吞掉前台命令路径的首次达标
                    // （refetch_and_store 变 Silent，红点与通知都不产生）
                    if decision.notify {
                        collect_low_balance(
                            &lang,
                            &entry.id,
                            &entry.name,
                            &data,
                            settings.low_balance_threshold_percent,
                            &mut fresh,
                        );
                    }
                }
                Err(e) => eprintln!("后台刷新：查询失败（{}）：{e}", entry.id),
            }
        }
        build_result(
            ChannelInfo {
                id: MESSAGES_CHANNEL_ID.into(),
                name: lang.notification_channel_name(),
            },
            decision.notify,
            fresh,
        )
    }

    /// 低余额边沿判定 + 通知文案组装（复用前台路径的判定三件套与
    /// 全局登记；锁内只碰集合，文案组装在锁外）。
    fn collect_low_balance(
        lang: &Lang,
        id: &str,
        name: &str,
        data: &[UsageData],
        threshold: u8,
        out: &mut Vec<NotificationItem>,
    ) {
        let breach = low_balance_breach(data, threshold);
        let is_notify = {
            let mut notified = LOW_BALANCE_NOTIFIED.lock().unwrap();
            match low_balance_edge(notified.contains(id), breach) {
                LowBalanceEdge::Notify => {
                    notified.insert(id.to_string());
                    true
                }
                LowBalanceEdge::Reset => {
                    notified.remove(id);
                    false
                }
                LowBalanceEdge::Silent => false,
            }
        };
        if is_notify && let Some(percent) = breach {
            out.push(NotificationItem {
                title: lang.low_balance_notify_title(),
                body: lang.low_balance_notify_body(name, percent.round() as u32),
            });
        }
    }

    /// Worker 兜底返回（Rust 侧 panic/参数异常时不带语言上下文的中性名；
    /// Kotlin 建渠道遇空名同以应用名兜底）。
    fn fallback_result() -> BackgroundRefreshResult {
        BackgroundRefreshResult {
            channel: ChannelInfo {
                id: MESSAGES_CHANNEL_ID.into(),
                name: "QuotaTray".into(),
            },
            notifications: Vec::new(),
        }
    }

    fn fallback_json() -> String {
        serde_json::to_string(&fallback_result()).expect("兜底 JSON 可序列化")
    }

    /// 调度接线：把 settings 的后台刷新开关/周期同步到 WorkManager
    /// （经 JNI 调 `BackgroundScheduler.schedule(Context, boolean, long)`，
    /// UPDATE 策略——间隔变更即时生效且保留周期对齐；开关关则
    /// cancelUniqueWork）。设置保存路径（persist_settings）与应用启动
    /// （setup Android 段）各调一次；失败仅日志——调度缺失仅表现为
    /// 后台不刷新，设置页开关状态不受影响。
    pub(crate) fn schedule_background_work(state: &crate::state::AppState) {
        let (enabled, interval) = {
            let s = state.settings.read().unwrap();
            (
                s.background_refresh_enabled,
                i64::from(s.background_refresh_interval_minutes),
            )
        };
        let scheduled = crate::apk_install::with_helper_class_named(
            "com.quotatray.android.BackgroundScheduler",
            |env, context, helper| {
                let ret = env
                    .call_static_method(
                        helper,
                        "schedule",
                        "(Landroid/content/Context;ZJ)Z",
                        &[
                            jni::objects::JValue::Object(context),
                            jni::objects::JValue::Bool(u8::from(enabled)),
                            jni::objects::JValue::Long(interval),
                        ],
                    )
                    .map_err(|e| format!("调用 BackgroundScheduler.schedule 失败：{e}"))?;
                ret.z().map_err(|e| format!("读取调度结果失败：{e}"))
            },
        );
        if let Err(e) = scheduled {
            eprintln!("后台刷新调度同步失败：{e}");
        }
    }

    /// JNI 导出：`Native.backgroundRefresh(dataDir: String): String`
    /// （独立 object 的实例方法——companion 的 external fun 符号名含
    /// `$` 转义陷阱，独立 object 无此问题；类名/包名改动会
    /// UnsatisfiedLinkError，keep 规则锁定）。receiver 是 object 单例
    /// 实例而非类对象。
    ///
    /// **env 必须按值接收**（`mut env: JNIEnv`，jni 0.21 起 JNIEnv 为
    /// Copy，官方 ABI 模式）：`&mut JNIEnv` 会多一层指针间接，JVM 传入
    /// 的 `JNIEnv*` 被错解为 `JNIEnv**`，解引用读到 null——模拟器实证
    /// （2026-08-30）表现为所有 JNI 调用报 NullDeref("*JNIEnv")。
    ///
    /// panic 跨 FFI 是 UB，兜底捕获并返回空通知 JSON；Java→Rust 入口
    /// 方向的 pending exception（get_string/new_string 失败均可能挂起）
    /// 统一清理——带着未清异常返回/后续 JNI 调用违反 JNI 规范，CheckJNI
    /// 下可 abort（与 apk_install 的 Rust→Java 方向收口对称）。new_string
    /// 兜底再失败（OOM 级）返回 null，Kotlin 侧 doWork 的判空兜底。
    /// edition 2024 中 `no_mangle` 属 unsafe 属性，显式标注。
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_quotatray_android_Native_backgroundRefresh(
        mut env: jni::JNIEnv<'_>,
        _receiver: jni::objects::JObject<'_>,
        data_dir: jni::objects::JString<'_>,
    ) -> jni::sys::jstring {
        use std::panic::AssertUnwindSafe;
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let dir = env
                .get_string(&data_dir)
                .map_err(|e| e.to_string())
                .map(String::from)
                .map(|dir| background_refresh_once(Path::new(&dir)));
            if dir.is_err() {
                // get_string 失败会挂 pending exception，进 JNI 必清
                let _ = env.exception_clear();
            }
            match dir {
                Ok(result) => serde_json::to_string(&result).unwrap_or_else(|_| fallback_json()),
                Err(e) => {
                    eprintln!("后台刷新：JNI 参数读取失败：{e}");
                    fallback_json()
                }
            }
        }))
        .unwrap_or_else(|_| {
            eprintln!("后台刷新：Rust 侧 panic 已捕获");
            fallback_json()
        });
        match env.new_string(outcome) {
            Ok(s) => s.into_raw(),
            Err(e) => {
                eprintln!("后台刷新：返回 JSON 构造失败：{e}");
                let _ = env.exception_clear();
                env.new_string(fallback_json())
                    .map(|s| s.into_raw())
                    .unwrap_or(std::ptr::null_mut())
            }
        }
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;

    /// 契约：决策矩阵——开关关整体不刷新（省流量是关它的核心诉求）；
    /// 刷新开时通知仅在「系统通知开 && 后台」时随刷新携带。
    #[test]
    fn decide_background_refresh_matrix() {
        // 开关关：无论通知/前后台，整体跳过
        for notify in [true, false] {
            for fg in [true, false] {
                let d = decide_background_refresh(false, notify, fg);
                assert!(!d.refresh, "开关关不刷新（notify={notify} fg={fg}）");
            }
        }
        assert_eq!(
            decide_background_refresh(true, true, false),
            BackgroundDecision {
                refresh: true,
                notify: true
            },
            "后台 + 通知开 → 发通知"
        );
        assert!(
            !decide_background_refresh(true, true, true).notify,
            "前台不发（留前端路径产生红点）"
        );
        assert!(
            !decide_background_refresh(true, false, false).notify,
            "通知开关关不发"
        );
        assert!(
            decide_background_refresh(true, false, true).refresh,
            "刷新本身只看后台刷新开关"
        );
    }

    /// 契约：结果组装——notify=false 丢弃通知项但渠道元数据保留
    /// （Kotlin 幂等建渠道与开关解耦）；notify=true 原样携带。
    #[test]
    fn build_result_gates_notifications_but_keeps_channel() {
        let channel = ChannelInfo {
            id: "quotatray-messages".into(),
            name: "测试渠道".into(),
        };
        let items = vec![NotificationItem {
            title: "余额提醒".into(),
            body: "X 已用 90%".into(),
        }];
        let gated = build_result(channel.clone(), false, items.clone());
        assert_eq!(gated.channel, channel, "渠道元数据恒携带");
        assert!(gated.notifications.is_empty(), "notify=false 丢弃通知项");
        let sent = build_result(channel.clone(), true, items.clone());
        assert_eq!(sent.notifications, items, "notify=true 原样携带");
    }
}
