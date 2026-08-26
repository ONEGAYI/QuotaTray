fn main() {
    stage_quota_cli();
    tauri_build::build()
}

/// Tauri 资源清单要求源文件在普通 cargo test 时也存在；先把同 profile 的
/// quota CLI 暂存到 ignored 目录。release 缺 CLI 直接失败，防止安装包静默
/// 携带空占位；debug/test 缺失时只生成配置校验用占位，tauri dev 的
/// beforeDevCommand 会先构建真实 debug CLI，因此实际运行不会命中占位。
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
    } else if profile == "release" {
        panic!(
            "release quota CLI 不存在：{}；请先运行 cargo build -p quota-cli --release",
            source.display()
        );
    } else if !staged.exists() {
        std::fs::write(&staged, []).expect("创建 quota CLI 测试占位失败");
    }
}

#[cfg(not(windows))]
fn stage_quota_cli() {}
