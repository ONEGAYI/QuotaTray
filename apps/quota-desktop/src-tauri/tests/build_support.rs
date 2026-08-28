#[path = "../build_support.rs"]
mod build_support;

use std::path::Path;

#[test]
fn native_build_reads_flat_target_profile() {
    assert_eq!(
        build_support::quota_cli_source(
            Path::new("C:/repo"),
            "release",
            "x86_64-pc-windows-msvc",
            "x86_64-pc-windows-msvc",
        ),
        Path::new("C:/repo/target/release/quota.exe")
    );
}

#[test]
fn cross_build_reads_target_triple_profile() {
    assert_eq!(
        build_support::quota_cli_source(
            Path::new("C:/repo"),
            "release",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ),
        Path::new("C:/repo/target/aarch64-pc-windows-msvc/release/quota.exe")
    );
}
