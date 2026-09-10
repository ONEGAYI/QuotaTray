//! 目录同步与持久缓存（T-03）：装载有效目录、双通道获取、版本判定、
//! 跨进程写锁与缓存信封。
//!
//! 设计约束（spec §6）：
//! - 有效目录 = 通过完整校验的本地缓存与内置种子中 revision 较高者；
//!   无缓存/损坏/不兼容回退种子并区分原因。
//! - 远程包先完整校验（含物理删除对比），再整体切换；禁止半包拼接。
//! - 同版本同内容 = 无变化；同版本异内容 = 异常；旧版本忽略不降级；
//!   人工回滚以更高 revision 重发旧内容完成。
//! - 固定 HTTPS 源、真直连优先（直连获取或校验失败且配置了代理时经
//!   代理再试一次，代理结果为最终结果）；请求不携带供应商凭据。
//! - 数据根级跨进程写锁内重读最新缓存再比较 revision，防止 CLI 与
//!   GUI 互相覆盖新版本；网络期间不持锁；锁忙不强抢、不删他人锁文件。
//! - 写临时文件后替换；**替换成功才发布新内存快照**；写入失败沿用
//!   当前快照并如实报告失败。
//! - 时钟、数据根与网络依赖显式注入；本模块不做节流决策（调用方用
//!   [`crate::update::should_check_within`] 自行判定到期）。

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::http::{HttpClient, HttpRequest, HttpResponse};
use crate::pricing_catalog::{
    Catalog, CatalogError, bundled_catalog, parse_catalog, validate_no_removal,
};
use crate::update::write_atomic_bytes;

/// 固定分发源（仓库 main 的原始文件；spec §4.1，连通性由 T-07 验收）。
pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/ONEGAYI/QuotaTray/main/data/pricing/v1/catalog.json";

/// 数据根下的缓存文件名（完整信封）。
pub const CATALOG_CACHE_FILE: &str = "pricing-catalog.json";

/// 数据根下的跨进程写锁文件名（存在即忙碌；内容为持有者 pid）。
pub const CATALOG_LOCK_FILE: &str = "pricing-catalog.lock";

/// 单包接受上限（spec §6：完整接收后检查体积，非流式下载内存上限）。
pub const CATALOG_MAX_BYTES: usize = 2 * 1024 * 1024;

/// 缓存信封格式版本（与目录 `schema_version` 相互独立）。
const ENVELOPE_SCHEMA_VERSION: u32 = 1;

/// 自动检查成功间隔（spec §7：距上次成功检查 ≥6 小时到期）。
pub const AUTO_CHECK_INTERVAL_MS: u64 = 6 * 60 * 60 * 1000;

/// 自动检查失败退避（spec §7：失败后至少 30 分钟再自动尝试）。
pub const AUTO_CHECK_BACKOFF_MS: u64 = 30 * 60 * 1000;

/// 目录自动检查到期判定（纯函数，时钟注入）：开关关闭恒否；失败退避
/// （距最近一次尝试不足 30 分钟）优先拦截；到期 = 从未成功或距上次
/// 成功 ≥6 小时。时钟回退 saturating 归零（视为未到期，不 panic）。
pub fn catalog_should_auto_check(
    enabled: bool,
    last_attempt_ms: Option<u64>,
    last_success_ms: Option<u64>,
    now_ms: u64,
) -> bool {
    if !enabled {
        return false;
    }
    if let Some(attempt) = last_attempt_ms
        && now_ms.saturating_sub(attempt) < AUTO_CHECK_BACKOFF_MS
    {
        return false;
    }
    last_success_ms.is_none_or(|success| now_ms.saturating_sub(success) >= AUTO_CHECK_INTERVAL_MS)
}

// ---- 候选包判定（纯函数） -----------------------------------------------------

/// 候选包相对当前目录的判定结果。
#[derive(Debug, Clone, PartialEq)]
pub enum CatalogDecision {
    /// 接受：revision 更高（含人工回滚以更高 revision 重发旧内容）。
    Apply,
    /// 无变化：同版本同内容，或 revision 低于当前（旧包忽略不降级）。
    Unchanged { current: u64, incoming: u64 },
    /// 异常拒绝：同 revision 但内容不同。
    RejectSameRevisionDivergent { revision: u64 },
    /// 异常拒绝：物理删除了当前目录已有的模型（下架须 retired 化保留）。
    RejectRemoval { detail: String },
}

/// 已解析目录间的版本判定（`evaluate_incoming` 的纯比较半程）。
pub fn decide_between(current: &Catalog, incoming: &Catalog) -> CatalogDecision {
    match incoming.revision.cmp(&current.revision) {
        std::cmp::Ordering::Greater => CatalogDecision::Apply,
        std::cmp::Ordering::Less => CatalogDecision::Unchanged {
            current: current.revision,
            incoming: incoming.revision,
        },
        std::cmp::Ordering::Equal if incoming == current => CatalogDecision::Unchanged {
            current: current.revision,
            incoming: incoming.revision,
        },
        std::cmp::Ordering::Equal => CatalogDecision::RejectSameRevisionDivergent {
            revision: incoming.revision,
        },
    }
}

/// 解析并完整判定候选包 JSON：格式与数据校验 → 物理删除对比 → 版本
/// 比较。返回已解析目录与判定；任何拒绝都不触碰当前有效目录。
pub fn evaluate_incoming(
    current: &Catalog,
    incoming_json: &str,
) -> Result<(Catalog, CatalogDecision), CatalogError> {
    let incoming = parse_catalog(incoming_json)?;
    validate_no_removal(&incoming, current)?;
    let decision = decide_between(current, &incoming);
    Ok((incoming, decision))
}

