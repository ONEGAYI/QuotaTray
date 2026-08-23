//! 快照持久化：`~/.quotatray/cache.json`。
//!
//! 查询成功后写入 `{ id: { data, at } }`，启动时恢复到托盘与前端——
//! 重启先渲染上次成功结果（标注"上次于 N 分钟前"）再异步刷新，消除空窗
//! （GUI-spec §5）。文件格式由 serde 契约测试锁定。

use std::collections::BTreeMap;
use std::path::Path;

use quota_core::UsageData;
use serde::{Deserialize, Serialize};

/// 单条目的快照。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub data: Vec<UsageData>,
    /// 最后一次成功查询的时刻（Unix epoch 毫秒）。
    pub at: u64,
}

/// 快照文件整体（顶层即 `{ id: { data, at } }` 映射，flatten 展开）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshots {
    #[serde(flatten)]
    pub entries: BTreeMap<String, SnapshotEntry>,
}

impl Snapshots {
    /// 加载快照；文件缺失或损坏返回空（快照是缓存，坏则弃之）。
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// 原子保存（tmp + rename）。
    /// tmp 名含进程内递增序号：多查询并发触发的快照写盘不互踩。
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
            "quotatray-snapshot-{tag}-{}.json",
            std::process::id()
        ));
        p
    }

    fn sample() -> Snapshots {
        let data = UsageData {
            remaining: Some(62.97),
            unit: Some("CNY".into()),
            ..Default::default()
        };
        let mut entries = BTreeMap::new();
        entries.insert(
            "p1".into(),
            SnapshotEntry {
                data: vec![data],
                at: 1_755_000_000_000,
            },
        );
        Snapshots { entries }
    }

    /// 契约：保存后加载 roundtrip 无损。
    #[test]
    fn save_load_roundtrip() {
        let path = temp_path("roundtrip");
        sample().save(&path).unwrap();
        assert_eq!(Snapshots::load(&path), sample());
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：文件格式为 spec 规定的顶层 `{ id: { data, at } }`（无外层包装）。
    #[test]
    fn file_shape_is_flat_id_map() {
        let path = temp_path("shape");
        sample().save(&path).unwrap();
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(raw.get("p1").is_some(), "顶层应直接是 id 映射：{raw}");
        assert_eq!(raw["p1"]["at"], 1_755_000_000_000_i64);
        assert_eq!(raw["p1"]["data"][0]["remaining"], 62.97);
        assert_eq!(raw["p1"]["data"][0]["unit"], "CNY");
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：损坏的快照返回空（缓存语义，不阻断启动）。
    #[test]
    fn corrupted_falls_back_to_empty() {
        let path = temp_path("corrupted");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(Snapshots::load(&path), Snapshots::default());
        let _ = std::fs::remove_file(&path);
    }
}
