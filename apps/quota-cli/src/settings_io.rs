//! CLI 侧 settings.json 读改写：更新偏好与使用统计比较组合。
//!
//! settings.json 归桌面端拥有（完整 Settings 结构在 quota-desktop crate，
//! CLI 不依赖它）；本模块以 mini struct 读取、以 `serde_json::Value`
//! 读改写回写——保留 CLI 不认识的其他字段。
//!
//! GUI 写设置前会从磁盘合并 CLI 拥有的 `update_last_check` 与比较组合字段，
//! 避免常驻 GUI 用旧内存状态覆盖 CLI 导入结果。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

/// settings.json 与 config.json 同目录（与 lang.rs 的语言读取同源推导）。
fn settings_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|d| d.join("settings.json"))
        .unwrap_or_else(|| PathBuf::from("settings.json"))
}

/// 当前 epoch 毫秒（时钟异常回退 0，调用方语义不受破坏——只是节流重置）。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 更新相关偏好（仅读取本模块关心的字段，容忍未知字段与缺字段）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UpdatePrefs {
    #[serde(default = "default_enabled")]
    pub update_check_enabled: bool,
    #[serde(default)]
    pub update_last_check: Option<u64>,
    /// 更新通道代理端口（GUI 设置页写入；None = 直连/环境变量）。
    #[serde(default)]
    pub update_proxy_port: Option<u16>,
}

fn default_enabled() -> bool {
    true
}

impl Default for UpdatePrefs {
    fn default() -> Self {
        Self {
            update_check_enabled: true,
            update_last_check: None,
            update_proxy_port: None,
        }
    }
}

/// 读取更新偏好：文件缺失/损坏/字段缺失 → 全默认（与桌面端同语义）。
pub fn load_prefs(config_path: &Path) -> UpdatePrefs {
    let p = settings_path(config_path);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 读取并归一化使用统计比较组合；字段/文件缺失视为旧版未初始化，
/// 损坏内容显式返回错误，供导出命令提示降级。
pub fn load_usage_comparison(
    config_path: &Path,
) -> std::io::Result<Option<Vec<quota_core::UsageComparisonSeries>>> {
    let p = settings_path(config_path);
    let text = match std::fs::read_to_string(p) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let root: serde_json::Value = serde_json::from_str(&text).map_err(std::io::Error::other)?;
    let Some(value) = root.get("usage_comparison_series") else {
        return Ok(None);
    };
    let items: Vec<quota_core::UsageComparisonSeries> =
        serde_json::from_value(value.clone()).map_err(std::io::Error::other)?;
    Ok(Some(quota_core::sanitize_usage_comparison_series(items)))
}

/// 导入迁移包后替换比较组合；None 删除字段，保留 settings.json 其他键。
pub fn write_usage_comparison(
    config_path: &Path,
    value: Option<&[quota_core::UsageComparisonSeries]>,
) -> std::io::Result<()> {
    let p = settings_path(config_path);
    let mut root: serde_json::Value = match std::fs::read_to_string(&p) {
        Ok(text) => serde_json::from_str(&text).map_err(std::io::Error::other)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(e),
    };
    if !root.is_object() {
        root = serde_json::json!({});
    }
    let object = root.as_object_mut().expect("normalized object");
    match value {
        Some(items) => {
            let items = quota_core::sanitize_usage_comparison_series(items.to_vec());
            object.insert(
                "usage_comparison_series".into(),
                serde_json::to_value(items).map_err(std::io::Error::other)?,
            );
        }
        None => {
            object.remove("usage_comparison_series");
        }
    }
    atomic_write(&p, &root)
}

/// 写回 `update_last_check`（Value 读改写保留未知字段 + 原子写）。
/// 文件不存在时新建仅含该字段的对象（桌面端加载时其余字段走默认）。
///
/// 读失败（非不存在）或内容损坏时**跳过写回**：从空对象重建再全量落盘
/// 会把 GUI 写入的其余设置（代理端口、主题等）整个抹掉；节流时间戳
/// 少记一次只导致下次多检一遍更新，无害。
pub fn write_last_check(config_path: &Path, now_ms: u64) -> std::io::Result<()> {
    let p = settings_path(config_path);
    let mut root: serde_json::Value = match std::fs::read_to_string(&p) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("settings.json 解析失败，跳过节流时间戳写回（保留原文件）：{e}");
                return Ok(());
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => {
            eprintln!("settings.json 读取失败，跳过节流时间戳写回（保留原文件）：{e}");
            return Ok(());
        }
    };
    // 合法 JSON 但非对象（如历史遗留数组）→ 重建为对象
    if !root.is_object() {
        root = serde_json::json!({});
    }
    root["update_last_check"] = serde_json::json!(now_ms);
    atomic_write(&p, &root)
}