// ---- 有效目录与回退原因 ------------------------------------------------------

/// 有效目录的数据载体。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogOrigin {
    /// 应用内置种子（随版本构建嵌入）。
    Bundled,
    /// 本地缓存（远程成功下载并校验后落盘的完整目录）。
    Cached,
}

/// 未采用缓存、使用内置种子的原因（`origin=bundled` 时才有值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    /// 数据根下没有缓存文件（初装离线）。
    NoCache,
    /// 缓存信封不是合法 JSON（损坏）。
    CorruptedCache,
    /// 缓存目录未通过校验（格式不兼容或数据非法）。
    IncompatibleCache,
    /// 缓存 revision 不高于内置种子（应用升级带来相等或更新的种子，
    /// 修订权在应用发布侧）。
    StaleCache,
}

/// 当前生效的完整目录及其来源（内存快照的不可变值）。
#[derive(Debug, Clone)]
pub struct EffectiveCatalog {
    pub catalog: Catalog,
    pub origin: CatalogOrigin,
    /// 回退到种子的原因（`origin=cached` 时为 None）。
    pub fallback_reason: Option<FallbackReason>,
}

// ---- 缓存信封 ----------------------------------------------------------------

/// 缓存文件结构：完整目录 + 同步元数据一体提交（spec §6：避免包和
/// 版本元数据分开写入）。不含用户条目、自定义价格或凭据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogCacheEnvelope {
    pub schema_version: u32,
    pub catalog: Catalog,
    pub last_attempt_ms: Option<u64>,
    pub last_success_ms: Option<u64>,
    /// 最近一次尝试的错误摘要（成功后清除）。
    pub last_error: Option<String>,
}

// ---- 同步错误与结果 ----------------------------------------------------------

/// 同步失败的错误分类（展示层据此区分「数据源问题」与「本机问题」）。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CatalogSyncError {
    #[error("网络请求失败：{0}")]
    Network(String),
    #[error("响应不可用：{0}")]
    BadResponse(String),
    #[error("候选包被拒绝：{0}")]
    Rejected(String),
    #[error("缓存写入失败：{0}")]
    Io(String),
}

/// 单次更新的结果。`Updated` 是应用端发「目录变更事件」的唯一依据
/// （重复相同包返回 Unchanged，不触发变更信号）。
#[derive(Debug, Clone, PartialEq)]
pub enum CatalogUpdateOutcome {
    /// 已接受并发布新快照（`catalog` 为 applied 后的有效目录）。
    Updated { catalog: Catalog },
    /// 无变化（同版本同内容或旧包；当前目录不受影响）。
    Unchanged { revision: u64 },
    /// 进程内已有一次更新在途，或跨进程写锁被其他进程持有。
    Busy,
    /// 失败（错误分类；当前有效目录保持可用）。
    Failed(CatalogSyncError),
}

/// 同步状态快照（应用端展示与节流决策消费）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CatalogStatusView {
    pub revision: u64,
    pub origin: CatalogOrigin,
    pub fallback_reason: Option<FallbackReason>,
    pub last_attempt_ms: Option<u64>,
    pub last_success_ms: Option<u64>,
    pub last_error: Option<String>,
}

// ---- 装载（缓存 vs 种子） -----------------------------------------------------

/// 从缓存信封 JSON 构造有效目录：解析与校验失败回退种子并给出原因；
/// 成功时与种子取 revision 较高者（应用升级可能带来更新的内置种子）。
pub fn effective_from_envelope_json(envelope_json: &str) -> EffectiveCatalog {
    let envelope: CatalogCacheEnvelope = match serde_json::from_str(envelope_json) {
        Ok(e) => e,
        Err(_) => return bundled_effective(FallbackReason::CorruptedCache),
    };
    if envelope.schema_version != ENVELOPE_SCHEMA_VERSION
        || crate::pricing_catalog::validate_catalog(&envelope.catalog).is_err()
    {
        return bundled_effective(FallbackReason::IncompatibleCache);
    }
    let bundled = bundled_catalog();
    if envelope.catalog.revision > bundled.revision {
        EffectiveCatalog {
            catalog: envelope.catalog,
            origin: CatalogOrigin::Cached,
            fallback_reason: None,
        }
    } else {
        bundled_effective(FallbackReason::StaleCache)
    }
}

/// 启动装载：读缓存文件（不存在 = 初装离线）并选出有效目录。
pub fn load_effective(data_root: &Path) -> EffectiveCatalog {
    let cache = data_root.join(CATALOG_CACHE_FILE);
    match std::fs::read_to_string(&cache) {
        Ok(text) => effective_from_envelope_json(&text),
        Err(_) => bundled_effective(FallbackReason::NoCache),
    }
}

fn bundled_effective(reason: FallbackReason) -> EffectiveCatalog {
    EffectiveCatalog {
        catalog: bundled_catalog().clone(),
        origin: CatalogOrigin::Bundled,
        fallback_reason: Some(reason),
    }
}

// ---- 同步服务 -----------------------------------------------------------------

