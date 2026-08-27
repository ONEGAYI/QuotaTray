//! 运行模式解析：安装态与便携态的路径契约（纯函数，不读进程环境）。
//!
//! 便携语义（预研报告 §4.2，方案 A）：exe 旁 [`PORTABLE_MARKER`] 标记
//! 便携形态，数据根落在 exe 旁 [`PORTABLE_DATA_DIR`]，主密钥为数据根内
//! 的 [`PORTABLE_KEY`]（32 字节随机，保密等级等同明文凭据）。判定与
//! 密钥来源必须绑定为本模块的 [`RuntimeMode`]——由调用端在构造任何
//! 持久化状态之前解析，杜绝「配置写进 Data、密钥却仍取 keyring」的
//! 混合状态。
//!
//! 解析优先级（[`resolve_mode`]）：
//! 1. `--data-dir` 保持安装态（烟测沙箱语义，密钥仍走系统凭据库）；
//! 2. `--portable` 显式选择 exe 旁 `Data/`；
//! 3. 无显式参数时才检测 exe 旁 marker；
//! 4. 都没有 → 默认安装态（`~/.quotatray` + keyring）。

use std::path::{Path, PathBuf};

/// 便携模式标记文件名（exe 同目录，空文件，内容忽略）。
pub const PORTABLE_MARKER: &str = "portable.marker";
/// 便携主密钥文件名（数据根内，32 字节裸二进制）。
pub const PORTABLE_KEY: &str = "portable.key";
/// 便携数据目录名（exe 同目录下）。
pub const PORTABLE_DATA_DIR: &str = "Data";

/// 运行形态：数据根与密钥后端的绑定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeMode {
    /// 安装态：数据根为 `~/.quotatray`（`data_dir` 仅作烟测沙箱覆盖），
    /// 主密钥走系统凭据库（`KeyringStore`）。
    Installed { data_dir: Option<PathBuf> },
    /// 便携态：数据根为 exe 旁 `Data/`，主密钥走 `Data/portable.key`
    /// （`FileStore`）。`root` 已是数据根本身。
    Portable { root: PathBuf },
}

impl RuntimeMode {
    pub fn is_portable(&self) -> bool {
        matches!(self, Self::Portable { .. })
    }

    /// 便携主密钥路径（安装态返回 None）。
    pub fn portable_key(&self) -> Option<PathBuf> {
        match self {
            Self::Portable { root } => Some(portable_key_path(root)),
            Self::Installed { .. } => None,
        }
    }
}

/// exe 旁便携数据根：`<exe_dir>/Data`。
pub fn portable_data_root(exe_dir: &Path) -> PathBuf {
    exe_dir.join(PORTABLE_DATA_DIR)
}

/// exe 旁是否存在便携标记文件（普通文件即可，内容忽略；同名目录不算
/// ——防误建目录被当成便携标记）。不存在 ≠ 目录不可读：读取错误按无
/// marker 处理，交由后续数据目录创建/写入流程暴露真实 IO 错误。
pub fn has_portable_marker(exe_dir: &Path) -> bool {
    exe_dir
        .join(PORTABLE_MARKER)
        .metadata()
        .is_ok_and(|m| m.is_file())
}

/// 数据根内的便携主密钥路径。
pub fn portable_key_path(root: &Path) -> PathBuf {
    root.join(PORTABLE_KEY)
}

/// 运行模式解析（输入由调用端从 argv / exe 位置取得，本函数不读环境）。
pub fn resolve_mode(
    data_dir_override: Option<PathBuf>,
    explicit_portable: bool,
    exe_dir: &Path,
) -> RuntimeMode {
    if data_dir_override.is_some() {
        return RuntimeMode::Installed {
            data_dir: data_dir_override,
        };
    }
    if explicit_portable || has_portable_marker(exe_dir) {
        return RuntimeMode::Portable {
            root: portable_data_root(exe_dir),
        };
    }
    RuntimeMode::Installed { data_dir: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qt-runtime-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 契约：路径推导——数据根为 exe 旁 Data，密钥在数据根内。
    #[test]
    fn portable_paths_derive_from_exe_dir() {
        let exe_dir = Path::new("/usb/QuotaTray");
        assert_eq!(
            portable_data_root(exe_dir),
            Path::new("/usb/QuotaTray/Data")
        );
        assert_eq!(
            portable_key_path(&portable_data_root(exe_dir)),
            Path::new("/usb/QuotaTray/Data/portable.key")
        );
    }

    /// 契约：marker 存在性判定（空文件即标记，内容忽略；同名目录不算）。
    #[test]
    fn marker_detection_follows_file_existence() {
        let dir = temp_dir("marker");
        assert!(!has_portable_marker(&dir), "无 marker = 非便携");
        std::fs::write(dir.join(PORTABLE_MARKER), "").unwrap();
        assert!(has_portable_marker(&dir));
        // 同名目录是误建，不构成便携标记
        std::fs::remove_file(dir.join(PORTABLE_MARKER)).unwrap();
        std::fs::create_dir(dir.join(PORTABLE_MARKER)).unwrap();
        assert!(!has_portable_marker(&dir), "目录不算 marker");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：解析优先级——--data-dir 覆盖不落入便携（沙箱语义），
    /// --portable 显式便携，无参数时才看 marker，默认安装态。
    #[test]
    fn resolve_mode_priority_contract() {
        let dir = temp_dir("resolve");
        std::fs::write(dir.join(PORTABLE_MARKER), "").unwrap();

        // 1. --data-dir 优先且保持安装态
        let m = resolve_mode(Some(dir.join("sandbox")), false, &dir);
        assert_eq!(
            m,
            RuntimeMode::Installed {
                data_dir: Some(dir.join("sandbox"))
            },
            "--data-dir 不得因 marker 存在而落入便携"
        );
        // --data-dir 与 --portable 同现：data-dir 赢（安装态沙箱优先）
        assert!(!resolve_mode(Some(dir.join("sandbox")), true, &dir).is_portable());

        // 2. --portable 显式便携（即使无 marker）
        std::fs::remove_file(dir.join(PORTABLE_MARKER)).unwrap();
        let m = resolve_mode(None, true, &dir);
        assert_eq!(
            m,
            RuntimeMode::Portable {
                root: dir.join("Data")
            }
        );

        // 3. 无参数 + marker → 便携
        std::fs::write(dir.join(PORTABLE_MARKER), "").unwrap();
        assert!(resolve_mode(None, false, &dir).is_portable());

        // 4. 无参数无 marker → 安装态
        std::fs::remove_file(dir.join(PORTABLE_MARKER)).unwrap();
        assert_eq!(
            resolve_mode(None, false, &dir),
            RuntimeMode::Installed { data_dir: None }
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 契约：RuntimeMode 辅助方法——便携态给出密钥路径，安装态不给。
    #[test]
    fn runtime_mode_helpers() {
        let portable = RuntimeMode::Portable {
            root: PathBuf::from("/usb/Data"),
        };
        assert!(portable.is_portable());
        assert_eq!(
            portable.portable_key(),
            Some(PathBuf::from("/usb/Data/portable.key"))
        );
        let installed = RuntimeMode::Installed { data_dir: None };
        assert!(!installed.is_portable());
        assert_eq!(installed.portable_key(), None);
    }
}
