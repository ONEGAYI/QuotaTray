//! 应用状态：查询引擎、保险库、数据路径与条目结果表。
//!
//! 结果表（`results`）是托盘与前端共享的数据源：
//! 查询命令写入它，托盘重建读取它；启动时从快照恢复，
//! 使托盘在首次查询完成前即有内容（消除空窗）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::atomic::AtomicU64;

use quota_core::{QueryEngine, Vault};
use serde::Serialize;

use crate::settings::Settings;
use crate::snapshot::Snapshots;
use crate::update_ctl::UpdateCtlState;

/// 数据目录布局：`~/.quotatray/`（可被 `--data-dir` 调试参数覆盖）。
#[derive(Debug, Clone)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    pub fn new(override_dir: Option<PathBuf>) -> Result<Self, String> {
        let root = match override_dir {
            Some(dir) => dir,
            None => {
                let home = dirs::home_dir().ok_or("无法定位用户主目录")?;
                home.join(".quotatray")
            }
        };
        Ok(Self { root })
    }

    pub fn config(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn snapshot(&self) -> PathBuf {
        self.root.join("cache.json")
    }
}

/// 错误信息（IPC 传输形状，kind 对齐 CLI `--json` 约定的小写字符串）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorInfo {
    pub kind: String,
    pub message: String,
}

impl ErrorInfo {
    pub fn from_query_error(e: &quota_core::QueryError) -> Self {
        Self {
            kind: if e.is_transient() {
                "transient"
            } else {
                "deterministic"
            }
            .into(),
            message: e.message().to_string(),
        }
    }
}

/// 单条目的当前状态（结果表条目）。
///
/// `data` / `at` 保留最后一次成功结果（keep-last-good 数据源），
/// `error` 是最近一次查询的错误（覆盖式更新）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntryState {
    pub data: Option<Vec<quota_core::UsageData>>,
    /// 最后一次成功时刻（epoch 毫秒）。
    pub at: Option<u64>,
    pub error: Option<ErrorInfo>,
}

/// 查询命令的 IPC 返回形状。
#[derive(Debug, Clone, Serialize)]
pub struct QueryOutcome {
    /// 本次查询是否成功。
    pub ok: bool,
    /// 保留的（可能是上一次成功的）用量数据。
    pub data: Option<Vec<quota_core::UsageData>>,
    pub error: Option<ErrorInfo>,
    /// 最后成功时刻（epoch 毫秒）。
    pub at: Option<u64>,
}

/// 全局应用状态（Tauri manage 托管）。
pub struct AppState {
    pub engine: QueryEngine,
    pub vault: Vault,
    pub paths: DataPaths,
    pub settings: RwLock<Settings>,
    pub results: RwLock<HashMap<String, EntryState>>,
    /// 托盘悬停刷新的上次触发时刻（epoch 毫秒），节流用。
    pub last_hover_refresh_ms: AtomicU64,
    /// 解析后的实际主题（true = dark）。前端 theme context 解析三态后推送
    /// （system 跟随 matchMedia 变化时同样推送），托盘圆环图标配色取用。
    /// 初始 false：跨平台无轻量取系统主题的 Rust API，前端首帧即推送
    /// 真实值，托盘首建的一帧浅色误差可接受（详见 commands::set_resolved_theme）。
    pub resolved_theme: RwLock<bool>,
    /// 更新检测的展示状态（版本信息/上次检测/错误）；
    /// 节流判定权威源是 settings.update_last_check（磁盘），此处为展示镜像。
    pub update_ctl: RwLock<UpdateCtlState>,
}

/// 当前 epoch 毫秒。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl AppState {
    /// 初始化：打开保险库（系统凭据库）、构造引擎、恢复设置与快照。
    pub fn init(data_dir: Option<PathBuf>) -> Result<Self, String> {
        let paths = DataPaths::new(data_dir)?;
        let store = quota_core::KeyringStore::new();
        let vault = Vault::open(&store).map_err(|e| format!("凭据保险库初始化失败：{e}"))?;
        let engine = QueryEngine::with_default_client()
            .map_err(|e| format!("HTTP 客户端初始化失败：{e}"))?;
        let settings = Settings::load(&paths.settings());
        // last_check 展示镜像从磁盘恢复（info 留空：启动后调度任务会补检）
        let last_check = settings.update_last_check;
        let mut results = HashMap::new();
        for (id, snap) in Snapshots::load(&paths.snapshot()).entries {
            results.insert(
                id,
                EntryState {
                    data: Some(snap.data),
                    at: Some(snap.at),
                    error: None,
                },
            );
        }
        Ok(Self {
            engine,
            vault,
            settings: RwLock::new(settings),
            results: RwLock::new(results),
            paths,
            last_hover_refresh_ms: AtomicU64::new(0),
            resolved_theme: RwLock::new(false),
            // last_check 展示镜像从磁盘恢复（info 留空：启动后调度任务会补检）
            update_ctl: RwLock::new(UpdateCtlState {
                last_check,
                ..Default::default()
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：ErrorInfo 分类字符串与 CLI --json 约定一致。
    #[test]
    fn error_info_kind_strings() {
        let t = ErrorInfo::from_query_error(&quota_core::QueryError::transient("x"));
        assert_eq!(t.kind, "transient");
        let d = ErrorInfo::from_query_error(&quota_core::QueryError::deterministic("y"));
        assert_eq!(d.kind, "deterministic");
        assert_eq!(d.message, "y");
    }

    /// 契约：--data-dir 覆盖生效；缺省落到 ~/.quotatray。
    #[test]
    fn data_paths_respect_override() {
        let p = DataPaths::new(Some(PathBuf::from("/tmp/sandbox"))).unwrap();
        assert_eq!(p.config(), PathBuf::from("/tmp/sandbox/config.json"));
        assert_eq!(p.snapshot(), PathBuf::from("/tmp/sandbox/cache.json"));
        let default = DataPaths::new(None).unwrap();
        assert!(
            default.config().ends_with(".quotatray\\config.json")
                || default.config().ends_with(".quotatray/config.json")
        );
    }
}