/// 目录同步服务：持有有效目录快照与同步元数据；`update` 可重入，
/// 进程内互斥 + 数据根级跨进程锁双层防并发。
pub struct CatalogSync {
    data_root: PathBuf,
    direct: Box<dyn HttpClient>,
    proxied: Option<Box<dyn HttpClient>>,
    now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    /// 进程内在途标志（原子量 + RAII 复位；无阻塞，可安全跨 await 持有）。
    in_flight: std::sync::atomic::AtomicBool,
    /// 有效快照与元数据（写锁短持有）。
    shared: RwLock<SharedState>,
}

#[derive(Clone)]
struct SharedState {
    effective: EffectiveCatalog,
    last_attempt_ms: Option<u64>,
    last_success_ms: Option<u64>,
    last_error: Option<String>,
}

impl CatalogSync {
    /// 构造并装载初始有效目录（缓存 vs 种子）。
    pub fn new(
        data_root: impl Into<PathBuf>,
        direct: Box<dyn HttpClient>,
        proxied: Option<Box<dyn HttpClient>>,
        now_ms: Box<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        let data_root = data_root.into();
        let effective = load_effective(&data_root);
        Self {
            data_root,
            direct,
            proxied,
            now_ms,
            in_flight: std::sync::atomic::AtomicBool::new(false),
            shared: RwLock::new(SharedState {
                effective,
                last_attempt_ms: None,
                last_success_ms: None,
                last_error: None,
            }),
        }
    }

    /// 当前有效目录（克隆快照；价格解析经
    /// [`crate::pricing::resolve_in_catalog`] 消费）。
    pub fn effective(&self) -> EffectiveCatalog {
        self.shared.read().unwrap().effective.clone()
    }

    /// 同步状态快照。
    pub fn status(&self) -> CatalogStatusView {
        let s = self.shared.read().unwrap();
        CatalogStatusView {
            revision: s.effective.catalog.revision,
            origin: s.effective.origin,
            fallback_reason: s.effective.fallback_reason.clone(),
            last_attempt_ms: s.last_attempt_ms,
            last_success_ms: s.last_success_ms,
            last_error: s.last_error.clone(),
        }
    }

    /// 本地缓存重载：其他进程（如 CLI）可能已写入更高版本，磁盘 revision
    /// 高于内存时重新装载（本地读取，不产生网络请求；不降级）。
    /// 返回 true 表示发生了升级（应用端据此广播目录变更）。
    pub fn reload_from_disk_if_newer(&self) -> bool {
        let on_disk = load_effective(&self.data_root);
        let mut s = self.shared.write().unwrap();
        if on_disk.catalog.revision > s.effective.catalog.revision {
            s.effective = on_disk;
            true
        } else {
            false
        }
    }

    /// 执行一次更新（到期判定由调用方负责；手动更新直接调用）。
    /// 网络期间不持锁；锁内重读磁盘缓存再比较，防止跨进程互相覆盖。
    pub async fn update(&self) -> CatalogUpdateOutcome {
        // 进程内在途互斥：忙碌即 Busy（自动场景跳过、手动场景返回 busy）
        if self
            .in_flight
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return CatalogUpdateOutcome::Busy;
        }
        let _in_flight_guard = InFlightGuard(&self.in_flight);

        // 双通道获取 + 校验：直连任何失败（网络/非 200/包不合格）且配置了
        // 代理时经代理再试一次，代理结果为最终结果（与安装包检测同款；
        // 拒绝类也走兜底——直连被劫持时可能返回被篡改的坏包）
        let fetched = match self.fetch_and_evaluate(self.direct.as_ref()).await {
            Ok(done) => Ok(done),
            Err(direct_err) => match self.proxied.as_ref() {
                Some(http) => self.fetch_and_evaluate(http.as_ref()).await,
                None => Err(direct_err),
            },
        };

        let now = (self.now_ms)();
        let incoming = match fetched {
            Ok((incoming, CatalogDecision::Apply)) => incoming,
            Ok((_, CatalogDecision::Unchanged { .. })) => {
                self.note_attempt(now, None, true);
                return CatalogUpdateOutcome::Unchanged {
                    revision: self.effective().catalog.revision,
                };
            }
            Ok((_, reject)) => {
                let msg = reject_message(&reject);
                self.note_attempt(now, Some(&msg), true);
                return CatalogUpdateOutcome::Failed(CatalogSyncError::Rejected(msg));
            }
            Err(e) => {
                let msg = e.to_string();
                self.note_attempt(now, Some(&msg), true);
                return CatalogUpdateOutcome::Failed(e);
            }
        };

        // 跨进程写锁：存在即忙碌，不删他人锁文件强抢
        let lock_path = self.data_root.join(CATALOG_LOCK_FILE);
        let _lock = match FileLock::acquire(&lock_path) {
            Some(lock) => lock,
            None => {
                // 锁忙：只更新内存元数据（不写盘，避免互踩）
                self.note_attempt(now, Some("跨进程写锁被其他进程持有"), false);
                return CatalogUpdateOutcome::Busy;
            }
        };

