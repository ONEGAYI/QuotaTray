//! 应用状态：查询引擎、保险库、数据路径与条目结果表。
//!
//! 结果表（`results`）是托盘与前端共享的数据源：
//! 查询命令写入它，托盘重建读取它；启动时从快照恢复，
//! 使托盘在首次查询完成前即有内容（消除空窗）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::RwLock;

use quota_core::pricing::PeakKind;
use quota_core::{HistoryStore, QueryEngine, Vault};
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

    /// 查询历史库（M5，30 天滚动）。
    pub fn history(&self) -> PathBuf {
        self.root.join("history.db")
    }
}

/// 错误信息（IPC 传输形状，kind 对齐 CLI `--json` 约定的小写字符串）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ErrorInfo {
    pub kind: String,
    pub message: String,
    /// 排查详情（core 已脱敏的响应体片段、serde 解析位置等），
    /// 仅供用户在卡片上显式复制；仅在存在时序列化。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
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
            detail: e.detail().map(str::to_string),
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
    /// 查询引擎：代理端口设置变更时热重建（save_settings），
    /// 读多写少用 RwLock。
    pub engine: std::sync::RwLock<QueryEngine>,
    pub vault: Vault,
    pub paths: DataPaths,
    /// 运行形态（安装/便携）：更新资产选择与自启动门控的依据，
    /// 与 paths/vault 的构造来源在 init 时一次性绑定。
    pub mode: quota_core::RuntimeMode,
    pub settings: RwLock<Settings>,
    pub results: RwLock<HashMap<String, EntryState>>,
    /// 解析后的实际主题（true = dark）。前端 theme context 解析三态后推送
    /// （system 跟随 matchMedia 变化时同样推送），托盘圆环图标配色取用。
    /// 初始 false：跨平台无轻量取系统主题的 Rust API，前端首帧即推送
    /// 真实值，托盘首建的一帧浅色误差可接受（详见 commands::set_resolved_theme）。
    pub resolved_theme: RwLock<bool>,
    /// 更新检测的展示状态（版本信息/上次检测/错误）；
    /// 节流判定权威源是 settings.update_last_check（磁盘），此处为展示镜像。
    pub update_ctl: RwLock<UpdateCtlState>,
    /// 上次峰谷检测时全部启用条目的判定快照（条目 id → 峰/谷）。
    /// 每分钟调度比对（`tray::rebuild_on_peak_flip`，覆盖全部有峰谷配置
    /// 的条目而非仅图标条目——主窗卡片与悬停面板同样消费该判定），
    /// 翻转才重建托盘并向 WebView 广播，避免轮询间隔长时标签过期。
    pub last_peak: RwLock<HashMap<String, PeakKind>>,
    /// 查询历史库（M5）。非关键数据：打开失败降级内存库（eprintln 告警），
    /// 查询主链路照常，仅历史不落盘。
    pub history: Mutex<HistoryStore>,
}

/// 启动门控：便携形态首次运行（`portable.key` 缺失）时，setup 阶段
/// 不初始化 AppState/托盘/调度器，前端先渲染首启安全确认页；用户确认
/// 后由 `confirm_portable_init` 命令建钥并补齐全部启动步骤。
///
/// `mode` 快照避免 confirm 时二次解析 argv/exe（幂等性由解析纯函数
/// 保证，此处直接复用结果）；完成后 `pending` 置 None，`get_boot_state`
/// 据此返回 ready。
#[derive(Default)]
pub struct BootGate {
    pub pending: std::sync::Mutex<Option<quota_core::RuntimeMode>>,
}

/// 当前 epoch 毫秒。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 网络代理端口变更后热重建查询引擎（save_settings 调用）。
pub fn rebuild_engine(state: &AppState) -> Result<(), quota_core::http::HttpError> {
    let proxy_port = state.settings.read().unwrap().update_proxy_port;
    let engine = build_engine(proxy_port)?;
    *state.engine.write().unwrap() = engine;
    Ok(())
}

