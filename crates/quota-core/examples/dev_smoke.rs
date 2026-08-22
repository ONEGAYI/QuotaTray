//! 本地冒烟测试：读取仓库根目录的 `.DevApiKey.json`，对真实平台发起查询。
//!
//! 仅手动运行（在仓库根目录执行
//! `cargo run -p quota-core --example dev_smoke`），CI 与单元测试不依赖。
//! 文件格式见 `.DevApiKey.json.example`；空 key 的平台自动跳过。

use std::collections::BTreeMap;

use quota_core::{ProviderEntry, ProviderKind, QueryEngine, Vault};

#[tokio::main]
async fn main() {
    let keys: BTreeMap<String, String> = serde_json::from_str(
        &std::fs::read_to_string(".DevApiKey.json")
            .expect("找不到 .DevApiKey.json（应在仓库根目录运行）"),
    )
    .expect("key 文件应为 {\"平台id\": \"key\"} 的 JSON 对象");

    let mut failures = 0usize;
    for (platform, key) in &keys {
        if key.trim().is_empty() {
            println!("[{platform}] 跳过（key 为空）");
            continue;
        }
        let kind = match platform.as_str() {
            "deepseek" => ProviderKind::Native { provider: "deepseek".into() },
            "siliconflow" => ProviderKind::Native { provider: "siliconflow".into() },
            "openrouter" => ProviderKind::Native { provider: "openrouter".into() },
            other => {
                println!("[{other}] 跳过（未知平台 id）");
                failures += 1;
                continue;
            }
        };

        // 走 core 完整链路：InMemory vault 加密 → 解密 → 真实 HTTP → 解析
        let vault = Vault::open(&quota_core::InMemoryStore::new()).unwrap();
        let mut entry = ProviderEntry {
            id: format!("smoke-{platform}"),
            name: platform.clone(),
            kind,
            enabled: true,
            api_key_enc: None,
            base_url: None,
        };
        entry.set_api_key(&vault, key).unwrap();

        let engine = QueryEngine::with_default_client().unwrap();
        match engine.query(&vault, &entry).await {
            Ok(data) => {
                for d in &data {
                    println!(
                        "[{platform}] OK {} remaining={:?} used={:?} unit={:?} valid={:?} msg={:?}",
                        d.plan_name.clone().unwrap_or_default(),
                        d.remaining,
                        d.used,
                        d.unit,
                        d.is_valid,
                        d.invalid_message,
                    );
                }
            }
            Err(e) => {
                println!("[{platform}] FAIL {e}");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        println!("共 {failures} 个平台失败");
        std::process::exit(1);
    }
    println!("全部通过");
}
