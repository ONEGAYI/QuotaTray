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
    /// 自动检测更新（启动时 + 每日定时；CLI 启动钩子共用该开关）。
    #[serde(default = "default_update_enabled")]
    pub update_check_enabled: bool,
    /// 每日定时检测时刻（"HH:MM" 24 小时制，GUI 常驻调度用）。
    #[serde(default = "default_update_time")]
    pub update_check_time: String,
    /// 上次自动检测时间（epoch 毫秒；CLI 与 GUI 共写做 24h 节流）。
    #[serde(default)]
    pub update_last_check: Option<u64>,
}

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

fn default_update_time() -> String {
    "09:00".into()
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
            update_check_time: default_update_time(),
            update_last_check: None,
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
        // 每日检测时刻：非法 HH:MM 回默认（core::update::parse_hhmm 为判据）
        if quota_core::update::parse_hhmm(&self.update_check_time).is_none() {
            self.update_check_time = default_update_time();
        }
    }

    /// 加载设置；文件缺失或损坏返回默认值（非关键数据，容错优先）。
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
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
            update_check_time: "12:30".into(),
            update_last_check: Some(1_700_000_000_000),
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
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
        assert_eq!(s.update_check_time, "09:00");
        assert_eq!(s.update_last_check, None);
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
            update_check_time: "09:00".into(),
            update_last_check: None,
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
        // 更新检测时刻：非法 HH:MM 回默认（update_check_enabled 是 bool 无需收口）
        s.update_check_time = "9时".into();
        s.sanitize();
        assert_eq!(s.update_check_time, "09:00");
        s.update_check_time = "24:99".into();
        s.sanitize();
        assert_eq!(s.update_check_time, "09:00");
        s.update_check_time = "23:59".into();
        s.sanitize();
        assert_eq!(s.update_check_time, "23:59", "合法时刻应保留");
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
}