        // 锁内重读磁盘信封：另一进程可能已写入更高版本（比内存新即先升级
        // 基准），再与候选包比较，防止旧包覆盖他进程的新版本
        let mut baseline = self.effective();
        let disk = load_effective(&self.data_root);
        if disk.catalog.revision > baseline.catalog.revision {
            baseline = disk;
        }
        match decide_between(&baseline.catalog, &incoming) {
            CatalogDecision::Apply => {
                let envelope = CatalogCacheEnvelope {
                    schema_version: ENVELOPE_SCHEMA_VERSION,
                    catalog: incoming.clone(),
                    last_attempt_ms: Some(now),
                    last_success_ms: Some(now),
                    last_error: None,
                };
                let json = serde_json::to_string(&envelope).expect("信封序列化不失败");
                let cache_path = self.data_root.join(CATALOG_CACHE_FILE);
                if let Err(e) = write_atomic_bytes(&cache_path, json.as_bytes()) {
                    // 写入失败：沿用当前内存快照，如实报告失败
                    let msg = format!("缓存写入失败：{e}");
                    self.note_attempt(now, Some(&msg), false);
                    return CatalogUpdateOutcome::Failed(CatalogSyncError::Io(msg));
                }
                // 替换成功才发布新内存快照（spec §6 步骤 6）
                let applied = EffectiveCatalog {
                    catalog: incoming,
                    origin: CatalogOrigin::Cached,
                    fallback_reason: None,
                };
                {
                    let mut s = self.shared.write().unwrap();
                    s.effective = applied;
                    s.last_attempt_ms = Some(now);
                    s.last_success_ms = Some(now);
                    s.last_error = None;
                }
                CatalogUpdateOutcome::Updated {
                    catalog: self.effective().catalog,
                }
            }
            // 锁内重读后基线不低于候选（他进程已更新）：作无变化处理
            _ => {
                self.note_attempt(now, None, true);
                CatalogUpdateOutcome::Unchanged {
                    revision: baseline.catalog.revision,
                }
            }
        }
    }

    /// 获取并判定（单通道半程）：网络 → 状态/体积 → 解析校验 → 版本判定。
    /// 返回 Err 表示该通道失败（调用方决定是否走代理）。
    async fn fetch_and_evaluate(
        &self,
        client: &dyn HttpClient,
    ) -> Result<(Catalog, CatalogDecision), CatalogSyncError> {
        let resp: HttpResponse = client
            .execute(HttpRequest::get(CATALOG_URL))
            .await
            .map_err(|e| CatalogSyncError::Network(e.to_string()))?;
        if resp.status != 200 {
            return Err(CatalogSyncError::BadResponse(format!(
                "HTTP {}",
                resp.status
            )));
        }
        if resp.body.len() > CATALOG_MAX_BYTES {
            return Err(CatalogSyncError::BadResponse(format!(
                "包体积 {} 超过上限 {} 字节",
                resp.body.len(),
                CATALOG_MAX_BYTES
            )));
        }
        let current = self.effective();
        let (incoming, decision) =
            evaluate_incoming(&current.catalog, &resp.body).map_err(|e| match e {
                CatalogError::Validation { field, reason } => {
                    CatalogSyncError::Rejected(format!("{field}：{reason}"))
                }
                CatalogError::Parse(e) => CatalogSyncError::BadResponse(e),
            })?;
        match decision {
            // 拒绝类以 Err 交给上层走代理兜底（直连劫持可能给出被篡改的包）
            reject @ (CatalogDecision::RejectSameRevisionDivergent { .. }
            | CatalogDecision::RejectRemoval { .. }) => {
                Err(CatalogSyncError::Rejected(reject_message(&reject)))
            }
            other => Ok((incoming, other)),
        }
    }

    /// 更新内存元数据；`persist` 时把（目录不变、元数据更新）的信封落盘，
    /// 落盘失败仅保留内存侧记录（目录数据不受影响）。
    fn note_attempt(&self, now_ms: u64, error: Option<&str>, persist: bool) {
        let snapshot = {
            let mut s = self.shared.write().unwrap();
            s.last_attempt_ms = Some(now_ms);
            s.last_error = error.map(str::to_string);
            if error.is_none() {
                s.last_success_ms = Some(now_ms);
            }
            s.clone()
        };
        if persist {
            let envelope = CatalogCacheEnvelope {
                schema_version: ENVELOPE_SCHEMA_VERSION,
                catalog: snapshot.effective.catalog,
                last_attempt_ms: snapshot.last_attempt_ms,
                last_success_ms: snapshot.last_success_ms,
                last_error: snapshot.last_error,
            };
            if let Ok(json) = serde_json::to_string(&envelope) {
                // 元数据写入失败不影响结果分类（磁盘留着旧信封，下次覆盖）
                let _ =
                    write_atomic_bytes(&self.data_root.join(CATALOG_CACHE_FILE), json.as_bytes());
            }
        }
    }
}

/// 拒绝类判定的错误文案。
fn reject_message(decision: &CatalogDecision) -> String {
    match decision {
        CatalogDecision::RejectSameRevisionDivergent { revision } => {
            format!("revision {revision} 与当前目录内容不一致（同版本异内容）")
        }
        CatalogDecision::RejectRemoval { detail } => {
            format!("候选包物理删除已知模型：{detail}")
        }
        other => unreachable!("reject_message 仅接受拒绝类，收到 {other:?}"),
    }
}

/// 在途标志的 RAII 复位 guard（仅持有原子引用，await 安全）。
struct InFlightGuard<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

// ---- 跨进程写锁 --------------------------------------------------------------

/// 数据根级跨进程写锁：`create_new` 语义（存在即失败）；Guard 释放时
/// 删除锁文件。不删除他人锁文件；陈旧锁（持有进程崩溃残留）首期同样
/// 视为忙碌——不引入基于时间的强抢（spec §6）。
struct FileLock {
    path: PathBuf,
}

