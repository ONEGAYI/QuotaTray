//! `quota dev-smoke`：真机冒烟（仅 debug 构建，release 不存在本模块）。
//!
//! 读 `.DevApiKey.json`（格式见 `.DevApiKey.json.example`），
//! 逐平台走 core 完整链路（InMemory 加密→解密→真实 HTTP→解析）。
//! 仅手动运行，CI 不执行。

use std::collections::BTreeMap;
use std::path::PathBuf;

use quota_core::config::{PlanVariant, ProviderEntry, ProviderKind};
use quota_core::{InMemoryStore, Vault, provider};

use crate::lang::Lang;
use crate::texts::{self, T, t};

/// key 文件中平台 key 的分类结果。
#[derive(Debug)]
pub struct Classified {
    /// (平台 id, key)——平台已注册且 key 非空。
    pub runnable: Vec<(String, String)>,
    /// 空 key 平台（跳过，不算失败）。
    pub skipped: Vec<String>,
    /// 未注册平台 id（告警 + 计失败）。
    pub unknown: Vec<String>,
}

/// 解析 key 文件内容并按注册表分类（纯函数）。
pub fn classify_keys(raw: &str, lang: Lang) -> Result<Classified, String> {
    let keys: BTreeMap<String, String> =
        serde_json::from_str(raw).map_err(|e| format!("{}{e}", t(lang, T::SmokeKeyFileFormat)))?;
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

pub async fn run(key_file: Option<PathBuf>, proxy: bool, lang: Lang) -> i32 {
    let path = key_file.unwrap_or_else(|| PathBuf::from(".DevApiKey.json"));
    let raw = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "{}{}",
                t(lang, T::Err),
                texts::smoke_unreadable(lang, &path, &e)
            );
            return 1;
        }
    };
    let classified = match classify_keys(&raw, lang) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{}{msg}", t(lang, T::Err));
            return 1;
        }
    };

    for platform in &classified.skipped {
        println!("[{platform}] {}", t(lang, T::SmokeSkipBody));
    }
    for platform in &classified.unknown {
        println!("[{platform}] {}", t(lang, T::SmokeUnknownWarn));
    }

    let vault = match Vault::open(&InMemoryStore::new()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    // 查询走与生产一致的代理设置（settings.json 网络代理端口）
    let default_config = quota_core::AppConfig::default_path().unwrap_or_default();
    let engine = match crate::ctx::build_engine_from_settings(&default_config, lang) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
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
            api_key2_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: proxy,
            console_url: None,
        };
        // CLI 凭据型平台（订阅四家）：key 文件条目只是「要测这个」的
        // 开关（占位值即可），凭据实际来自本机官方 CLI 登录文件
        if !quota_core::provider::uses_cli_credentials(platform)
            && let Err(e) = entry.set_api_key(&vault, key)
        {
            eprintln!("[{platform}] {}{e}", t(lang, T::SmokeEncryptFail));
            failures += 1;
            continue;
        }
        match engine.query(&vault, &entry).await {
            Ok(data) => {
                for d in &data {
                    println!(
                        "[{platform}] OK {} remaining={} used={} unit={} valid={} msg={:?} extra={}",
                        d.plan_name.clone().unwrap_or_default(),
                        d.remaining
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".into()),
                        d.used.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
                        d.unit.clone().unwrap_or_else(|| "-".into()),
                        match d.is_valid {
                            Some(false) => t(lang, T::No),
                            _ => t(lang, T::Yes),
                        },
                        d.invalid_message,
                        d.extra
                            .as_ref()
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "-".into()),
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
        println!("{}", texts::smoke_total_fail(lang, failures));
        1
    } else {
        println!("{}", t(lang, T::SmokeAllPass));
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
        for lang in [Lang::Zh, Lang::En] {
            let c = classify_keys(&raw, lang).unwrap();
            assert_eq!(
                c.runnable,
                vec![("deepseek".to_string(), "sk-1".to_string())]
            );
            assert_eq!(c.skipped, vec!["siliconflow".to_string()]);
            assert_eq!(c.unknown, vec!["no-such-platform".to_string()]);
        }
    }

    /// 契约：坏 JSON 报错（双语文案）。
    #[test]
    fn bad_key_file_rejected() {
        for lang in [Lang::Zh, Lang::En] {
            let err = classify_keys("not json", lang).unwrap_err();
            assert!(
                err.starts_with(t(lang, T::SmokeKeyFileFormat)),
                "{lang:?}: {err}"
            );
        }
    }
}
