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
    /// 界面语言（M3 仅占位，zh/en 后续版本）。
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_interval() -> u32 {
    5
}

fn default_threshold() -> u8 {
    80
}

fn default_language() -> String {
    "zh".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_interval_minutes: default_interval(),
            low_balance_threshold_percent: default_threshold(),
            autostart: false,
            language: default_language(),
        }
    }
}

impl Settings {
    /// 合法性收口：间隔 1..=1440 分钟、阈值 0..=100、语言白名单。
    pub fn sanitize(&mut self) {
        self.refresh_interval_minutes = self.refresh_interval_minutes.clamp(1, 1440);
        self.low_balance_threshold_percent = self.low_balance_threshold_percent.min(100);
        if !matches!(self.language.as_str(), "zh" | "en") {
            self.language = default_language();
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

    /// 契约：保存后加载 roundtrip 无损。
    #[test]
    fn save_load_roundtrip() {
        let path = temp_path("roundtrip");
        let s = Settings {
            refresh_interval_minutes: 10,
            low_balance_threshold_percent: 70,
            autostart: true,
            language: "en".into(),
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：默认值——间隔 5 分钟、阈值 80%、中文、不自启。
    #[test]
    fn defaults() {
        let s = Settings::default();
        assert_eq!(s.refresh_interval_minutes, 5);
        assert_eq!(s.low_balance_threshold_percent, 80);
        assert!(!s.autostart);
        assert_eq!(s.language, "zh");
    }

    /// 契约：部分字段的配置文件（老版本升级）回退字段级默认而非整体失败。
    #[test]
    fn partial_config_uses_field_defaults() {
        let path = temp_path("partial");
        std::fs::write(&path, r#"{"refresh_interval_minutes": 30}"#).unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.refresh_interval_minutes, 30);
        assert_eq!(s.low_balance_threshold_percent, 80);
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

    /// 契约：sanitize 收口越界值。
    #[test]
    fn sanitize_clamps_out_of_range() {
        let mut s = Settings {
            refresh_interval_minutes: 0,
            low_balance_threshold_percent: 150,
            autostart: false,
            language: "fr".into(),
        };
        s.sanitize();
        assert_eq!(s.refresh_interval_minutes, 1);
        assert_eq!(s.low_balance_threshold_percent, 100);
        assert_eq!(s.language, "zh");
    }
}
