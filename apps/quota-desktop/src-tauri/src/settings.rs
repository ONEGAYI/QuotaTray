//! 桌面端自有设置：`~/.quotatray/settings.json` 读写。
//!
//! 非关键数据——文件缺失或损坏时回退默认值（不阻断启动），
//! 与 core 的 `config.json`（损坏即报 Parse 错误）策略不同。

use std::path::Path;

use serde::{Deserialize, Serialize};

/// 应用设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// 自动刷新间隔（分钟）。
    #[serde(default = "default_interval")]
    pub refresh_interval_minutes: u32,
    /// 低额度提醒阈值（已用百分比，≥ 该值触发提醒）。
    #[serde(default = "default_threshold")]
    pub low_balance_threshold_percent: u8,
    /// 开机自启（实际状态由 autostart 插件落系统，此处存用户意图）。
    #[serde(default)]
    pub autostart: bool,
    /// 界面语言：`zh` / `en` / `system`（跟随系统 locale）。
    #[serde(default = "default_language")]
    pub language: String,
    /// 界面主题：`light` / `dark` / `system`（跟随系统偏好）。
    #[serde(default = "default_theme")]
    pub theme: String,
    /// 托盘圆环图标的「每圈单位」（余额型一整圈代表的数值）。
    #[serde(default = "default_ring_units")]
    pub ring_units_per_circle: f64,
    /// 托盘图标显示的条目 id（None = 第一个 enabled 条目；失效 id 回退同左）。
    #[serde(default)]
    pub tray_icon_entry_id: Option<String>,
    /// 自动检查更新（应用运行期间常驻轮询；CLI 启动钩子共用该开关）。
    #[serde(default = "default_update_enabled")]
    pub update_check_enabled: bool,
    /// 上次自动检测时间（epoch 毫秒；CLI 与 GUI 共写做节流——GUI 轮询
    /// 5 分钟、CLI 启动 24h）。
    #[serde(default)]
    pub update_last_check: Option<u64>,
    /// 更新通道代理端口（本机 HTTP 代理，如 Clash；None = 直连/环境变量）。
    /// 检测与下载安装包共用；CLI 读同一 settings.json 自动生效。
    #[serde(default)]
    pub update_proxy_port: Option<u16>,
    /// 更新通道代理主机（IP 或域名；None/空白 = 127.0.0.1 本机代理，
    /// 桌面既有语义不变）。Android 上 127.0.0.1 指向手机自身——要经
    /// 电脑代理时在此填其局域网 IP，且代理软件需允许局域网连接。
    #[serde(default)]
    pub update_proxy_host: Option<String>,
    /// 检测到新版本后自动下载安装包（默认关：代理用户流量自主权优先；
    /// 下载完成后经消息中心/系统通知询问安装）。仅安装版生效——
    /// 便携/普通 zip 更新维持「打开目录手动覆盖」引导。
    #[serde(default)]
    pub update_auto_download: bool,
    /// 系统通知总开关（两端共用；默认开——开关只拦截「显式关闭」，桌面
    /// 默认行为不变）。消费方：桌面 notify_desktop（更新就绪 + 低余额，
    /// 主窗不可见时）、Android notify_background（后台补发）。Android 侧
    /// 叠加系统运行时权限层：开关开但 POST_NOTIFICATIONS 未授权时通知被
    /// 系统静默丢弃（等效关闭）。
    #[serde(default = "default_notifications_enabled")]
    pub notifications_enabled: bool,
    /// Android 后台刷新开关（WorkManager 周期查询；默认关——Preview
    /// 谨慎口径，后台流量用户显式开启）。桌面不消费（已有常驻调度）。
    #[serde(default)]
    pub background_refresh_enabled: bool,
    /// Android 后台刷新周期（分钟；默认 30）。系统硬限最小 15 分钟
    /// （`PeriodicWorkRequest.MIN_PERIODIC_INTERVAL_MILLIS`），sanitize
    /// 收口到 15..=360；实际执行受 Doze/省电影响可能延后。
    #[serde(default = "default_background_interval")]
    pub background_refresh_interval_minutes: u32,
    /// 使用统计比较组合；None = 尚未初始化，Some([]) = 用户主动清空。
    #[serde(default)]
    pub usage_comparison_series: Option<Vec<quota_core::UsageComparisonSeries>>,
    /// 使用统计定位线时刻（epoch 毫秒，最多 2 条，按写入顺序——两条拖动
    /// 交叉后不保证时间有序）；两条线用于测量时间差。仅存本机，不进配置
    /// 迁移包。
    #[serde(default)]
    pub usage_marker_lines: Option<Vec<u64>>,
}