/// JSON 原子写（tmp + rename，pid + 进程内序号，与 GUI settings /
/// core config 同一模式）。
fn atomic_write(path: &Path, v: &serde_json::Value) -> std::io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(v).map_err(std::io::Error::other)?;
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{}.{}.tmp", std::process::id(), seq));
    std::fs::write(&tmp, text)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quota-cli-setio-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 契约：读取容忍缺失/损坏/缺字段，均回默认。
    #[test]
    fn load_prefs_fallbacks() {
        let dir = temp_dir("load");
        let cfg = dir.join("config.json");

        assert_eq!(
            load_prefs(&cfg),
            UpdatePrefs::default(),
            "文件不存在 → 默认"
        );

        std::fs::write(dir.join("settings.json"), "{ not json").unwrap();
        assert_eq!(load_prefs(&cfg), UpdatePrefs::default(), "损坏 → 默认");

        std::fs::write(
            dir.join("settings.json"),
            r#"{"update_check_enabled":false,"theme":"dark","update_proxy_port":7897}"#,
        )
        .unwrap();
        let p = load_prefs(&cfg);
        assert!(!p.update_check_enabled, "已存字段正常读取");
        assert_eq!(p.update_last_check, None);
        assert_eq!(p.update_proxy_port, Some(7897), "GUI 写入的代理端口可读");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：旧版 settings.json 残留已删字段（update_check_time）被忽略，
    /// 其余字段正常读取——升级路径依赖 serde 默认容忍未知键。
    #[test]
    fn load_prefs_ignores_removed_field() {
        let dir = temp_dir("legacy-removed");
        let cfg = dir.join("config.json");
        std::fs::write(
            dir.join("settings.json"),
            r#"{"update_check_time":"09:00","update_check_enabled":true,"update_last_check":77}"#,
        )
        .unwrap();
        let p = load_prefs(&cfg);
        assert!(p.update_check_enabled);
        assert_eq!(p.update_last_check, Some(77));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：写回 last_check 保留未知字段（桌面端并行新增的 theme 等）。
    #[test]
    fn write_last_check_preserves_unknown_fields() {
        let dir = temp_dir("write");
        let cfg = dir.join("config.json");
        std::fs::write(
            dir.join("settings.json"),
            r#"{"theme":"dark","language":"en","update_last_check":100}"#,
        )
        .unwrap();

        write_last_check(&cfg, 42_000).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(v["update_last_check"], 42_000, "时间戳已更新");
        assert_eq!(v["theme"], "dark", "未知字段保留");
        assert_eq!(v["language"], "en", "未知字段保留");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：文件不存在 / 非对象 JSON 时新建合法对象；不留 tmp 残留。
    #[test]
    fn write_last_check_creates_and_no_tmp_left() {
        let dir = temp_dir("create");
        let cfg = dir.join("config.json");

        write_last_check(&cfg, 7).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(v["update_last_check"], 7, "新建文件仅含 last_check");

        std::fs::write(dir.join("settings.json"), "[1,2]").unwrap();
        write_last_check(&cfg, 9).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(v["update_last_check"], 9, "非对象 JSON 重建为对象");

        let tmps: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(tmps.is_empty(), "不留 tmp 残留");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：settings.json 损坏时跳过写回——原文件内容原样保留，
    /// 不得用空对象重建把 GUI 写入的其余设置抹掉。
    #[test]
    fn write_last_check_skips_on_corrupted_file() {
        let dir = temp_dir("skip-corrupt");
        let cfg = dir.join("config.json");
        let corrupted = "{ not json";
        std::fs::write(dir.join("settings.json"), corrupted).unwrap();

        write_last_check(&cfg, 42).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json")).unwrap(),
            corrupted,
            "损坏文件原样保留，不重建为空壳"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：读取 IO 失败（非不存在）同样跳过写回——目录占位模拟
    /// 文件锁定/权限类读错误，断言目录不被文件覆盖。
    #[test]
    fn write_last_check_skips_on_io_failure() {
        let dir = temp_dir("skip-io");
        let cfg = dir.join("config.json");
        let blocker = dir.join("settings.json");
        std::fs::create_dir_all(&blocker).unwrap();

        write_last_check(&cfg, 42).unwrap();
        assert!(blocker.is_dir(), "IO 失败时不产生覆盖写");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn usage_comparison_roundtrip_preserves_unknown_settings_and_none_removes_field() {
        let dir = temp_dir("usage-comparison");
        let cfg = dir.join("config.json");
        std::fs::write(dir.join("settings.json"), r#"{"theme":"dark"}"#).unwrap();
        let items = vec![quota_core::UsageComparisonSeries {
            provider_id: "p1".into(),
            window_key: "w1".into(),
            color_slot: 2,
        }];

        write_usage_comparison(&cfg, Some(&items)).unwrap();
        assert_eq!(load_usage_comparison(&cfg).unwrap(), Some(items));
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["theme"], "dark");

        write_usage_comparison(&cfg, None).unwrap();
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert!(saved.get("usage_comparison_series").is_none());
        assert_eq!(saved["theme"], "dark");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn usage_comparison_read_and_write_share_core_sanitize_rules() {
        let dir = temp_dir("usage-comparison-sanitize");
        let cfg = dir.join("config.json");
        let invalid = vec![
            quota_core::UsageComparisonSeries {
                provider_id: " p1 ".into(),
                window_key: " w1 ".into(),
                color_slot: 9,
            },
            quota_core::UsageComparisonSeries {
                provider_id: "p1".into(),
                window_key: "w1".into(),
                color_slot: 0,
            },
        ];

        write_usage_comparison(&cfg, Some(&invalid)).unwrap();
        assert_eq!(
            load_usage_comparison(&cfg).unwrap(),
            Some(vec![quota_core::UsageComparisonSeries {
                provider_id: "p1".into(),
                window_key: "w1".into(),
                color_slot: 0,
            }])
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
