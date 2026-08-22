//! `quota dev-smoke`：真机冒烟（仅 debug 构建，release 不存在本模块）。
//!
//! 读 `.DevApiKey.json`（格式见 `.DevApiKey.json.example`），
//! 逐平台走 core 完整链路（InMemory 加密→解密→真实 HTTP→解析）。
//! 仅手动运行，CI 不执行。

use std::collections::BTreeMap;
use std::path::PathBuf;

use quota_core::config::{ProviderEntry, ProviderKind};
use quota_core::{InMemoryStore, QueryEngine, Vault, provider};

/// key 文件中平台 key 的分类结果。
pub struct Classified {
    /// (平台 id, key)——平台已注册且 key 非空。
    pub runnable: Vec<(String, String)>,
    /// 空 key 平台（跳过，不算失败）。
    pub skipped: Vec<String>,
    /// 未注册平台 id（告警 + 计失败）。
    pub unknown: Vec<String>,
}

/// 解析 key 文件内容并按注册表分类（纯函数）。
pub fn classify_keys(raw: &str) -> Result<Classified, String> {
    let keys: BTreeMap<String, String> = serde_json::from_str(raw)
        .map_err(|e| format!("key 文件应为 {{\"平台id\": \"key\"}} 的 JSON 对象：{e}"))?;
    let mut out = Classified {
        runnable: Vec::new(),
        skipped: Vec::new(),
        unknown: Vec::new(),
    };
    let known: Vec<&str> = provider::metas().iter().map(|m| m.id).collect();
    for (platform, key) in keys {
        if key.trim().is_empty() {
            out.skipped.push(platform);
        } else if known.contains(&platform.as_str()) {
            out.runnable.push((platform, key));
        } else {
            out.unknown.push(platform);
        }
    }
    Ok(out)
}

pub async fn run(key_file: Option<PathBuf>) -> i32 {
    let path = key_file.unwrap_or_else(|| PathBuf::from(".DevApiKey.json"));
    let raw = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "错误：无法读取 {}（{e}）；在仓库根运行或用 --key-file 指定",
                path.display()
            );
            return 1;
        }
    };
    let classified = match classify_keys(&raw) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("错误：{msg}");
            return 1;
        }
    };

    for platform in &classified.skipped {
        println!("[{platform}] 跳过（key 为空）");
    }
    for platform in &classified.unknown {
        println!("[{platform}] 跳过（未知平台 id）");
    }

    let vault = match Vault::open(&InMemoryStore::new()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("错误：{e}");
            return 1;
        }
    };
    let engine = match QueryEngine::with_default_client() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("错误：{e}");
            return 1;
        }
    };

    let mut failures = classified.unknown.len();
    for (platform, key) in &classified.runnable {
        let mut entry = ProviderEntry {
            id: format!("smoke-{platform}"),
            name: platform.clone(),
            kind: ProviderKind::Native {
                provider: platform.clone(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
        };
        if let Err(e) = entry.set_api_key(&vault, key) {
            eprintln!("[{platform}] 加密失败：{e}");
            failures += 1;
            continue;
        }
        match engine.query(&vault, &entry).await {
            Ok(data) => {
                for d in &data {
                    println!(
                        "[{platform}] OK {} remaining={} used={} unit={} valid={} msg={:?}",
                        d.plan_name.clone().unwrap_or_default(),
                        d.remaining
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".into()),
                        d.used.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                        d.unit.clone().unwrap_or_else(|| "-".into()),
                        match d.is_valid {
                            Some(false) => "否",
                            _ => "是",
                        },
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
        println!("共 {failures} 个平台失败/未知");
        1
    } else {
        println!("全部通过");
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：分类——空 key 跳过、未知 id 告警、已注册平台进入 runnable。
    #[test]
    fn classifies_key_file() {
        let raw = serde_json::json!({
            "deepseek": "sk-1",
            "siliconflow": "  ",
            "no-such-platform": "sk-2"
        })
        .to_string();
        let c = classify_keys(&raw).unwrap();
        assert_eq!(
            c.runnable,
            vec![("deepseek".to_string(), "sk-1".to_string())]
        );
        assert_eq!(c.skipped, vec!["siliconflow".to_string()]);
        assert_eq!(c.unknown, vec!["no-such-platform".to_string()]);
    }

    /// 契约：坏 JSON 报错。
    #[test]
    fn bad_key_file_rejected() {
        assert!(classify_keys("not json").is_err());
    }
}