/// 定位线上限：与前端 `usageChartView.ts` 的 `USAGE_MARKER_LIMIT` 同值，
/// 两端同步修改。
const MAX_USAGE_MARKER_LINES: usize = 2;

/// 定位线时刻上界：JS `Date` 安全域为 ±8.64e15 毫秒，超出即 Invalid Date，
/// 读数行的 `toISOString()` 会抛错——手改文件塞超大值必须在此拦下。
const MAX_USAGE_MARKER_TS: u64 = 8_640_000_000_000_000;

fn default_interval() -> u32 {
    5
}

fn default_threshold() -> u8 {
    80
}

fn default_language() -> String {
    "system".into()
}

fn default_theme() -> String {
    "system".into()
}

fn default_ring_units() -> f64 {
    100.0
}

fn default_update_enabled() -> bool {
    true
}

fn default_notifications_enabled() -> bool {
    true
}

fn default_background_interval() -> u32 {
    30
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_interval_minutes: default_interval(),
            low_balance_threshold_percent: default_threshold(),
            autostart: false,
            language: default_language(),
            theme: default_theme(),
            ring_units_per_circle: default_ring_units(),
            tray_icon_entry_id: None,
            update_check_enabled: default_update_enabled(),
            update_last_check: None,
            update_proxy_port: None,
            update_proxy_host: None,
            update_auto_download: false,
            notifications_enabled: default_notifications_enabled(),
            background_refresh_enabled: false,
            background_refresh_interval_minutes: default_background_interval(),
            usage_comparison_series: None,
            usage_marker_lines: None,
        }
    }
}