impl FileLock {
    fn acquire(path: &Path) -> Option<Self> {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                use std::io::Write;
                let _ = writeln!(file, "pid={}", std::process::id());
                Some(FileLock {
                    path: path.to_path_buf(),
                })
            }
            Err(_) => None,
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

// ---- 测试（临时数据根 + mock HTTP；不碰生产凭据与真实网络） -------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::PriceTier;
    use crate::provider::testing::{MockHttp, MockResp};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 唯一临时目录（每测试独立数据根，结束清理）。
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "quotatray-catalog-{}-{}-{}",
                tag,
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 固定时钟（epoch ms，2026-09 中旬）。
    const NOW: u64 = 1_789_000_000_000;

    fn clock() -> Box<dyn Fn() -> u64 + Send + Sync> {
        Box::new(|| NOW)
    }

    /// 基于内置种子的合法包（改 revision；内容等价 = 「更高 revision 携带
    /// 旧价格」的人工回滚形状）。
    fn pkg(revision: u64) -> String {
        let mut cat = bundled_catalog().clone();
        cat.revision = revision;
        serde_json::to_string(&cat).unwrap()
    }

    /// 包内容变异（同 revision 异内容 / 负价 / 删模型等坏包基座）。
    fn pkg_mut(revision: u64, f: impl FnOnce(&mut Catalog)) -> String {
        let mut cat = bundled_catalog().clone();
        cat.revision = revision;
        f(&mut cat);
        serde_json::to_string(&cat).unwrap()
    }

    fn sync_with(dir: &TempDir, direct: MockHttp) -> CatalogSync {
        CatalogSync::new(&dir.0, Box::new(direct), None, clock())
    }

    fn write_envelope(dir: &TempDir, envelope: &CatalogCacheEnvelope) {
        let json = serde_json::to_string(envelope).unwrap();
        write_atomic_bytes(&dir.0.join(CATALOG_CACHE_FILE), json.as_bytes()).unwrap();
    }

