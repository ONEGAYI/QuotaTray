//! 提醒状态的跨重启持久化：`~/.quotatray/alert_state.json`（#132）。
//!
//! 两个职责：
//! 1. **低额度登记跨重启**：现有 [`crate::state::LOW_BALANCE_NOTIFIED`]
//!    是会话内存（重启即空，重启后首次查询会重复低额度提醒、且丢失
//!    「先前已进入低额度状态」的恢复判定前提）。本文件把该登记镜像到
//!    磁盘，进程启动（前台 init / Worker 每轮）灌入内存并集。
//! 2. **待展示恢复消息**：Android 后台 Worker 触发的恢复事件无前端可
//!    广播，落盘后由前台下次启动读取入列（读取即清，消息交会话内存
//!    接管）。
//!
//! 非关键数据：缺失/损坏回默认，写失败仅告警不阻断查询主链路。
//!
//! 并发口径：写路径是「读盘 → 应用本进程本轮边沿变化 → 原子写回」的
//! 增量合并（不整体覆盖），前台进程与 Worker 的写入不互踩对方条目；
//! 极端同时写仍可能丢一次对方刚写入的增量（边沿变化本身罕见、下一轮
//! 查询会重新收敛），原子写（tmp + rename）保证文件本身不损坏。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

/// 待展示的恢复消息（Worker 触发落盘 / 前台启动读取的 IPC 形状）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoveryNotice {
    pub provider_id: String,
    /// 条目显示名（触发时刻快照，条目随后改名不影响已落盘消息）。
    pub name: String,
    /// 恢复时的最低剩余百分比（0-100，参与判定的 % 窗口中最保守值）。
    pub remaining_percent: f64,
    /// 恢复事件时刻（epoch 毫秒）。
    pub at: u64,
}

/// 提醒状态文件整体。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AlertState {
    /// 处于低额度状态（已通知、未恢复）的条目 id 集合——
    /// [`crate::state::LOW_BALANCE_NOTIFIED`] 的跨重启镜像。
    pub low_balance: BTreeSet<String>,
    /// 待展示恢复消息（按条目 id 去重，同条目新消息覆盖旧消息）。
    pub pending_recovery: BTreeMap<String, RecoveryNotice>,
}

