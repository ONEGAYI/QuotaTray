//! `quota script test`：脚本静态校验（干跑）+ 真实试查一次。
//!
//! key 来源与 template test 同约定：`--entry` 复用已存条目（密文零解密）；
//! `--json` 模式从 stdin 读脚本配置 JSON（`{code, allowInsecure?}`），
//! 代码引用 `{{apiKey}}` 时经 tty 交互输入（仅本次测试，不落盘）。

use quota_core::AppConfig;
use quota_core::config::{PlanVariant, ProviderEntry, ProviderKind};
use quota_core::script::{self, ScriptConfig};
use zeroize::Zeroizing;

use crate::cmd::template::{exit_code_for, print_usage};
use crate::ctx::Ctx;
use crate::io;
use crate::lang::Lang;
use crate::texts::{self, T, t};

/// stdin 模式试查用临时条目 id（不落盘，仅构造引擎入参）。
const TEST_ENTRY_ID: &str = "script-test";

/// 试查输入：`--entry` 沿用源条目（密文零解密）；`--json` 的 key 在
/// 校验通过后按需收集（管道场景至少能看到校验结果）。
enum TestSource {
    Entry(Box<ProviderEntry>),
    Stdin {
        config: Box<ScriptConfig>,
        base_url: Option<String>,
    },
}

pub async fn run(
    ctx: &Ctx,
    entry_id: Option<String>,
    json_mode: bool,
    base_url_override: Option<String>,
) -> i32 {
    let lang = ctx.lang;
    // 1. 收集试查输入
    let source = if let Some(id) = entry_id {
        match build_from_entry(ctx, &id, base_url_override) {
            Ok(s) => s,
            Err(msg) => {
                eprintln!("{}{msg}", t(lang, T::Err));
                return 1;
            }
        }
    } else if json_mode {
        match build_from_stdin(base_url_override, lang) {
            Ok(s) => s,
            Err(msg) => {
                eprintln!("{}{msg}", t(lang, T::Err));
                return 1;
            }
        }
    } else {
        eprintln!("{}{}", t(lang, T::Err), t(lang, T::NeedEntryOrJson));
        return 1;
    };

    // 2. 静态校验（干跑：假变量替换 + request() 产物形状，不发 HTTP）
    let cfg_ref = match &source {
        TestSource::Entry(e) => match &e.kind {
            ProviderKind::Script(s) => s.as_ref(),
            _ => unreachable!("build_from_entry 已保证 script 类型"),
        },
        TestSource::Stdin { config, .. } => config,
    };
    if let Err(e) = script::validate(cfg_ref) {
        eprintln!("{}{e}", t(lang, T::ScriptValidateFail));
        return 1;
    }
    println!("{}", t(lang, T::StaticCheckOk));

    // 3. 真实试查（走引擎完整链路：沙箱 + mock 不可用的生产 HTTP）
    let engine = match ctx.new_engine() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    let vault = match ctx.open_vault() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}{e}", t(lang, T::Err));
            return 1;
        }
    };
    let test_entry = match source {
        TestSource::Entry(e) => *e,
        TestSource::Stdin { config, base_url } => {
            // 校验已过；引用 {{apiKey}} 时交互收 key（stdin 被重定向占用时
            // 无法交互，错误文案引导改走 --entry）
            let api_key = if script::uses_api_key(&config) {
                let k = match io::read_secret(t(lang, T::NeedsKeyPrompt), lang) {
                    Ok(k) => k,
                    Err(e) => {
                        eprintln!("{}{}{e}", t(lang, T::Err), t(lang, T::KeyReadFail));
                        return 1;
                    }
                };
                if k.trim().is_empty() {
                    eprintln!("{}{}", t(lang, T::Err), t(lang, T::KeyEmptyHint));
                    return 1;
                }
                k
            } else {
                Zeroizing::new(String::new())
            };
            let mut e = ProviderEntry {
                id: TEST_ENTRY_ID.into(),
                name: t(lang, T::ScriptTestEntryName).into(),
                kind: ProviderKind::Script(config),
                enabled: true,
                api_key_enc: None,
                base_url,
                pricing: None,
                plan_variant: PlanVariant::Auto,
            };
            if let Err(err) = e.set_api_key(&vault, api_key.trim()) {
                eprintln!(
                    "{}{}{err}",
                    t(lang, T::Err),
                    t(lang, T::TryQueryEncryptFail)
                );
                return 1;
            }
            e
        }
    };

    match engine.query(&vault, &test_entry).await {
        Ok(rows) => {
            print_usage(&rows, lang);
            0
        }
        Err(e) => {
            eprintln!("{}{}{e}", t(lang, T::Err), t(lang, T::TryQueryFail));
            exit_code_for(&e)
        }
    }
}