impl Settings {
    /// 合法性收口：间隔 1..=1440 分钟、阈值 0..=100、语言/主题白名单、
    /// 每圈单位 1.0..=1e6（NaN/无穷回默认——JSON 正常解析不会产生，双保险）。
    pub fn sanitize(&mut self) {
        self.refresh_interval_minutes = self.refresh_interval_minutes.clamp(1, 1440);
        self.low_balance_threshold_percent = self.low_balance_threshold_percent.min(100);
        if !matches!(self.language.as_str(), "zh" | "en" | "system") {
            self.language = default_language();
        }
        if !matches!(self.theme.as_str(), "light" | "dark" | "system") {
            self.theme = default_theme();
        }
        if !self.ring_units_per_circle.is_finite() {
            self.ring_units_per_circle = default_ring_units();
        }
        self.ring_units_per_circle = self.ring_units_per_circle.clamp(1.0, 1e6);
        // 更新代理端口：0 非法（u16 类型上限 65535 已由类型保证）→ 视为未配置
        self.update_proxy_port = self.update_proxy_port.filter(|p| *p != 0);
        // 更新代理主机：坏输入不得透传——非法代理 URL 会让 build_engine
        // 失败进而挡住冷启动（settings 是非关键数据，不该有此破坏力）。
        // 收口规则：trim + 大小写归一（主机名大小写不敏感）→ 剥
        // http(s):// 前缀（粘贴完整 URL 的常见形态）→ 剥误带的数字
        // 端口尾巴（端口由独立字段承担）→ 尾部过滤：含 `:`（裸/带括号
        // IPv6）、控制/空白字符、`@ / ? # %`（URL 变义字符：`a@b` 把
        // `a` 变 userinfo 劫持 host 语义，`/ ? #` 吞端口，`%` 百分号
        // 编码无正当用例）一律视为未配置，最后对
        // `http://{host}:1` 做 URL 解析预检一次性拦下剩余非法形态
        //（全角冒号等 Unicode 标点，探针实证可使 reqwest 构造失败）；
        // 预检天然放行 IDN/全角数字主机。None → core 拼接回退
        // 127.0.0.1（直连可用的安全值）。
        self.update_proxy_host = self
            .update_proxy_host
            .take()
            .map(|raw| {
                let h = raw.trim().to_ascii_lowercase();
                let h = h
                    .strip_prefix("https://")
                    .or_else(|| h.strip_prefix("http://"))
                    .unwrap_or(&h)
                    .trim();
                let host = match h.rsplit_once(':') {
                    Some((head, tail))
                        if !head.is_empty()
                            && !tail.is_empty()
                            && tail.bytes().all(|b| b.is_ascii_digit()) =>
                    {
                        head
                    }
                    _ => h,
                };
                host.to_owned()
            })
            .filter(|h| {
                !h.is_empty()
                    && !h.contains(':')
                    && !h.contains(['@', '/', '?', '#', '%'])
                    && !h.chars().any(|c| c.is_control() || c.is_whitespace())
                    && url::Url::parse(&format!("http://{h}:1")).is_ok()
            });
        // 后台刷新周期：系统硬限 15 分钟（更小值会被 WorkManager 抬到 15，
        // 与其静默抬升不如落盘时就收口）；上限 6 小时（再长失去后台刷新意义）
        self.background_refresh_interval_minutes =
            self.background_refresh_interval_minutes.clamp(15, 360);
        if let Some(items) = self.usage_comparison_series.take() {
            self.usage_comparison_series =
                Some(quota_core::sanitize_usage_comparison_series(items));
        }
        // 定位线：过滤 0 与超 JS Date 安全域的坏值（防手改文件）、保序去重
        // （重复时刻时间差恒 0 无意义）、超上限时与前端同向丢弃最早写入的
        // 条目——前端 addUsageMarker 已收口，这里防手改文件与旧版残留。
        if let Some(items) = self.usage_marker_lines.take() {
            let mut seen = std::collections::HashSet::new();
            let mut cleaned: Vec<u64> = Vec::new();
            for ts in items {
                if ts > 0 && ts < MAX_USAGE_MARKER_TS && seen.insert(ts) {
                    cleaned.push(ts);
                }
            }
            if cleaned.len() > MAX_USAGE_MARKER_LINES {
                cleaned.drain(..cleaned.len() - MAX_USAGE_MARKER_LINES);
            }
            self.usage_marker_lines = Some(cleaned);
        }
    }

    /// 加载设置；文件缺失或损坏返回默认值（非关键数据，容错优先）。
    ///
    /// IO 失败（文件存在但读不出——开机自启时序下杀毒/同步盘短暂锁定
    /// 等）与"缺失"分流：缺失是正常态直接默认；IO 失败短重试后仍失败
    /// 才回退——静默回退会让引擎以"无代理端口"的幽灵默认运行，且启动
    /// 首检（run_check）会把默认值全量落盘、连磁盘上的真实设置一起抹掉。
    pub fn load(path: &Path) -> Self {
        let Some(text) = read_with_retry(path) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(mut s) => {
                s.sanitize();
                s
            }
            Err(e) => {
                log::warn!("settings.json 解析失败，回退默认设置：{e}");
                Self::default()
            }
        }
    }

    /// 原子保存（tmp + rename，与 core AppConfig 同一模式）。
    /// tmp 名含进程内递增序号：同进程并发保存（多查询同时触发收尾）不互踩。
    pub fn save(&self, path: &Path) -> Result<(), std::io::Error> {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = path.with_extension(format!("json.{}.{}.tmp", std::process::id(), seq));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        Ok(())
    }
}

