fn main() {
    stage_quota_cli();
    tauri_build::build()
}

/// Tauri 资源清单要求源文件在普通 cargo test/check 时也存在；先把同 profile
/// 的 quota CLI 暂存到 ignored 目录。只在 Tauri CLI 驱动的构建
/// （TAURI_ENV_* 存在，且 beforeBuild/beforeDevCommand 已先行构建 CLI）中
/// release 缺 CLI 才 panic，防止安装包静默携带空占位；普通
/// `cargo check/build/test`（含 CI 的 `cargo check --release`，不产出 bin）
/// 缺失时落空占位——产物只用于资源清单存在性校验。
#[cfg(windows)]
fn stage_quota_cli() {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("缺少 CARGO_MANIFEST_DIR"),
    );
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let source = manifest_dir
        .join("../../..")
        .join("target")
        .join(&profile)
        .join("quota.exe");
    let staged = manifest_dir.join("generated/quota.exe");
    println!("cargo:rerun-if-changed={}", source.display());
    std::fs::create_dir_all(staged.parent().expect("暂存路径应有父目录"))
        .expect("创建 quota CLI 暂存目录失败");

    if source.is_file() {
        std::fs::copy(&source, &staged).expect("暂存 quota CLI 失败");
    } else if profile == "release" && tauri_cli_driven() {
        panic!(
            "release quota CLI 不存在：{}；请先运行 cargo build -p quota-cli --release",
            source.display()
        );
    } else if !staged.exists() {
        std::fs::write(&staged, []).expect("创建 quota CLI 测试占位失败");
    }
}

/// 是否由 Tauri CLI 驱动（tauri build/dev 会注入 TAURI_ENV_* 环境变量）。
fn tauri_cli_driven() -> bool {
    std::env::var_os("TAURI_ENV_PLATFORM").is_some()
        || std::env::var_os("TAURI_ENV_ARCH").is_some()
        || std::env::var_os("TAURI_ENV_FAMILY").is_some()
}

#[cfg(not(windows))]
fn stage_quota_cli() {}
