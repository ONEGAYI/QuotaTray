//! GUI 冒烟注入器（仅手动运行，CI 不执行）。
//!
//! 向 `--data-dir` 沙箱写入带密文凭据的 `config.json`：
//! - native 条目来自 `--key-file`（`.DevApiKey.json`，空 key 跳过）；
//! - 追加一个指向本地 mock 服务的 template 条目（`--mock-url`）。
//!
//! 凭据经系统凭据库的正式主密钥加密（与 GUI 同一把 keyring 条目），
//! key 全程不回显（安全红线 1/2）。运行示例：
//!
//! ```text
//! cargo run -p quota-desktop --example smoke_setup -- \
//!   --data-dir <沙箱目录> --mock-url http://127.0.0.1:18080
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use quota_core::PlanVariant;
use quota_core::{AppConfig, ProviderEntry, ProviderKind, TemplateConfig, Vault};

fn main() {
    let mut data_dir: Option<PathBuf> = None;
    let mut key_file = PathBuf::from(".DevApiKey.json");
    let mut mock_url: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            "--key-file" => key_file = args.next().map(PathBuf::from).unwrap_or(key_file),
            "--mock-url" => mock_url = args.next(),
            other => {
                eprintln!("未知参数：{other}");
                std::process::exit(2);
            }
        }
    }
    let data_dir = data_dir.unwrap_or_else(|| std::env::temp_dir().join("quotatray-gui-smoke"));
    let mock_url = mock_url.unwrap_or_else(|| "http://127.0.0.1:18080".into());

    let vault = Vault::open(&quota_core::KeyringStore::new()).expect("打开系统凭据库失败");
    let keys: BTreeMap<String, String> = serde_json::from_str(
        &std::fs::read_to_string(&key_file)
            .unwrap_or_else(|e| panic!("读取 key 文件失败（{}）：{e}", key_file.display())),
    )
    .expect("key 文件应为 {\"平台id\": \"key\"} 的 JSON 对象");

    let mut providers = Vec::new();
    for (platform, key) in &keys {
        if key.trim().is_empty() {
            println!("[{platform}] 跳过（key 为空）");
            continue;
        }
        let mut entry = ProviderEntry {
            id: format!("smoke-{platform}"),
            name: platform.clone(),
            kind: ProviderKind::Native {
                provider: platform.clone(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
        };
        entry
            .set_api_key(&vault, key)
            .unwrap_or_else(|e| panic!("[{platform}] 凭据加密失败：{e}"));
        providers.push(entry);
    }

    // 指向本地 mock 服务的模板条目（http loopback 在 URL 安全规则内豁免）
    let template: TemplateConfig = serde_json::from_str(
        r#"{
            "request": {
                "url": "{{baseUrl}}/balance",
                "headers": { "Authorization": "Bearer {{apiKey}}" }
            },
            "extract": {
                "remaining": "$.data.balance",
                "unit": { "const": "CNY" },
                "planName": { "const": "Mock 套餐" }
            }
        }"#,
    )
    .expect("内置 mock 模板解析失败");
    let mut mock_entry = ProviderEntry {
        id: "smoke-mock".into(),
        name: "本地 Mock".into(),
        kind: ProviderKind::Template(Box::new(template)),
        enabled: true,
        api_key_enc: None,
        base_url: Some(mock_url),
        pricing: None,
        plan_variant: PlanVariant::Auto,
    };
    mock_entry
        .set_api_key(&vault, "sk-mock-smoke")
        .expect("mock 凭据加密失败");
    providers.push(mock_entry);

    let config_path = data_dir.join("config.json");
    AppConfig {
        providers,
        custom_models: Default::default(),
    }
    .save(&config_path)
    .expect("写入沙箱配置失败");
    println!(
        "已注入 {} 个条目到 {}（key 不回显）",
        count_entries(&config_path),
        config_path.display()
    );
}

fn count_entries(path: &std::path::Path) -> usize {
    AppConfig::load(path)
        .map(|c| c.providers.len())
        .unwrap_or(0)
}
