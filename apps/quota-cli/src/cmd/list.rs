//! `quota list`：列出全部供应商条目及状态。

use quota_core::AppConfig;

use crate::ctx::Ctx;
use crate::render;
use crate::texts::{T, t};

pub fn run(ctx: &Ctx, json: bool) -> i32 {
    let lang = ctx.lang;
    let cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    if json {
        // spec §3：--json 输出 AppConfig 的 providers 数组
        println!(
            "{}",
            serde_json::to_string_pretty(&cfg.providers).unwrap_or_default()
        );
        return 0;
    }
    if cfg.providers.is_empty() {
        println!("{}", t(lang, T::ListEmpty));
        return 0;
    }
    println!("{}", render::list_table(&cfg.providers, lang));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use quota_core::PlanVariant;
    use quota_core::config::{ProviderEntry, ProviderKind};

    /// 契约：空配置打印引导文案、退出 0。
    #[test]
    fn empty_config_prints_hint() {
        let dir = std::env::temp_dir().join(format!("quota-cli-list-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json"); // 不存在 → 空配置
        let ctx = Ctx::with_store(path, std::sync::Arc::new(quota_core::InMemoryStore::new()));
        assert_eq!(run(&ctx, false), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 契约：--json 输出即为 providers 的 serde 序列化（含 kind tag）。
    #[test]
    fn json_output_is_providers_array() {
        let entry = ProviderEntry {
            id: "abc234".into(),
            name: "DS".into(),
            kind: ProviderKind::Native {
                provider: "deepseek".into(),
            },
            enabled: true,
            api_key_enc: None,
            base_url: None,
            pricing: None,
            plan_variant: PlanVariant::Auto,
            use_proxy: false,
        };
        let j = serde_json::to_value(&[entry]).unwrap();
        assert_eq!(j[0]["kind"]["type"], "native");
        assert_eq!(j[0]["kind"]["provider"], "deepseek");
    }
}
