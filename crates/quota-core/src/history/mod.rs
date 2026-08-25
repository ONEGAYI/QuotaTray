//! 查询结果的历史存储（M5）：SQLite 时序表 + 版本化 schema 迁移。
//!
//! 设计要点：
//! - 一条 [`UsageData`] 一行，主键 `(provider_id, window_key, sampled_at)`，
//!   同毫秒重放幂等（`INSERT OR REPLACE`）；多窗口条目一次查询产生多行。
//! - 仅存走势所需的数值列（used/remaining/total/unit），不保真 extra 等
//!   细节——将来需要时通过 schema 迁移加列。
//! - 滚动保留 [`DEFAULT_RETENTION_DAYS`] 天，写入时节流触发清理。
//! - schema 演进走 `PRAGMA user_version` + [`MIGRATIONS`] 逐版本事务应用；
//!   库版本比二进制新（降级运行）时拒绝打开。
//!
//! 历史库是非关键附属数据：调用方（CLI/桌面端）对写失败应静默告警，
//! 不得影响查询主链路与退出码。

use std::cell::Cell;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::model::UsageData;

/// 滚动保留天数（30 天）。
pub const DEFAULT_RETENTION_DAYS: u64 = 30;

/// 清理节流间隔：两次滚动清理之间的最小间隔（毫秒）。
const CLEANUP_INTERVAL_MS: u64 = 60 * 60 * 1000;

/// schema 迁移脚本，下标 i 的脚本把库从版本 i 升到 i+1。
/// 只允许追加，不允许修改已发布条目。
const MIGRATIONS: &[&str] = &[
    // v0 -> v1：初始表 + sampled_at 索引（滚动清理的 DELETE 走该索引）。
    "CREATE TABLE history (
        provider_id TEXT NOT NULL,
        window_key  TEXT NOT NULL,
        sampled_at  INTEGER NOT NULL,
        used        REAL,
        remaining   REAL,
        total       REAL,
        unit        TEXT,
        PRIMARY KEY (provider_id, window_key, sampled_at)
    ) WITHOUT ROWID;
    CREATE INDEX idx_history_sampled_at ON history(sampled_at);",
];