    fn envelope_on_disk(dir: &TempDir) -> CatalogCacheEnvelope {
        let text = std::fs::read_to_string(dir.0.join(CATALOG_CACHE_FILE)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    // ---- V-04 装载：缓存 vs 种子 ----

    /// 契约：初装离线（无缓存）→ 内置种子，回退原因 NoCache。
    #[test]
    fn load_without_cache_uses_bundled() {
        let dir = TempDir::new("v04a");
        let e = load_effective(&dir.0);
        assert_eq!(e.origin, CatalogOrigin::Bundled);
        assert_eq!(e.fallback_reason, Some(FallbackReason::NoCache));
        assert_eq!(e.catalog.revision, bundled_catalog().revision);
    }

    /// 契约：有效且更高的缓存 → 采用缓存（origin=cached）。
    #[test]
    fn load_with_newer_cache_uses_cached() {
        let dir = TempDir::new("v04b");
        write_envelope(
            &dir,
            &CatalogCacheEnvelope {
                schema_version: 1,
                catalog: serde_json::from_str(&pkg(7)).unwrap(),
                last_attempt_ms: Some(NOW),
                last_success_ms: Some(NOW),
                last_error: None,
            },
        );
        let e = load_effective(&dir.0);
        assert_eq!(e.origin, CatalogOrigin::Cached);
        assert_eq!(e.fallback_reason, None);
        assert_eq!(e.catalog.revision, 7);
    }

    /// 契约：损坏缓存（垃圾字节）与不兼容缓存（信封版本错/目录校验失败）
    /// 均回退种子，原因分别为 CorruptedCache / IncompatibleCache。
    #[test]
    fn load_with_corrupted_or_incompatible_cache_falls_back() {
        let dir = TempDir::new("v04c");
        write_atomic_bytes(&dir.0.join(CATALOG_CACHE_FILE), b"{ not json").unwrap();
        let e = load_effective(&dir.0);
        assert_eq!(e.fallback_reason, Some(FallbackReason::CorruptedCache));

        let dir = TempDir::new("v04d");
        write_envelope(
            &dir,
            &CatalogCacheEnvelope {
                schema_version: 99,
                catalog: serde_json::from_str(&pkg(7)).unwrap(),
                last_attempt_ms: None,
                last_success_ms: None,
                last_error: None,
            },
        );
        assert_eq!(
            load_effective(&dir.0).fallback_reason,
            Some(FallbackReason::IncompatibleCache)
        );

        // 目录数据非法（负价）同样不兼容
        let dir = TempDir::new("v04e");
        let bad = pkg_mut(7, |cat| {
            cat.providers[0].suites[0].models[0].peak = Some(PriceTier {
                output: Some(-1.0),
                ..Default::default()
            });
        });
        write_envelope(
            &dir,
            &CatalogCacheEnvelope {
                schema_version: 1,
                catalog: serde_json::from_str(&bad).unwrap(),
                last_attempt_ms: None,
                last_success_ms: None,
                last_error: None,
            },
        );
        assert_eq!(
            load_effective(&dir.0).fallback_reason,
            Some(FallbackReason::IncompatibleCache)
        );
    }

    /// 契约：缓存 revision 不高于内置种子（应用升级带来相等/更新种子）
    /// → 采用种子，原因 StaleCache。
    #[test]
    fn load_with_stale_cache_prefers_bundled() {
        let dir = TempDir::new("v04f");
        write_envelope(
            &dir,
            &CatalogCacheEnvelope {
                schema_version: 1,
                catalog: bundled_catalog().clone(),
                last_attempt_ms: None,
                last_success_ms: None,
                last_error: None,
            },
        );
        let e = load_effective(&dir.0);
        assert_eq!(e.origin, CatalogOrigin::Bundled);
        assert_eq!(e.fallback_reason, Some(FallbackReason::StaleCache));
    }

    // ---- V-05 坏包不覆盖有效目录 ----

    /// 契约：404 / HTML / 非法 JSON / 负价 / 同版本异内容 / 物理删除
    /// 均失败收场，磁盘目录数据不被替换。
    #[tokio::test]
    async fn update_rejects_bad_packages_without_overwriting() {
        // 404（元数据信封允许落盘，但目录数据不得被写入）
        let dir = TempDir::new("v05a");
        let s = sync_with(&dir, MockHttp::seq(&[(404, "")]));
        assert!(matches!(s.update().await, CatalogUpdateOutcome::Failed(
            CatalogSyncError::BadResponse(m)) if m.contains("404")));
        let on_disk = std::fs::read_to_string(dir.0.join(CATALOG_CACHE_FILE)).unwrap_or_default();
        if !on_disk.is_empty() {
            let env: CatalogCacheEnvelope = serde_json::from_str(&on_disk).unwrap();
            assert_eq!(env.catalog, *bundled_catalog(), "坏包目录不得落盘");
        }

        // HTML（伪装 200）
        let dir = TempDir::new("v05b");
        let s = sync_with(&dir, MockHttp::ok("<html>hi</html>"));
        assert!(matches!(
            s.update().await,
            CatalogUpdateOutcome::Failed(CatalogSyncError::BadResponse(_))
        ));

        // 负价（数据校验拒绝）
        let dir = TempDir::new("v05c");
        let bad = pkg_mut(2, |cat| {
            cat.providers[0].suites[0].models[0].peak = Some(PriceTier {
                output: Some(-1.0),
                ..Default::default()
            });
        });
        let s = sync_with(&dir, MockHttp::ok(&bad));
        assert!(matches!(s.update().await, CatalogUpdateOutcome::Failed(
            CatalogSyncError::Rejected(m)) if m.contains("价格")));

        // 同版本异内容（bundled=rev1；同 rev 改价）
        let dir = TempDir::new("v05d");
        let divergent = pkg_mut(1, |cat| {
            cat.providers[0].suites[0].models[0].peak = Some(PriceTier::full(9.9, 9.9, 9.9));
        });
        let s = sync_with(&dir, MockHttp::ok(&divergent));
        assert!(matches!(s.update().await, CatalogUpdateOutcome::Failed(
            CatalogSyncError::Rejected(m)) if m.contains("同版本异内容")));

        // 物理删除（删 deepseek flash）
        let dir = TempDir::new("v05e");
        let removal = pkg_mut(2, |cat| {
            cat.providers[0].suites[0]
                .models
                .retain(|m| m.id != "flash");
        });
        let s = sync_with(&dir, MockHttp::ok(&removal));
        assert!(matches!(s.update().await, CatalogUpdateOutcome::Failed(
            CatalogSyncError::Rejected(m)) if m.contains("flash")));
        // 物理删除场景的磁盘目录数据仍为种子等价（坏包未替换任何内容）
        let env = envelope_on_disk(&dir);
        assert_eq!(env.catalog, *bundled_catalog(), "坏包目录不得替换磁盘数据");
    }

    // ---- V-06 版本单调 / 并发 / 写入 ----

    /// 契约：先 rev3 后 rev2（先发后到）→ revision 不倒退；重复同包 →
    /// Unchanged（不发变更信号）；人工回滚（更高 rev 携带旧价）→ Apply。
    #[tokio::test]
    async fn update_is_monotonic_and_idempotent() {
        let dir = TempDir::new("v06a");
        let s = sync_with(
            &dir,
            MockHttp::seq(&[
                (200, &pkg(3)),
                (200, &pkg(2)),
                (200, &pkg(3)),
                (200, &pkg(5)),
            ]),
        );
        match s.update().await {
            CatalogUpdateOutcome::Updated { catalog } => assert_eq!(catalog.revision, 3),
            other => panic!("rev3 应 Updated：{other:?}"),
        }
        match s.update().await {
            CatalogUpdateOutcome::Unchanged { revision } => assert_eq!(revision, 3),
            other => panic!("旧包应 Unchanged 不降级：{other:?}"),
        }
        assert_eq!(s.effective().catalog.revision, 3, "不倒退");
        assert!(
            matches!(s.update().await, CatalogUpdateOutcome::Unchanged { .. }),
            "重复同包 Unchanged"
        );
        // 人工回滚：更高 revision 携带旧价格内容 → 接受
        match s.update().await {
            CatalogUpdateOutcome::Updated { catalog } => assert_eq!(catalog.revision, 5),
            other => panic!("人工回滚应 Updated：{other:?}"),
        }
        assert_eq!(
            envelope_on_disk(&dir).catalog.revision,
            5,
            "磁盘同步到 rev5"
        );
    }

    /// 契约：进程内已有更新在途 → Busy（自动场景跳过、手动返回 busy）。
    #[tokio::test]
    async fn update_returns_busy_when_in_flight() {
        let dir = TempDir::new("v06b");
        let s = sync_with(&dir, MockHttp::ok(&pkg(2)));
        s.in_flight
            .store(true, std::sync::atomic::Ordering::Relaxed); // 模拟在途
        assert_eq!(s.update().await, CatalogUpdateOutcome::Busy);
        s.in_flight
            .store(false, std::sync::atomic::Ordering::Release);
        assert!(matches!(
            s.update().await,
            CatalogUpdateOutcome::Updated { .. }
        ));
    }

    /// 契约：跨进程锁被他人持有时 → Busy，且不删除他人锁文件。
    #[tokio::test]
    async fn update_returns_busy_when_cross_process_lock_held() {
        let dir = TempDir::new("v06c");
        let s = sync_with(&dir, MockHttp::ok(&pkg(2)));
        let lock = dir.0.join(CATALOG_LOCK_FILE);
        std::fs::write(&lock, b"pid=99999").unwrap(); // 他人锁
        assert_eq!(s.update().await, CatalogUpdateOutcome::Busy);
        assert!(lock.exists(), "不删他人锁文件");
        std::fs::remove_file(&lock).unwrap();
        assert!(matches!(
            s.update().await,
            CatalogUpdateOutcome::Updated { .. }
        ));
    }

    /// 契约：锁内重读磁盘——另一进程已写入 rev5，候选 rev3 不覆盖新版本。
    #[tokio::test]
    async fn update_reloads_disk_inside_lock_and_skips_stale_candidate() {
        let dir = TempDir::new("v06d");
        write_envelope(
            &dir,
            &CatalogCacheEnvelope {
                schema_version: 1,
                catalog: serde_json::from_str(&pkg(5)).unwrap(),
                last_attempt_ms: None,
                last_success_ms: None,
                last_error: None,
            },
        );
        let s = CatalogSync::new(&dir.0, Box::new(MockHttp::ok(&pkg(3))), None, clock());
        match s.update().await {
            CatalogUpdateOutcome::Unchanged { revision } => assert_eq!(revision, 5),
            other => panic!("不应覆盖他进程新版本：{other:?}"),
        }
        assert_eq!(envelope_on_disk(&dir).catalog.revision, 5, "磁盘保持 rev5");
    }

    /// 契约：缓存写入失败（路径被目录占用）→ Failed(Io)，内存快照仍可用。
    #[tokio::test]
    async fn update_write_failure_keeps_memory_snapshot() {
        let dir = TempDir::new("v06e");
        let s = sync_with(&dir, MockHttp::ok(&pkg(2)));
        // 缓存路径占位为目录：tmp 写入成功但 rename 失败（Windows 实测路径）
        std::fs::create_dir_all(dir.0.join(CATALOG_CACHE_FILE)).unwrap();
        match s.update().await {
            CatalogUpdateOutcome::Failed(CatalogSyncError::Io(m)) => {
                assert!(m.contains("写入失败"), "{m}")
            }
            other => panic!("写入失败应 Io：{other:?}"),
        }
        let e = s.effective();
        assert_eq!(
            e.catalog.revision,
            bundled_catalog().revision,
            "内存沿用种子"
        );
        assert!(s.status().last_error.is_some());
    }

    // ---- V-07 双通道 ----

    /// 契约：直连成功 → 零代理请求。
    #[tokio::test]
    async fn direct_success_makes_no_proxy_request() {
        let dir = TempDir::new("v07a");
        let proxy = MockHttp::ok(&pkg(9));
        let s = CatalogSync::new(
            &dir.0,
            Box::new(MockHttp::ok(&pkg(2))),
            Some(Box::new(proxy.clone())),
            clock(),
        );
        assert!(matches!(
            s.update().await,
            CatalogUpdateOutcome::Updated { .. }
        ));
        assert!(
            proxy.captured_requests().is_empty(),
            "直连成功不得发代理请求"
        );
    }

    /// 契约：直连失败且配置了代理 → 经代理重试，代理结果为最终结果。
    #[tokio::test]
    async fn direct_failure_falls_back_to_proxy() {
        let dir = TempDir::new("v07b");
        let s = CatalogSync::new(
            &dir.0,
            Box::new(MockHttp::seq_of(&[MockResp::Fail])),
            Some(Box::new(MockHttp::ok(&pkg(2)))),
            clock(),
        );
        match s.update().await {
            CatalogUpdateOutcome::Updated { catalog } => assert_eq!(catalog.revision, 2),
            other => panic!("代理兜底应成功：{other:?}"),
        }
    }

    /// 契约：直连返回坏包（劫持形状：HTML）→ 同样走代理兜底。
    #[tokio::test]
    async fn direct_bad_package_falls_back_to_proxy() {
        let dir = TempDir::new("v07c");
        let s = CatalogSync::new(
            &dir.0,
            Box::new(MockHttp::ok("<hijacked>")),
            Some(Box::new(MockHttp::ok(&pkg(2)))),
            clock(),
        );
        assert!(matches!(
            s.update().await,
            CatalogUpdateOutcome::Updated { .. }
        ));
    }

    /// 契约：双通道皆失败 → Failed，当前有效目录保持可用。
    #[tokio::test]
    async fn both_channels_fail_keeps_old_data() {
        let dir = TempDir::new("v07d");
        // 先成功一次落盘 rev2，再双失败
        let s = CatalogSync::new(
            &dir.0,
            Box::new(MockHttp::seq(&[(200, &pkg(2))])),
            None,
            clock(),
        );
        assert!(matches!(
            s.update().await,
            CatalogUpdateOutcome::Updated { .. }
        ));
        let s2 = CatalogSync::new(
            &dir.0,
            Box::new(MockHttp::seq_of(&[MockResp::Fail])),
            Some(Box::new(MockHttp::seq_of(&[MockResp::Fail]))),
            clock(),
        );
        match s2.update().await {
            CatalogUpdateOutcome::Failed(CatalogSyncError::Network(_)) => {}
            other => panic!("双失败应 Network：{other:?}"),
        }
        assert_eq!(s2.effective().catalog.revision, 2, "缓存数据仍可读");
        assert_eq!(s2.status().origin, CatalogOrigin::Cached);
    }

    // ---- 状态 / 信封 / 本地重载 ----

    /// 契约：成功更新后 status 透出 revision/cached/attempt/success 且
    /// last_error 清空；失败后 last_error 携带摘要、有效目录不变。
    #[tokio::test]
    async fn status_tracks_attempt_success_and_error() {
        let dir = TempDir::new("st");
        let s = CatalogSync::new(
            &dir.0,
            Box::new(MockHttp::seq(&[(200, &pkg(2)), (404, "")])),
            None,
            clock(),
        );
        assert!(matches!(
            s.update().await,
            CatalogUpdateOutcome::Updated { .. }
        ));
        let st = s.status();
        assert_eq!(st.revision, 2);
        assert_eq!(st.origin, CatalogOrigin::Cached);
        assert_eq!(st.last_success_ms, Some(NOW));
        assert_eq!(st.last_error, None);
        assert!(matches!(s.update().await, CatalogUpdateOutcome::Failed(_)));
        let st = s.status();
        assert_eq!(st.revision, 2, "失败不改变有效目录");
        assert!(st.last_error.as_deref().is_some_and(|e| e.contains("404")));
    }

    /// 契约：缓存信封只含目录与同步元数据，不携带用户条目/凭据形态字段。
    #[test]
    fn envelope_contains_only_catalog_and_sync_metadata() {
        let envelope = CatalogCacheEnvelope {
            schema_version: 1,
            catalog: bundled_catalog().clone(),
            last_attempt_ms: Some(NOW),
            last_success_ms: Some(NOW),
            last_error: None,
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "catalog",
                "last_attempt_ms",
                "last_error",
                "last_success_ms",
                "schema_version"
            ]
        );
        let text = serde_json::to_string(&envelope).unwrap();
        for banned in ["api_key", "custom_models"] {
            assert!(!text.contains(banned), "信封不应含 {banned}");
        }
    }

    /// 契约（V-09）：自动检查到期判定矩阵——开关关闭恒否；从未检查到期；
    /// 成功 6h 间隔；失败 30min 退避优先于到期；时钟回退不 panic 视为未到期。
    #[test]
    fn catalog_should_auto_check_matrix() {
        use super::{AUTO_CHECK_BACKOFF_MS, AUTO_CHECK_INTERVAL_MS, catalog_should_auto_check};
        let now = 1_789_000_000_000u64;
        assert!(!catalog_should_auto_check(false, None, None, now));
        assert!(catalog_should_auto_check(true, None, None, now));
        assert!(!catalog_should_auto_check(
            true,
            Some(now - AUTO_CHECK_INTERVAL_MS + 1),
            Some(now - AUTO_CHECK_INTERVAL_MS + 1),
            now
        ));
        assert!(catalog_should_auto_check(
            true,
            Some(now - AUTO_CHECK_INTERVAL_MS),
            Some(now - AUTO_CHECK_INTERVAL_MS),
            now
        ));
        // 失败退避：距最近尝试不足 30min → 否（即使 6h 已满）
        assert!(!catalog_should_auto_check(
            true,
            Some(now - AUTO_CHECK_BACKOFF_MS + 1),
            Some(now - AUTO_CHECK_INTERVAL_MS),
            now
        ));
        // 退避期满 + 6h 已满 → 到期（失败重试）
        assert!(catalog_should_auto_check(
            true,
            Some(now - AUTO_CHECK_BACKOFF_MS),
            Some(now - AUTO_CHECK_INTERVAL_MS),
            now
        ));
        assert!(!catalog_should_auto_check(
            true,
            Some(now - 1_000),
            None,
            now
        ));
        assert!(catalog_should_auto_check(
            true,
            Some(now - AUTO_CHECK_BACKOFF_MS),
            None,
            now
        ));
        // 时钟回退：saturating 归零 → 视为未到期，不 panic
        assert!(!catalog_should_auto_check(
            true,
            Some(now),
            Some(now - 1),
            now - 100
        ));
    }

    /// 契约：本地重载——磁盘出现更高 revision（他进程写入）时升级并返回
    /// true；不高时不动作（不降级、不发变更信号）。
    #[test]
    fn reload_from_disk_upgrades_only_when_newer() {
        let dir = TempDir::new("rl");
        let s = sync_with(&dir, MockHttp::ok(""));
        assert!(!s.reload_from_disk_if_newer(), "无缓存不升级");
        write_envelope(
            &dir,
            &CatalogCacheEnvelope {
                schema_version: 1,
                catalog: serde_json::from_str(&pkg(4)).unwrap(),
                last_attempt_ms: None,
                last_success_ms: None,
                last_error: None,
            },
        );
        assert!(s.reload_from_disk_if_newer());
        assert_eq!(s.status().revision, 4);
        assert_eq!(s.status().origin, CatalogOrigin::Cached);
        assert!(!s.reload_from_disk_if_newer(), "重复调用不动作");
    }
}