impl AlertState {
    /// 加载；文件缺失或损坏返回默认（非关键数据，坏则弃之）。
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// 原子保存（tmp + rename，与 settings/snapshot 同一模式）。
    /// tmp 名含进程内递增序号：同进程多次边沿变化落盘不互踩。
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

/// 把本进程本轮的边沿变化合并进盘上状态（读-改-写）：低额度登记增删、
/// 恢复事件消息。失败由调用方告警（回退会话语义，不阻断查询主链路）。
pub fn commit_low_edge(
    path: &Path,
    low_added: &[&str],
    low_removed: &[&str],
    recovery: Option<RecoveryNotice>,
) -> Result<(), std::io::Error> {
    let mut state = AlertState::load(path);
    for id in low_added {
        state.low_balance.insert((*id).to_string());
    }
    for id in low_removed {
        state.low_balance.remove(*id);
    }
    if let Some(notice) = recovery {
        state
            .pending_recovery
            .insert(notice.provider_id.clone(), notice);
    }
    state.save(path)
}

/// 读取并清空待展示恢复消息（读取即清：消息已交前台会话内存接管，
/// 遗留会导致下次启动重复红点）。读改写失败时返回盘上现值并告警
/// （宁可下次重复入列也不丢失消息）。
pub fn take_pending_recovery(path: &Path) -> Vec<RecoveryNotice> {
    let mut state = AlertState::load(path);
    let taken = state.pending_recovery.values().cloned().collect::<Vec<_>>();
    if taken.is_empty() {
        return taken;
    }
    state.pending_recovery.clear();
    if let Err(e) = state.save(path) {
        log::warn!("待展示恢复消息清除失败（下次启动可能重复入列）：{e}");
    }
    taken
}

/// 清除单条待展示恢复消息（前台收到 balance-recovered 广播入列后回执：
/// Worker 抢先落盘的消息已由本会话展示，不该再等下次启动）。
pub fn ack_pending_recovery(path: &Path, provider_id: &str) {
    let mut state = AlertState::load(path);
    if state.pending_recovery.remove(provider_id).is_some()
        && let Err(e) = state.save(path)
    {
        log::warn!("恢复消息回执清除失败（{provider_id}）：{e}");
    }
}

/// 盘上低额度登记灌入会话内存（并集）：前台进程启动（AppState::init）
/// 与 Worker 每轮刷新前调用——重启/冷启动后「先前已进入低额度状态」
/// 的恢复判定前提与低额度防重都从盘恢复，双路共享同一防重。
pub fn hydrate_low_balance_notified(path: &Path) {
    let disk = AlertState::load(path);
    if disk.low_balance.is_empty() {
        return;
    }
    let mut notified = crate::state::LOW_BALANCE_NOTIFIED.lock().unwrap();
    notified.extend(disk.low_balance);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("qt-alert-state-{tag}-{}.json", std::process::id()))
    }

    fn notice(id: &str, remaining: f64) -> RecoveryNotice {
        RecoveryNotice {
            provider_id: id.into(),
            name: format!("条目 {id}"),
            remaining_percent: remaining,
            at: 1_755_000_000_000,
        }
    }

    /// 契约：文件缺失或损坏回默认（空状态）——非关键数据不阻断启动。
    #[test]
    fn load_missing_or_corrupted_falls_back_to_default() {
        let path = temp_path("missing");
        let _ = std::fs::remove_file(&path);
        assert_eq!(AlertState::load(&path), AlertState::default());

        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(AlertState::load(&path), AlertState::default());
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：读-改-写合并——本进程边沿变化（登记/清除/恢复消息）合并进
    /// 盘上现值而非整体覆盖内存快照，前台进程与 Worker 的并发写入不互踩。
    #[test]
    fn commit_edge_merges_into_disk_state() {
        let path = temp_path("merge");
        let _ = std::fs::remove_file(&path);

        commit_low_edge(&path, &["A"], &[], None).unwrap();
        assert_eq!(
            AlertState::load(&path).low_balance,
            ["A".to_string()].into_iter().collect(),
            "首次登记落盘"
        );

        commit_low_edge(&path, &["B"], &["A"], None).unwrap();
        let merged = AlertState::load(&path);
        assert_eq!(
            merged.low_balance,
            ["B".to_string()].into_iter().collect(),
            "增量合并：新登记进入、旧清除移除，不覆盖盘上无关条目"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// 契约：恢复事件落盘待展示消息（按条目 id 去重，同条目新消息覆盖
    /// 旧消息）并清除低额度登记。
    #[test]
    fn commit_edge_records_recovery_notice() {
        let path = temp_path("recovery");
        let _ = std::fs::remove_file(&path);
        commit_low_edge(&path, &["A", "B"], &[], None).unwrap();

        commit_low_edge(&path, &[], &["A"], Some(notice("A", 96.0))).unwrap();
        let state = AlertState::load(&path);
        assert_eq!(
            state.low_balance,
            ["B".to_string()].into_iter().collect(),
            "恢复条目的低额度登记同步清除"
        );
        assert_eq!(state.pending_recovery.len(), 1, "恢复消息落盘");
        assert_eq!(state.pending_recovery["A"].remaining_percent, 96.0);

        // 同条目再次恢复（低→高→低→高）：新消息覆盖旧消息不叠加
        commit_low_edge(&path, &[], &[], Some(notice("A", 98.0))).unwrap();
        let updated = AlertState::load(&path);
        assert_eq!(updated.pending_recovery.len(), 1);
        assert_eq!(
            updated.pending_recovery["A"].remaining_percent, 98.0,
            "同条目恢复消息按 id 去重，保留最新"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// 契约：读取即清——take_pending_recovery 返回全部待展示消息并把
    /// 队列清空（消息已交前台会话内存接管），低额度登记不受影响。
    /// 覆盖「后台写入 → 前台可见」链路与跨重启（load/take 是纯磁盘操作，
    /// 模拟两进程读写同一文件）。
    #[test]
    fn take_pending_recovery_reads_and_clears() {
        let path = temp_path("take");
        let _ = std::fs::remove_file(&path);
        commit_low_edge(&path, &["A"], &[], Some(notice("A", 96.0))).unwrap();
        // 模拟另一进程（Worker）追加第二条
        commit_low_edge(&path, &[], &[], Some(notice("B", 97.0))).unwrap();

        let taken = take_pending_recovery(&path);
        assert_eq!(
            taken
                .iter()
                .map(|n| n.provider_id.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B"],
            "全部待展示消息返回（BTreeMap 按 id 有序）"
        );
        assert_eq!(taken[0].name, "条目 A");
        assert_eq!(taken[0].remaining_percent, 96.0);

        let state = AlertState::load(&path);
        assert!(state.pending_recovery.is_empty(), "读取即清");
        assert_eq!(
            state.low_balance,
            ["A".to_string()].into_iter().collect(),
            "低额度登记不受 take 影响"
        );
        assert!(
            take_pending_recovery(&path).is_empty(),
            "再次读取返回空（不重复入列）"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// 契约：跨重启状态保留——登记与待展示消息经 save/load 往返无损
    /// （重启后恢复判定前提仍在）。
    #[test]
    fn roundtrip_preserves_state_across_restart() {
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        let state = AlertState {
            low_balance: ["P1".to_string(), "P2".to_string()].into_iter().collect(),
            pending_recovery: [("P1".to_string(), notice("P1", 95.5))]
                .into_iter()
                .collect(),
        };
        state.save(&path).unwrap();
        assert_eq!(AlertState::load(&path), state, "跨重启往返无损");
        let _ = std::fs::remove_file(&path);
    }

    /// 契约：磁盘登记灌入会话内存取并集——启动/Worker 每轮把盘上条目
    /// 合入 LOW_BALANCE_NOTIFIED（不冲掉本进程已有登记，双路防重共享）。
    #[test]
    fn hydrate_unions_disk_into_session_set() {
        let path = temp_path("hydrate");
        let _ = std::fs::remove_file(&path);
        commit_low_edge(&path, &["DISK1", "DISK2"], &[], None).unwrap();

        {
            let mut notified = crate::state::LOW_BALANCE_NOTIFIED.lock().unwrap();
            notified.clear();
            notified.insert("MEM".to_string());
        }
        hydrate_low_balance_notified(&path);
        let notified = crate::state::LOW_BALANCE_NOTIFIED.lock().unwrap();
        let expected = ["DISK1", "DISK2", "MEM"]
            .iter()
            .map(|s| s.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            notified
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            expected,
            "盘上条目并集进内存，本进程已有登记保留"
        );
        drop(notified);
        crate::state::LOW_BALANCE_NOTIFIED.lock().unwrap().clear();
        let _ = std::fs::remove_file(&path);
    }
}