/// 代理端口磁盘对账自愈：内存端口为空时读磁盘 settings 比对，磁盘有
/// 端口（典型成因：启动加载抖动回退了默认值）则整体以磁盘同步内存并
/// 热重建引擎。返回 true = 已修复。
///
/// 窄触发设计：内存端口非空直接返回（一次读锁，零磁盘开销），只在
/// "将要产生『未配置代理端口』确定性错误"的脱节状态才触碰磁盘——
/// 正常态永不覆盖内存，避免与用户刚保存的设置竞争。
/// rebuild 失败时回滚内存（get_settings 与引擎保持一致）并返回 false。
pub fn reconcile_proxy_from_disk(state: &AppState) -> bool {
    if state.settings.read().unwrap().update_proxy_port.is_some() {
        return false;
    }
    let disk = Settings::load(&state.paths.settings());
    if disk.update_proxy_port.is_none() {
        return false;
    }
    // 写锁内 double-check（CAS 语义）：读盘期间并发保存（save/patch）
    // 可能已把含端口的完整设置写入内存——此时不覆盖，用户刚保存的
    // 值优先；写锁即取即释，锁释放后才 rebuild（内部拿读锁，无嵌套）。
    let prev = {
        let mut guard = state.settings.write().unwrap();
        if guard.update_proxy_port.is_some() {
            return false;
        }
        let prev = guard.clone();
        *guard = disk;
        prev
    };
    if let Err(e) = rebuild_engine(state) {
        eprintln!("代理端口对账后重建查询引擎失败，回滚内存设置：{e}");
        *state.settings.write().unwrap() = prev;
        return false;
    }
    eprintln!(
        "代理端口对账自愈：磁盘端口 {} 已同步进运行态并重建查询引擎",
        state
            .settings
            .read()
            .unwrap()
            .update_proxy_port
            .unwrap_or_default()
    );
    true
}

/// 按代理端口构造双通道查询引擎：直连通道恒在，代理通道仅在配了
/// 全局网络代理端口时装配（条目 use_proxy 决定路由，未配端口而条目
/// 开代理 → 引擎路由层确定性引导）。
fn build_engine(proxy_port: Option<u16>) -> Result<QueryEngine, quota_core::http::HttpError> {
    let direct = quota_core::http::ReqwestHttpClient::new(quota_core::DEFAULT_TIMEOUT)?;
    let proxied = match quota_core::update::proxy_url_of(proxy_port).as_deref() {
        Some(url) => Some(quota_core::http::ReqwestHttpClient::new_with_proxy(
            quota_core::DEFAULT_TIMEOUT,
            Some(url),
        )?),
        None => None,
    };
    Ok(QueryEngine::with_proxied(
        std::sync::Arc::new(direct),
        proxied.map(|c| std::sync::Arc::new(c) as std::sync::Arc<dyn quota_core::http::HttpClient>),
        quota_core::DEFAULT_TIMEOUT,
    ))
}