/// `--entry`：直接沿用源条目（含密文与 id，AAD 匹配零解密）。
fn build_from_entry(
    ctx: &Ctx,
    id: &str,
    base_url_override: Option<String>,
) -> Result<TestSource, String> {
    let lang = ctx.lang;
    let cfg = AppConfig::load(&ctx.config_path).map_err(|e| format!("{}{e}", t(lang, T::Err)))?;
    let entry = cfg
        .providers
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| texts::entry_not_found(lang, id))?;
    if !matches!(entry.kind, ProviderKind::Script(_)) {
        return Err(texts::not_script_entry(lang, id));
    }
    let mut test_entry = entry.clone();
    if let Some(b) = base_url_override {
        test_entry.base_url = Some(b);
    }
    Ok(TestSource::Entry(Box::new(test_entry)))
}

/// `--json`：stdin 双形态宽容解析——先按脚本配置 JSON（`{code, allowInsecure?}`），
/// 失败则整段文本视为纯 JS 代码（examples/scripts/ 的 .js 文件可直接重定向试查）。
fn build_from_stdin(base_url_override: Option<String>, lang: Lang) -> Result<TestSource, String> {
    let text = io::read_multiline_json(t(lang, T::PasteScriptCode), lang)
        .map_err(|e| format!("{}{e}", t(lang, T::StdinReadFail)))?;
    let config = serde_json::from_str::<ScriptConfig>(&text).unwrap_or(ScriptConfig {
        code: text,
        allow_insecure: false,
    });
    Ok(TestSource::Stdin {
        config: Box::new(config),
        base_url: base_url_override,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(code: &str) -> ScriptConfig {
        ScriptConfig {
            code: code.into(),
            allow_insecure: false,
        }
    }

    /// 契约：`--json` 输入双形态——配置 JSON（`{code, allowInsecure?}`）与
    /// 纯 JS 代码文本都可被收（examples/scripts/ 的 .js 文件直接重定向）。
    #[test]
    fn stdin_accepts_config_json_and_plain_js() {
        let raw = serde_json::json!({
            "code": "function request(){ return { url: \"https://a.com\" }; } function extract(r){ return { remaining: 1 }; }",
            "allowInsecure": false
        })
        .to_string();
        let cfg: ScriptConfig = serde_json::from_str(&raw).unwrap();
        assert!(!cfg.allow_insecure);
        assert!(cfg.code.contains("request"));

        // 纯 JS 文本不是合法 JSON → 回退为整段 code
        let plain = "function request(){ return { url: \"https://a.com\" }; }\nfunction extract(r){ return { remaining: 1 }; }";
        let cfg = serde_json::from_str::<ScriptConfig>(plain).unwrap_or(ScriptConfig {
            code: plain.to_string(),
            allow_insecure: false,
        });
        assert_eq!(cfg.code, plain);
        assert!(!cfg.allow_insecure);
    }

    /// 契约：uses_api_key 联动 key 收集（{{apiKey}} 占位）。
    #[test]
    fn detects_api_key_usage() {
        assert!(script::uses_api_key(&config(
            "function request(){ return { url: \"https://a.com?key={{apiKey}}\" }; }"
        )));
        assert!(!script::uses_api_key(&config("function request(){}")));
    }
}
