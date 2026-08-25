//! `quota set-key`：隐藏输入读取 key，经 vault 加密写入配置。
//!
//! 不接受命令行参数形式的 key（避免进入 shell history）；
//! 管道 stdin 允许（`echo $KEY | quota set-key id`）。

use quota_core::AppConfig;
use quota_core::config::ProviderKind;

use crate::ctx::Ctx;
use crate::io;
use crate::texts::{self, T, t};

pub fn run(ctx: &Ctx, id: String) -> i32 {
    let lang = ctx.lang;
    let mut cfg = match AppConfig::load(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    let Some(entry) = cfg.providers.iter_mut().find(|e| e.id == id) else {
        eprintln!("{}{}", t(lang, T::Err), texts::entry_not_found(lang, &id));
        return 1;
    };
    // CLI 凭据型平台（订阅四家）凭据来自本机官方 CLI，set-key 无意义
    if matches!(&entry.kind, ProviderKind::Native { provider }
        if quota_core::provider::uses_cli_credentials(provider))
    {
        println!("{}", t(lang, T::CliCredentialNote));
        return 0;
    }

    let key = match io::read_secret(t(lang, T::SetKeyPrompt), lang) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{}{}{e}", t(lang, T::Err), t(lang, T::KeyReadFail));
            return 1;
        }
    };
    if key.trim().is_empty() {
        eprintln!("{}{}", t(lang, T::Err), t(lang, T::KeyEmptyRejected));
        return 1;
    }

    let vault = match ctx.open_vault() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    let name = if let Err(e) = entry.set_api_key(&vault, key.trim()) {
        eprintln!("{}{}{e}", t(lang, T::Err), t(lang, T::EncryptFail));
        return 1;
    } else {
        entry.name.clone()
    };
    if let Err(e) = cfg.save(&ctx.config_path) {
        eprintln!("{}{e}", t(lang, T::Err));
        return 1;
    }
    println!("{}", texts::key_updated(lang, &name, &id));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::Ctx;
    use quota_core::InMemoryStore;
    use std::sync::Arc;

    /// 契约：set-key 不存在的 id → 退出 1（在读取 stdin 之前拦截，测试无需喂 key）。
    #[test]
    fn setkey_missing_id_fails_before_input() {
        let dir = std::env::temp_dir().join(format!("quota-cli-sk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = Ctx::with_store(dir.join("config.json"), Arc::new(InMemoryStore::new()));
        assert_eq!(run(&ctx, "zzz".into()), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