impl AppState {
    /// 初始化：按运行形态打开保险库（安装态 keyring / 便携态
    /// `Data/portable.key`）、构造引擎、恢复设置与快照。
    ///
    /// 便携形态的防御：密钥文件必须已存在（首启建钥的确认门控在
    /// 调用方完成，见 lib.rs BootGate）——此处缺失即报错，杜绝任何
    /// 未经「固定安全提示 + 显式确认」的静默建钥路径。
    pub fn init(mode: quota_core::RuntimeMode) -> Result<Self, String> {
        let paths = DataPaths::new(match &mode {
            quota_core::RuntimeMode::Installed { data_dir } => data_dir.clone(),
            quota_core::RuntimeMode::Portable { root } => Some(root.clone()),
        })?;
        let vault = match &mode {
            quota_core::RuntimeMode::Installed { .. } => {
                Vault::open(&quota_core::KeyringStore::new())
            }
            quota_core::RuntimeMode::Portable { root } => {
                let key_path = quota_core::portable_key_path(root);
                if !key_path.is_file() {
                    return Err(format!(
                        "便携主密钥不存在（{}）：请先完成首次运行的安全确认",
                        key_path.display()
                    ));
                }
                Vault::open(&quota_core::FileStore::new(key_path))
            }
        }
        .map_err(|e| format!("凭据保险库初始化失败：{e}"))?;
        let settings = Settings::load(&paths.settings());
        // 查询通道代理：复用设置中的网络代理端口（chatgpt.com 等被墙
        // 站点的订阅查询必需），proxy_url_of 与更新通道同口径
        let engine = build_engine(settings.update_proxy_port)
            .map_err(|e| format!("HTTP 客户端初始化失败：{e}"))?;
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
        // 历史库（M5）非关键：打开失败降级内存库，不阻断启动
        let history = match HistoryStore::open(&paths.history()) {
            Ok(store) => store,
            Err(e) => {
                eprintln!("历史库打开失败（本次运行不落盘）：{e}");
                HistoryStore::open_in_memory().map_err(|e| e.to_string())?
            }
        };
        Ok(Self {
            engine: std::sync::RwLock::new(engine),
            vault,
            settings: RwLock::new(settings),
            results: RwLock::new(results),
            paths,
            mode,
            resolved_theme: RwLock::new(false),
            // last_check 展示镜像从磁盘恢复（info 留空：启动后调度任务会补检）
            update_ctl: RwLock::new(UpdateCtlState {
                last_check,
                ..Default::default()
            }),
            last_peak: RwLock::new(HashMap::new()),
            history: Mutex::new(history),
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

    /// 契约：便携形态的 init 防御——密钥文件必须已存在（首启建钥的
    /// 确认门控在调用方），缺失即确定性错误，绝不静默建钥；key 就位后
    /// init 成功且 paths/mode 均绑定便携数据根（FileStore 后端互通）。
    #[test]
    fn app_state_init_portable_requires_existing_key() {
        let root = std::env::temp_dir().join(format!("qt-init-port-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mode = quota_core::RuntimeMode::Portable { root: root.clone() };

        let err = match AppState::init(mode.clone()) {
            Err(e) => e,
            Ok(_) => panic!("密钥缺失时 init 不应成功"),
        };
        assert!(
            err.contains("便携主密钥不存在") && err.contains("portable.key"),
            "缺失密钥应确定性报错并带路径：{err}"
        );
        assert!(
            !quota_core::portable_key_path(&root).exists(),
            "防御分支不得创建密钥"
        );

        // 就位后成功：mode/paths 绑定便携根，密钥经 FileStore 互通
        quota_core::Vault::open(&quota_core::FileStore::new(quota_core::portable_key_path(
            &root,
        )))
        .unwrap();
        let state = AppState::init(mode).unwrap();
        assert!(state.mode.is_portable());
        assert_eq!(state.paths.config(), root.join("config.json"));
        assert_eq!(state.paths.settings(), root.join("settings.json"));
        let _ = std::fs::remove_dir_all(&root);
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

    /// 手工组装最小 AppState（AppState 依赖 keyring，测试绕开生产构造；
    /// 与 update_ctl 测试的 sandbox_state 同款）。
    fn sandbox_state(dir: &std::path::Path) -> AppState {
        let paths = DataPaths::new(Some(dir.to_path_buf())).unwrap();
        let vault = quota_core::Vault::open(&quota_core::InMemoryStore::new()).unwrap();
        let engine = quota_core::QueryEngine::with_default_client().unwrap();
        AppState {
            mode: quota_core::RuntimeMode::Installed {
                data_dir: Some(dir.to_path_buf()),
            },
            engine: std::sync::RwLock::new(engine),
            vault,
            paths,
            settings: RwLock::new(Settings::default()),
            results: RwLock::new(HashMap::new()),
            resolved_theme: RwLock::new(false),
            update_ctl: RwLock::new(crate::update_ctl::UpdateCtlState::default()),
            last_peak: RwLock::new(HashMap::new()),
            history: std::sync::Mutex::new(quota_core::HistoryStore::open_in_memory().unwrap()),
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qt-reconcile-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 契约：内存端口为空而磁盘有端口（启动加载抖动丢字段）→ 同步内存
    /// 并重建引擎，返回 true。
    #[test]
    fn reconcile_recovers_port_from_disk() {
        let dir = temp_dir("recover");
        let state = sandbox_state(&dir);
        Settings {
            update_proxy_port: Some(7897),
            ..Settings::default()
        }
        .save(&state.paths.settings())
        .unwrap();

        assert!(reconcile_proxy_from_disk(&state), "脱节状态应修复");
        assert_eq!(
            state.settings.read().unwrap().update_proxy_port,
            Some(7897),
            "内存端口已从磁盘同步"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：内存端口非空（正常态）不触盘不改动——磁盘端口与内存不同
    /// 也不覆盖，避免与用户刚保存的设置竞争。
    #[test]
    fn reconcile_noop_when_memory_has_port() {
        let dir = temp_dir("noop");
        let state = sandbox_state(&dir);
        *state.settings.write().unwrap() = Settings {
            update_proxy_port: Some(1080),
            ..Settings::default()
        };
        Settings {
            update_proxy_port: Some(7897),
            ..Settings::default()
        }
        .save(&state.paths.settings())
        .unwrap();

        assert!(!reconcile_proxy_from_disk(&state));
        assert_eq!(
            state.settings.read().unwrap().update_proxy_port,
            Some(1080),
            "正常态内存不被磁盘覆盖"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：内存与磁盘都无端口（用户确实没配）→ 不动，返回 false。
    #[test]
    fn reconcile_noop_when_disk_also_empty() {
        let dir = temp_dir("both-empty");
        let state = sandbox_state(&dir);

        assert!(!reconcile_proxy_from_disk(&state));
        assert_eq!(state.settings.read().unwrap().update_proxy_port, None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
