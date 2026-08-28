use std::path::{Path, PathBuf};

/// 根据 Cargo 的宿主/目标三元组定位 quota CLI。
///
/// 原生构建位于 `target/<profile>`；交叉构建位于
/// `target/<target>/<profile>`。目标判断必须使用 Cargo 传给构建脚本的
/// HOST/TARGET，不能使用反映构建脚本宿主的 `cfg!`。
pub fn quota_cli_source(workspace_root: &Path, profile: &str, host: &str, target: &str) -> PathBuf {
    let target_root = workspace_root.join("target");
    let profile_root = if host == target {
        target_root.join(profile)
    } else {
        target_root.join(target).join(profile)
    };
    profile_root.join("quota.exe")
}