/// 读取文件内容：不存在 → None（首次运行正常态）；存在但 IO 失败
/// （Windows 文件锁定抖动）→ 短重试 3 次，仍失败告警回 None。
/// 重试间隔 50ms：只吸收毫秒级锁定窗口，最坏多阻塞 ~150ms 不拖慢启动体感。
fn read_with_retry(path: &Path) -> Option<String> {
    let mut last_err = None;
    for attempt in 1..=3 {
        match std::fs::read_to_string(path) {
            Ok(text) => return Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                log::warn!("settings.json 读取失败（第 {attempt} 次）：{e}");
                last_err = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    log::warn!("settings.json 连续读取失败，回退默认设置：{last_err:?}");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "quotatray-settings-{tag}-{}.json",
            std::process::id()
        ));
        p
    }

    /// 契约：保存后加载 roundtrip 无损（含 M4 字段）。
    #[test]
    fn save_load_roundtrip() {
        let path = temp_path("roundtrip");
        let s = Settings {
            refresh_interval_minutes: 10,
            low_balance_threshold_percent: 70,
            autostart: true,
            language: "en".into(),
            theme: "dark".into(),
            ring_units_per_circle: 500.0,
            tray_icon_entry_id: Some("AB2C3D".into()),
            update_check_enabled: false,
            update_last_check: Some(1_700_000_000_000),
            update_proxy_port: Some(7897),
            update_proxy_host: Some("192.168.1.5".into()),
            update_auto_download: true,
            notifications_enabled: false,
            background_refresh_enabled: true,
            background_refresh_interval_minutes: 120,
            usage_comparison_series: Some(vec![quota_core::UsageComparisonSeries {
                provider_id: "AB2C3D".into(),
                window_key: "w0".into(),
                color_slot: 0,
            }]),
            usage_marker_lines: Some(vec![1_700_000_000_000, 1_700_086_000_000]),
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s, "完整设置 roundtrip 无损");
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：默认值——间隔 5 分钟、阈值 80%、语言/主题跟随系统、
    /// 每圈单位 100、图标条目自动（None）、不自启。
    #[test]
    fn defaults() {
        let s = Settings::default();
        assert_eq!(s.refresh_interval_minutes, 5);
        assert_eq!(s.low_balance_threshold_percent, 80);
        assert!(!s.autostart);
        assert_eq!(s.language, "system");
        assert_eq!(s.theme, "system");
        assert_eq!(s.ring_units_per_circle, 100.0);
        assert_eq!(s.tray_icon_entry_id, None);
        assert!(s.update_check_enabled, "自动检测默认开启");
        assert_eq!(s.update_last_check, None);
        assert_eq!(s.update_proxy_port, None, "默认不走代理");
        assert_eq!(
            s.update_proxy_host, None,
            "默认代理主机未配置（回退 127.0.0.1）"
        );
        assert!(!s.update_auto_download, "自动下载默认关闭");
        assert!(
            s.notifications_enabled,
            "系统通知默认开启（桌面现状行为不变）"
        );
        assert!(
            !s.background_refresh_enabled,
            "后台刷新默认关闭（Preview 谨慎）"
        );
        assert_eq!(s.background_refresh_interval_minutes, 30);
        assert_eq!(
            s.usage_comparison_series, None,
            "旧版/首次运行等待前端自动初始化"
        );
        assert_eq!(s.usage_marker_lines, None, "默认没有定位线");
    }

    #[test]
    fn sanitize_usage_comparison_keeps_four_unique_series_and_repairs_color_slots() {
        let mut s = Settings {
            usage_comparison_series: Some(vec![
                quota_core::UsageComparisonSeries {
                    provider_id: "p1".into(),
                    window_key: "w1".into(),
                    color_slot: 3,
                },
                quota_core::UsageComparisonSeries {
                    provider_id: "p1".into(),
                    window_key: "w1".into(),
                    color_slot: 0,
                },
                quota_core::UsageComparisonSeries {
                    provider_id: "p2".into(),
                    window_key: "w2".into(),
                    color_slot: 3,
                },
                quota_core::UsageComparisonSeries {
                    provider_id: "p3".into(),
                    window_key: "w3".into(),
                    color_slot: 8,
                },
                quota_core::UsageComparisonSeries {
                    provider_id: "p4".into(),
                    window_key: "w4".into(),
                    color_slot: 1,
                },
                quota_core::UsageComparisonSeries {
                    provider_id: "p5".into(),
                    window_key: "w5".into(),
                    color_slot: 2,
                },
            ]),
            ..Settings::default()
        };

        s.sanitize();

        let series = s.usage_comparison_series.unwrap();
        assert_eq!(series.len(), 4);
        assert_eq!(
            series
                .iter()
                .map(|item| item.color_slot)
                .collect::<Vec<_>>(),
            vec![3, 0, 1, 2]
        );
        assert_eq!(series[1].provider_id, "p2");
    }

    #[test]
    fn sanitize_usage_comparison_preserves_explicit_empty_and_drops_blank_keys() {
        let mut empty = Settings {
            usage_comparison_series: Some(Vec::new()),
            ..Settings::default()
        };
        empty.sanitize();
        assert_eq!(empty.usage_comparison_series, Some(Vec::new()));

        let mut blank = Settings {
            usage_comparison_series: Some(vec![quota_core::UsageComparisonSeries {
                provider_id: "  ".into(),
                window_key: "w0".into(),
                color_slot: 0,
            }]),
            ..Settings::default()
        };
        blank.sanitize();
        assert_eq!(blank.usage_comparison_series, Some(Vec::new()));
    }

    /// 契约：定位线 sanitize——过滤 0 与超大值、保序去重、与前端同向保留
    /// 最新两条（丢弃最早写入）；空列表保持显式空。
    #[test]
    fn sanitize_usage_marker_lines_dedupes_and_truncates() {
        let mut s = Settings {
            usage_marker_lines: Some(vec![0, 500, 500, 600, 700]),
            ..Settings::default()
        };
        s.sanitize();
        assert_eq!(
            s.usage_marker_lines,
            Some(vec![600, 700]),
            "0 被过滤、重复去重、超两条时丢弃最早写入保留最新"
        );

        let mut oversized = Settings {
            usage_marker_lines: Some(vec![9_000_000_000_000_000, 100]),
            ..Settings::default()
        };
        oversized.sanitize();
        assert_eq!(
            oversized.usage_marker_lines,
            Some(vec![100]),
            "超 JS Date 安全域的时刻被过滤（读数行 toISOString 会抛错）"
        );

        let mut cleared = Settings {
            usage_marker_lines: Some(Vec::new()),
            ..Settings::default()
        };
        cleared.sanitize();
        assert_eq!(cleared.usage_marker_lines, Some(Vec::new()));
    }

    /// 契约：部分字段的配置文件（老版本升级）回退字段级默认而非整体失败。
    #[test]
    fn partial_config_uses_field_defaults() {
        let path = temp_path("partial");
        std::fs::write(&path, r#"{"refresh_interval_minutes": 30}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.refresh_interval_minutes, 30);
        assert_eq!(s.low_balance_threshold_percent, 80);
        assert_eq!(s.language, "system");
        assert_eq!(s.theme, "system");
        assert_eq!(s.ring_units_per_circle, 100.0);
        assert_eq!(s.tray_icon_entry_id, None);
        assert!(s.notifications_enabled, "老版本配置缺字段回退默认开启");
        assert!(
            !s.background_refresh_enabled,
            "老版本配置缺字段回退默认关闭"
        );
        assert_eq!(s.background_refresh_interval_minutes, 30);
        assert_eq!(s.usage_marker_lines, None, "老版本配置缺定位线字段回退默认");
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：损坏的 settings.json 回退默认（不阻断启动）。
    #[test]
    fn corrupted_falls_back_to_default() {
        let path = temp_path("corrupted");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：IO 失败（路径是目录，非 NotFound 的读错误）重试后仍回退默认，
    /// 不 panic——启动路径上的文件锁定抖动必须能落到默认值继续启动。
    #[test]
    fn io_failure_falls_back_to_default_after_retry() {
        let dir =
            std::env::temp_dir().join(format!("quotatray-settings-iofail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 目录路径的 read_to_string 在 Windows（PermissionDenied）与
        // Linux（IsADirectory）上均为非 NotFound 错误
        assert_eq!(Settings::load(&dir), Settings::default());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：sanitize 收口越界值（语言/主题白名单、每圈单位 clamp）。
    #[test]
    fn sanitize_clamps_out_of_range() {
        let mut s = Settings {
            refresh_interval_minutes: 0,
            low_balance_threshold_percent: 150,
            autostart: false,
            language: "fr".into(),
            theme: "blue".into(),
            ring_units_per_circle: 0.5,
            tray_icon_entry_id: Some("X".into()),
            update_check_enabled: true,
            update_last_check: None,
            update_proxy_port: Some(7897),
            update_proxy_host: None,
            update_auto_download: false,
            notifications_enabled: true,
            background_refresh_enabled: true,
            background_refresh_interval_minutes: 9999,
            usage_comparison_series: None,
            usage_marker_lines: None,
        };
        s.sanitize();
        assert_eq!(s.refresh_interval_minutes, 1);
        assert_eq!(s.low_balance_threshold_percent, 100);
        assert_eq!(s.language, "system");
        assert_eq!(s.theme, "system");
        assert_eq!(s.ring_units_per_circle, 1.0, "低于下限应收到 1.0");
        s.ring_units_per_circle = 2e6;
        s.sanitize();
        assert_eq!(s.ring_units_per_circle, 1e6, "高于上限应收到 1e6");
        s.ring_units_per_circle = f64::NAN;
        s.sanitize();
        assert_eq!(
            s.ring_units_per_circle, 100.0,
            "NaN 应回默认而非 clamp panic"
        );
        // id 是自由字符串（失效 id 的回退语义由托盘侧处理），sanitize 不动它
        assert_eq!(s.tray_icon_entry_id, Some("X".into()));
        // 更新代理端口：0 非法（u16 类型上限 65535 已由类型保证）→ 视为未配置
        s.update_proxy_port = Some(0);
        s.sanitize();
        assert_eq!(s.update_proxy_port, None, "端口 0 视为未配置");
        s.update_proxy_port = Some(1);
        s.sanitize();
        assert_eq!(s.update_proxy_port, Some(1), "合法端口应保留");
        // 后台刷新周期：系统硬限 15 分钟，落盘时收口
        s.background_refresh_interval_minutes = 9999;
        s.sanitize();
        assert_eq!(s.background_refresh_interval_minutes, 360, "超上限收到 360");
        s.background_refresh_interval_minutes = 5;
        s.sanitize();
        assert_eq!(
            s.background_refresh_interval_minutes, 15,
            "低于系统硬限收到 15"
        );
        s.update_proxy_port = Some(u16::MAX);
        s.sanitize();
        assert_eq!(s.update_proxy_port, Some(u16::MAX), "上界端口应保留");
        // 更新代理主机：trim 收口、剥 http:// 前缀、空白视为未配置
        //（None 语义 = core proxy_url_of_host 回退 127.0.0.1）
        s.update_proxy_host = Some("  192.168.1.5  ".into());
        s.sanitize();
        assert_eq!(
            s.update_proxy_host,
            Some("192.168.1.5".into()),
            "主机应 trim"
        );
        s.update_proxy_host = Some("http://192.168.1.5".into());
        s.sanitize();
        assert_eq!(
            s.update_proxy_host,
            Some("192.168.1.5".into()),
            "http:// 前缀应剥离（拼接层统一补 scheme）"
        );
        s.update_proxy_host = Some("   ".into());
        s.sanitize();
        assert_eq!(s.update_proxy_host, None, "空白主机视为未配置");
        s.update_proxy_host = None;
        s.sanitize();
        assert_eq!(s.update_proxy_host, None, "None 保持不变");
        // review 轮收紧：粘贴完整 URL / 误带端口 / IPv6 / 非 http scheme
        // 的坏输入不得透传——否则拼接出非法代理 URL 会让 build_engine
        // 失败，冷启动直接被挡在启动弹窗（review 实证链条）
        s.update_proxy_host = Some("HTTP://192.168.1.5".into());
        s.sanitize();
        assert_eq!(
            s.update_proxy_host,
            Some("192.168.1.5".into()),
            "大写 scheme 同样剥离"
        );
        s.update_proxy_host = Some("https://Proxy.LAN".into());
        s.sanitize();
        assert_eq!(
            s.update_proxy_host,
            Some("proxy.lan".into()),
            "https 前缀剥离且大小写归一（主机名大小写不敏感）"
        );
        s.update_proxy_host = Some("192.168.1.5:8080".into());
        s.sanitize();
        assert_eq!(
            s.update_proxy_host,
            Some("192.168.1.5".into()),
            "误带数字端口尾巴应剥除（端口由独立字段承担）"
        );
        for bad in ["::1", "[::1]", "ftp://x", "host name"] {
            s.update_proxy_host = Some(bad.into());
            s.sanitize();
            assert_eq!(
                s.update_proxy_host, None,
                "IPv6 字面量/非 http scheme/含空格主机不支持，视为未配置：{bad}"
            );
        }
        // 第 2 轮 review 补：分支钉住 + 穿透/变义形态（url 预检收口）
        s.update_proxy_host = Some("a:b".into());
        s.sanitize();
        assert_eq!(s.update_proxy_host, None, "非数字尾巴不剥端口，含冒号拒收");
        s.update_proxy_host = Some("a:8080:9090".into());
        s.sanitize();
        assert_eq!(s.update_proxy_host, None, "多冒号剥最右一段后仍含冒号拒收");
        s.update_proxy_host = Some("host:".into());
        s.sanitize();
        assert_eq!(s.update_proxy_host, None, "空端口尾巴不剥，含冒号拒收");
        s.update_proxy_host = Some(":8080".into());
        s.sanitize();
        assert_eq!(s.update_proxy_host, None, "空主机含冒号拒收");
        s.update_proxy_host = Some("https://a.b:8080".into());
        s.sanitize();
        assert_eq!(
            s.update_proxy_host,
            Some("a.b".into()),
            "scheme + 误带端口复合形态全收口"
        );
        for bad in [
            "１９２：８０", // 全角冒号：URL 解析失败的穿透形态
            "a b",          // 中间空白（含全角/nbsp 由 is_whitespace 覆盖）
            "a@b",          // userinfo 变义：a 变 userinfo、host 劫持为 b
            "a/b",          // path 变义：端口被吞进路径
            "a#b",          // fragment 变义
            "a%2eb",        // 百分号编码形态
        ] {
            s.update_proxy_host = Some(bad.into());
            s.sanitize();
            assert_eq!(
                s.update_proxy_host, None,
                "穿透/变义形态应由 URL 预检与字符过滤拦截：{bad}"
            );
        }
    }

    /// 契约：既有 v1 settings（M3 旧字段集）加载不丢新字段默认值。
    #[test]
    fn legacy_config_without_m4_fields() {
        let path = temp_path("legacy");
        std::fs::write(
            &path,
            r#"{"refresh_interval_minutes":15,"low_balance_threshold_percent":90,"autostart":true,"language":"zh"}"#,
        )
        .unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.language, "zh", "旧文件已存语言应保留");
        assert_eq!(s.theme, "system");
        assert_eq!(s.ring_units_per_circle, 100.0);
        assert_eq!(s.tray_icon_entry_id, None);
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：旧版 settings.json 残留已删字段（如 update_check_time）时
    /// 忽略未知键正常加载——升级路径依赖 serde 默认容忍，防止未来引入
    /// `deny_unknown_fields` 静默破坏存量配置。
    #[test]
    fn legacy_removed_field_is_ignored() {
        let path = temp_path("legacy-removed");
        std::fs::write(
            &path,
            r#"{"update_check_time":"09:00","language":"en","update_last_check":123}"#,
        )
        .unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.language, "en", "已存字段正常读取");
        assert_eq!(s.update_last_check, Some(123));
        let _ = std::fs::remove_file(&path);
    }
}