/// 历史库打开、迁移或读写失败。
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("历史库目录创建失败：{0}")]
    Io(#[source] std::io::Error),
    #[error("历史库打开或初始化失败：{0}")]
    Open(#[source] rusqlite::Error),
    #[error("历史库迁移到版本 {version} 失败：{source}")]
    Migrate {
        version: u16,
        #[source]
        source: rusqlite::Error,
    },
    #[error("历史库版本过新（v{version}），请先升级应用")]
    NewerVersion { version: i64 },
    #[error("历史库版本号异常（v{version}，可能被外部工具篡改），可删除 history.db 重建")]
    AbnormalVersion { version: i64 },
    #[error("历史库读写失败：{0}")]
    Db(#[from] rusqlite::Error),
}

/// 指定条目自某时刻起、全部窗口的历史点（serde 形状预留给未来 IPC）。
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct HistoryPoint {
    pub window_key: String,
    /// 采样时刻（epoch 毫秒）。
    pub sampled_at: u64,
    pub used: Option<f64>,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub unit: Option<String>,
}

/// 迁移容器（`.qtray-export` v2）携带的历史行，仅数值列。
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct HistoryExportRow {
    pub provider_id: String,
    pub window_key: String,
    /// 采样时刻（epoch 毫秒）。
    pub sampled_at: u64,
    pub used: Option<f64>,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub unit: Option<String>,
}

/// 窗口键推导：`plan_name` 非空（去空白）取之，否则回退序数 `w{ordinal}`。
///
/// 同一条目的返回结构随平台实现稳定，因此 plan_name 在条目内是稳定键。
/// 约束：native 各平台的 plan_name 文案（如「Claude 订阅（5h）」）已
/// **冻结为窗口键**——调整展示文案会整体断掉历史时间线，i18n 化等改动
/// 需保持键不变（展示层另做映射）。
///
/// 同次查询出现重复键的两种来源：模板 `windowsFrom` 数组展开对每个元素
/// 产出同名行（M2a 既有行为，配置数组内重名窗口由 `template::validate`
/// 拒绝），以及 script 返回重复 `plan_name`。[`HistoryStore::record`]
/// 对重复键按出现顺序追加 `#2`、`#3` 消歧——跨查询的键稳定性依赖
/// 返回数组顺序稳定，见 history-spec §2。
pub fn window_key(data: &UsageData, ordinal: usize) -> String {
    match data.plan_name.as_deref() {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => format!("w{ordinal}"),
    }
}

/// 历史库句柄。多线程共享由调用方加锁（rusqlite `Connection` 非 `Sync`）。
#[derive(Debug)]
pub struct HistoryStore {
    conn: Connection,
    /// 上次滚动清理时刻（epoch 毫秒）；0 表示进程启动后尚未清理，
    /// 保证首次写入必清一次过期数据。
    last_cleanup_ms: Cell<u64>,
}

impl HistoryStore {
    /// 打开（或创建）指定路径的历史库：建目录 → WAL/NORMAL → 迁移。
    pub fn open(path: &Path) -> Result<Self, HistoryError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(HistoryError::Io)?;
        }
        let conn = Connection::open(path).map_err(HistoryError::Open)?;
        Self::init(conn)
    }

    /// 内存库：测试与桌面端打开失败时的降级。
    pub fn open_in_memory() -> Result<Self, HistoryError> {
        let conn = Connection::open_in_memory().map_err(HistoryError::Open)?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Self, HistoryError> {
        // WAL 不能在事务内设置；内存库会返回 "memory"，不视为错误。
        let _mode: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .map_err(HistoryError::Open)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(HistoryError::Open)?;
        // 写锁竞争等待：桌面端常驻进程与 CLI 并发写同一库时，
        // 无 busy handler 会立即报 database is locked 并静默丢点
        conn.busy_timeout(std::time::Duration::from_millis(3000))
            .map_err(HistoryError::Open)?;
        migrate(&mut conn)?;
        Ok(Self {
            conn,
            last_cleanup_ms: Cell::new(0),
        })
    }

    /// 一次成功查询的多窗口数据落库（单事务，同毫秒重放幂等），
    /// 并按 [`CLEANUP_INTERVAL_MS`] 节流触发滚动清理。
    ///
    /// `is_valid == Some(false)` 的行跳过：凭据失效期间数值不可信
    /// （常为 0 而非空），落库会在走势上留下无法区分的假断崖；
    /// 失效期间时间线中断，由读取方按空档呈现。
    ///
    /// 同次查询出现重复窗口键时按出现顺序追加 `#2`、`#3` 消歧
    /// （见 [`window_key`]）。
    pub fn record(
        &self,
        provider_id: &str,
        data: &[UsageData],
        at_ms: u64,
    ) -> Result<(), HistoryError> {
        let tx = self.conn.unchecked_transaction()?;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (ordinal, item) in data.iter().enumerate() {
            if item.is_valid == Some(false) {
                continue;
            }
            let mut key = window_key(item, ordinal);
            if !seen.insert(key.clone()) {
                let mut suffix = 2;
                while !seen.insert(format!("{key}#{suffix}")) {
                    suffix += 1;
                }
                key = format!("{key}#{suffix}");
            }
            tx.execute(
                "INSERT OR REPLACE INTO history
                    (provider_id, window_key, sampled_at, used, remaining, total, unit)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    provider_id,
                    key,
                    at_ms as i64,
                    item.used,
                    item.remaining,
                    item.total,
                    item.unit,
                ],
            )?;
        }
        tx.commit()?;
        self.maybe_cleanup(at_ms);
        Ok(())
    }

    /// 指定条目自 `from_ms`（含）起全部窗口的点，按采样时刻升序。
    pub fn range(
        &self,
        provider_id: &str,
        from_ms: u64,
    ) -> Result<Vec<HistoryPoint>, HistoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT window_key, sampled_at, used, remaining, total, unit
             FROM history
             WHERE provider_id = ?1 AND sampled_at >= ?2
             ORDER BY sampled_at ASC",
        )?;
        let rows = stmt.query_map(params![provider_id, from_ms as i64], |row| {
            Ok(HistoryPoint {
                window_key: row.get(0)?,
                sampled_at: row.get::<_, i64>(1)? as u64,
                used: row.get(2)?,
                remaining: row.get(3)?,
                total: row.get(4)?,
                unit: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(HistoryError::Db)
    }

    /// 清除历史：`Some(id)` 清单条目，`None` 清空全部。
    pub fn clear(&self, provider_id: Option<&str>) -> Result<(), HistoryError> {
        match provider_id {
            Some(id) => {
                self.conn
                    .execute("DELETE FROM history WHERE provider_id = ?1", params![id])?;
            }
            None => {
                self.conn.execute("DELETE FROM history", params![])?;
            }
        }
        Ok(())
    }

    /// 全量导出（跨机器迁移用），按 (provider_id, window_key, sampled_at) 排序。
    pub fn export_rows(&self) -> Result<Vec<HistoryExportRow>, HistoryError> {
        let mut stmt = self.conn.prepare(
            "SELECT provider_id, window_key, sampled_at, used, remaining, total, unit
             FROM history
             ORDER BY provider_id, window_key, sampled_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(HistoryExportRow {
                provider_id: row.get(0)?,
                window_key: row.get(1)?,
                sampled_at: row.get::<_, i64>(2)? as u64,
                used: row.get(3)?,
                remaining: row.get(4)?,
                total: row.get(5)?,
                unit: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(HistoryError::Db)
    }

    /// 幂等合并导入（主键冲突覆盖），供迁移包导入端使用。
    pub fn merge_rows(&self, rows: &[HistoryExportRow]) -> Result<(), HistoryError> {
        let tx = self.conn.unchecked_transaction()?;
        for row in rows {
            tx.execute(
                "INSERT OR REPLACE INTO history
                    (provider_id, window_key, sampled_at, used, remaining, total, unit)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    row.provider_id,
                    row.window_key,
                    row.sampled_at as i64,
                    row.used,
                    row.remaining,
                    row.total,
                    row.unit,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn maybe_cleanup(&self, now_ms: u64) {
        if now_ms.saturating_sub(self.last_cleanup_ms.get()) < CLEANUP_INTERVAL_MS {
            return;
        }
        if self.cleanup(now_ms).is_ok() {
            self.last_cleanup_ms.set(now_ms);
        }
    }

    fn cleanup(&self, now_ms: u64) -> Result<usize, HistoryError> {
        let cutoff = now_ms.saturating_sub(DEFAULT_RETENTION_DAYS * 24 * 60 * 60 * 1000) as i64;
        let removed = self
            .conn
            .execute("DELETE FROM history WHERE sampled_at < ?1", params![cutoff])?;
        Ok(removed)
    }
}

/// 逐版本应用迁移；库版本比迁移表新（降级运行）时拒绝。
fn migrate(conn: &mut Connection) -> Result<(), HistoryError> {
    migrate_with(conn, MIGRATIONS)
}

fn migrate_with(conn: &mut Connection, migrations: &[&str]) -> Result<(), HistoryError> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(HistoryError::Open)?;
    let latest = migrations.len() as i64;
    // 负值只可能来自外部工具手动篡改 PRAGMA；as usize 会位回绕成巨大
    // 偏移导致静默跳过迁移，按版本异常拒绝（升级应用救不回负值）
    if version < 0 {
        return Err(HistoryError::AbnormalVersion { version });
    }
    if version > latest {
        return Err(HistoryError::NewerVersion { version });
    }
    for (idx, script) in migrations.iter().enumerate().skip(version as usize) {
        let target = (idx + 1) as u16;
        let tx = conn.transaction().map_err(|source| HistoryError::Migrate {
            version: target,
            source,
        })?;
        tx.execute_batch(script)
            .map_err(|source| HistoryError::Migrate {
                version: target,
                source,
            })?;
        tx.pragma_update(None, "user_version", target as i64)
            .map_err(|source| HistoryError::Migrate {
                version: target,
                source,
            })?;
        tx.commit().map_err(|source| HistoryError::Migrate {
            version: target,
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UsageData;

    /// 固定基准时刻，避免测试依赖真实时钟。
    const T0: u64 = 1_700_000_000_000;
    const DAY_MS: u64 = 24 * 60 * 60 * 1000;

    fn temp_db(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("quotatray-history-{tag}-{}.db", std::process::id()))
    }

    /// WAL 模式会产生伴生文件，一并清理。
    fn remove_db(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    fn usage(plan_name: Option<&str>, remaining: f64) -> UsageData {
        UsageData {
            plan_name: plan_name.map(str::to_string),
            remaining: Some(remaining),
            used: Some(100.0 - remaining),
            total: Some(100.0),
            unit: Some("USD".into()),
            ..Default::default()
        }
    }

    #[test]
    fn open_creates_db_at_version_1() {
        let path = temp_db("open-v1");
        remove_db(&path);
        {
            let store = HistoryStore::open(&path).unwrap();
            let version: i64 = store
                .conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, 1);
            // 表可用：直接走一次 record。
            store
                .record("p1", &[usage(Some("five_hour"), 42.0)], T0)
                .unwrap();
        }
        // Windows 上句柄存活时删除会失败，先出作用域再清理
        remove_db(&path);
    }

    #[test]
    fn reopen_keeps_data_and_stays_idempotent() {
        let path = temp_db("reopen");
        remove_db(&path);
        {
            let store = HistoryStore::open(&path).unwrap();
            store
                .record("p1", &[usage(Some("five_hour"), 42.0)], T0)
                .unwrap();
        }
        {
            let store = HistoryStore::open(&path).unwrap();
            let points = store.range("p1", 0).unwrap();
            assert_eq!(points.len(), 1);
            assert_eq!(points[0].remaining, Some(42.0));
        }
        remove_db(&path);
    }

    #[test]
    fn record_is_idempotent_on_same_millisecond() {
        let store = HistoryStore::open_in_memory().unwrap();
        store
            .record("p1", &[usage(Some("five_hour"), 1.0)], T0)
            .unwrap();
        store
            .record("p1", &[usage(Some("five_hour"), 2.0)], T0)
            .unwrap();
        let points = store.range("p1", 0).unwrap();
        assert_eq!(points.len(), 1, "同毫秒重放必须覆盖而非追加");
        assert_eq!(points[0].remaining, Some(2.0));
    }

    #[test]
    fn window_key_prefers_plan_name_and_falls_back_to_ordinal() {
        assert_eq!(window_key(&usage(Some("weekly"), 1.0), 0), "weekly");
        assert_eq!(window_key(&usage(Some("  "), 1.0), 3), "w3");
        assert_eq!(window_key(&usage(None, 1.0), 1), "w1");
    }

    #[test]
    fn multi_window_query_records_one_row_per_window() {
        let store = HistoryStore::open_in_memory().unwrap();
        store
            .record(
                "p1",
                &[usage(Some("five_hour"), 10.0), usage(Some("weekly"), 90.0)],
                T0,
            )
            .unwrap();
        let points = store.range("p1", 0).unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].window_key, "five_hour");
        assert_eq!(points[1].window_key, "weekly");
    }

    #[test]
    fn range_filters_by_time_and_sorts_ascending() {
        let store = HistoryStore::open_in_memory().unwrap();
        store
            .record("p1", &[usage(None, 1.0)], T0 + 20_000)
            .unwrap();
        store
            .record("p1", &[usage(None, 2.0)], T0 + 10_000)
            .unwrap();
        store
            .record("p1", &[usage(None, 3.0)], T0 + 30_000)
            .unwrap();
        store
            .record("other", &[usage(None, 4.0)], T0 + 15_000)
            .unwrap();

        let points = store.range("p1", T0 + 15_000).unwrap();
        assert_eq!(
            points
                .iter()
                .map(|point| point.remaining)
                .collect::<Vec<_>>(),
            vec![Some(1.0), Some(3.0)],
            "过滤下界含端点、升序，且不串条目"
        );
    }

    #[test]
    fn invalid_rows_are_skipped() {
        let store = HistoryStore::open_in_memory().unwrap();
        let mut invalid = usage(Some("five_hour"), 0.0);
        invalid.is_valid = Some(false);
        invalid.invalid_message = Some("key 已过期".into());
        store
            .record("p1", &[usage(Some("weekly"), 7.0), invalid], T0)
            .unwrap();

        let points = store.range("p1", 0).unwrap();
        assert_eq!(points.len(), 1, "is_valid=false 的行不得落库");
        assert_eq!(points[0].window_key, "weekly");

        // is_valid 未声明（None）视为有效照常记录
        store.record("p2", &[usage(None, 1.0)], T0).unwrap();
        assert_eq!(store.range("p2", 0).unwrap().len(), 1);
    }

    /// 契约：同次查询重复窗口键按出现顺序消歧（windowsFrom 数组展开
    /// 的同名多行 / script 重复 plan_name），不得静默互相覆盖。
    #[test]
    fn duplicate_window_keys_are_disambiguated() {
        let store = HistoryStore::open_in_memory().unwrap();
        // 模拟 windowsFrom 数组两个元素套用同一 WindowSpec：同名两行
        store
            .record(
                "p1",
                &[
                    usage(Some("Quota"), 30.0),
                    usage(Some("Quota"), 70.0),
                    usage(Some("weekly"), 90.0),
                ],
                T0,
            )
            .unwrap();
        let points = store.range("p1", 0).unwrap();
        let keys: Vec<(&str, Option<f64>)> = points
            .iter()
            .map(|p| (p.window_key.as_str(), p.remaining))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("Quota", Some(30.0)),
                ("Quota#2", Some(70.0)),
                ("weekly", Some(90.0))
            ],
            "重复键追加 #2 消歧，各行独立保留"
        );

        // 下次查询数组顺序稳定 → 键稳定，各自续线
        store
            .record(
                "p1",
                &[usage(Some("Quota"), 25.0), usage(Some("Quota"), 65.0)],
                T0 + 60_000,
            )
            .unwrap();
        assert_eq!(store.range("p1", 0).unwrap().len(), 5);
        // 无效行不参与消歧计数
        let mut invalid = usage(Some("Quota"), 0.0);
        invalid.is_valid = Some(false);
        store
            .record("p2", &[invalid, usage(Some("Quota"), 5.0)], T0)
            .unwrap();
        let p2 = store.range("p2", 0).unwrap();
        assert_eq!(p2.len(), 1);
        assert_eq!(p2[0].window_key, "Quota");
    }

    #[test]
    fn rolling_cleanup_deletes_expired_rows_with_throttle() {
        let store = HistoryStore::open_in_memory().unwrap();
        // 首次写入触发一次空清理，记录节流锚点。
        store.record("p1", &[usage(None, 1.0)], T0).unwrap();

        // 残留 31 天前的点：间隔不足 1h 的写入不应触发清理。
        insert_raw_row(&store, "stale-early", T0 + 1000 - 31 * DAY_MS);
        store.record("p2", &[usage(None, 2.0)], T0 + 1000).unwrap();
        assert_eq!(
            store.range("stale-early", 0).unwrap().len(),
            1,
            "清理节流窗口内的写入不应触发清理"
        );

        // 距上次清理超过 1h 的写入触发清理：过期点删除、新鲜点保留。
        insert_raw_row(
            &store,
            "stale-late",
            T0 + CLEANUP_INTERVAL_MS + 2000 - 31 * DAY_MS,
        );
        store
            .record("p3", &[usage(None, 3.0)], T0 + CLEANUP_INTERVAL_MS + 2000)
            .unwrap();
        assert!(store.range("stale-early", 0).unwrap().is_empty());
        assert!(store.range("stale-late", 0).unwrap().is_empty());
        assert_eq!(store.range("p1", 0).unwrap().len(), 1, "30 天内的点保留");
    }

    /// 绕过 record 直接注入行，模拟库里残留的历史数据。
    fn insert_raw_row(store: &HistoryStore, provider_id: &str, sampled_at: u64) {
        store
            .conn
            .execute(
                "INSERT INTO history (provider_id, window_key, sampled_at)
                 VALUES (?1, 'w0', ?2)",
                params![provider_id, sampled_at as i64],
            )
            .unwrap();
    }

    #[test]
    fn migrations_apply_in_order_and_failure_keeps_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_with(
            &mut conn,
            &[
                "CREATE TABLE t1 (a INTEGER);",
                "ALTER TABLE t1 ADD COLUMN b INTEGER;",
            ],
        )
        .unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);

        // 第二个迁移失败：版本号不落、已执行的 DDL 回滚。
        let mut conn = Connection::open_in_memory().unwrap();
        let result = migrate_with(
            &mut conn,
            &["CREATE TABLE t2 (a INTEGER);", "THIS IS NOT SQL;"],
        );
        assert!(matches!(
            result,
            Err(HistoryError::Migrate { version: 2, .. })
        ));
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1, "失败的迁移不得推进版本号");
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name='t2')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "v1 已提交的部分保留");
    }

    #[test]
    fn newer_db_version_is_rejected() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        assert!(matches!(
            migrate_with(&mut conn, MIGRATIONS),
            Err(HistoryError::NewerVersion { version: 99 })
        ));
        // 手动篡改的负版本同样拒绝（as usize 位回绕会静默跳过迁移），
        // 且升级应用救不回——单独文案引导删除重建
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", -1).unwrap();
        assert!(matches!(
            migrate_with(&mut conn, MIGRATIONS),
            Err(HistoryError::AbnormalVersion { version: -1 })
        ));
    }

    #[test]
    fn export_merge_roundtrip_across_stores_is_idempotent() {
        let source = HistoryStore::open_in_memory().unwrap();
        source
            .record(
                "p1",
                &[usage(Some("five_hour"), 10.0), usage(Some("weekly"), 90.0)],
                T0,
            )
            .unwrap();
        source
            .record("p1", &[usage(Some("five_hour"), 8.0)], T0 + 300_000)
            .unwrap();
        let rows = source.export_rows().unwrap();
        assert_eq!(rows.len(), 3);

        let target = HistoryStore::open_in_memory().unwrap();
        target.merge_rows(&rows).unwrap();
        // 幂等：重复合并不追加行。
        target.merge_rows(&rows).unwrap();
        assert_eq!(
            target.export_rows().unwrap(),
            rows,
            "合并按主键覆盖，重放结果应与源一致"
        );
    }

    #[test]
    fn clear_removes_single_entry_or_all() {
        let store = HistoryStore::open_in_memory().unwrap();
        store.record("a", &[usage(None, 1.0)], T0).unwrap();
        store.record("b", &[usage(None, 2.0)], T0).unwrap();

        store.clear(Some("a")).unwrap();
        assert!(store.range("a", 0).unwrap().is_empty());
        assert_eq!(store.range("b", 0).unwrap().len(), 1);

        store.clear(None).unwrap();
        assert!(store.range("b", 0).unwrap().is_empty());
    }

    #[test]
    fn open_rejects_non_sqlite_file() {
        let path = temp_db("garbage");
        remove_db(&path);
        std::fs::write(&path, b"this is not a sqlite database at all").unwrap();
        assert!(HistoryStore::open(&path).is_err());
        remove_db(&path);
    }
}
