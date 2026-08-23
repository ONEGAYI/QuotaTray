//! 语言三态（zh / en / system）与解析。
//!
//! 来源优先级：`--lang` 参数 > settings.json 的 `language` 字段 > System。
//! `System` 是"跟随系统"占位，展示前须经 [`Lang::resolve`] 消解；
//! 文案表 [`crate::texts::t`] 对未消解的 `System` 按中文兜底
//!（本项目默认语言为中文，与 GUI 的 settings 默认一致）。
//!
//! settings.json 由桌面端拥有（字段会并行扩展 theme 等），CLI 侧只以
//! mini struct 提取 `language` 单字段，serde 默认容忍未知字段，
//! 文件缺失/损坏/字段缺失/非法值一律回退 System（容错优先，不阻断）。

use std::path::{Path, PathBuf};

/// 用户语言偏好三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    /// 跟随系统 locale（`zh*` 前缀 → Zh，否则 En）。
    #[default]
    System,
    Zh,
    En,
}

impl Lang {
    /// 解析配置值：`zh` | `en` | `system`（ASCII 大小写不敏感），其余 None。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "zh" => Some(Self::Zh),
            "en" => Some(Self::En),
            "system" => Some(Self::System),
            _ => None,
        }
    }

    /// 消解 `System`：读取系统 locale 折叠为具体语言；其余原样返回。
    pub fn resolve(self) -> Self {
        match self {
            Self::System => Self::from_locale_tag(&sys_locale::get_locale().unwrap_or_default()),
            other => other,
        }
    }

    /// locale tag 折叠（纯函数，可离线测试）：
    /// `zh*` 前缀（zh / zh-CN / zh-Hant…）→ Zh，其余（含空）→ En。
    pub fn from_locale_tag(tag: &str) -> Self {
        if tag.trim().to_ascii_lowercase().starts_with("zh") {
            Self::Zh
        } else {
            Self::En
        }
    }
}

impl std::str::FromStr for Lang {
    type Err = String;

    /// 错误串用英文：clap 的值解析错误骨架（error:/invalid value …）
    /// 是库文案无法翻译，混排中文反而不一致——纯英文与骨架统一。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| "invalid value (valid: zh | en | system)".into())
    }
}

/// settings.json 的语言字段提取（仅 `language` 单字段；
/// 桌面端并行扩展 theme 等字段时 CLI 不受影响）。
#[derive(serde::Deserialize)]
struct SettingsLang {
    #[serde(default)]
    language: String,
}

/// 读 settings.json（与 config.json 同目录）的 `language` 偏好。
///
/// 文件缺失 / 损坏 / 字段缺失 / 非法值一律回退 [`Lang::System`]。
pub fn lang_from_settings(config_path: &Path) -> Lang {
    let Some(dir) = config_path.parent() else {
        return Lang::System;
    };
    let text = std::fs::read_to_string(dir.join("settings.json")).unwrap_or_default();
    let settings: SettingsLang = serde_json::from_str(&text).unwrap_or(SettingsLang {
        language: String::new(),
    });
    Lang::parse(&settings.language).unwrap_or_default()
}

/// 语言优先级合并：`--lang` 参数 > settings.json > System。
pub fn resolve_lang(cli_lang: Option<Lang>, config_path: &Path) -> Lang {
    cli_lang.unwrap_or_else(|| lang_from_settings(config_path))
}

/// 从原始 argv 预提取 `--lang` 与 `--config`（best effort，含 `--x=v` 形式）。
///
/// 供 clap 解析**之前**决定 help / 用法错误的语言：全局参数可出现在
/// 子命令之后，故顺序扫描全部 token。严格解析仍由 clap 完成，此处
/// 仅影响错误/help 渲染语言，对未知形态不做报错。
pub fn scan_args(args: &[String]) -> (Option<Lang>, Option<PathBuf>) {
    let mut lang = None;
    let mut config = None;
    let mut iter = args.iter().skip(1); // 跳过程序名
    while let Some(a) = iter.next() {
        if let Some(v) = a.strip_prefix("--lang=") {
            lang = Lang::parse(v);
        } else if a == "--lang" {
            if let Some(v) = iter.next() {
                lang = Lang::parse(v);
            }
        } else if let Some(v) = a.strip_prefix("--config=") {
            config = Some(PathBuf::from(v));
        } else if a == "--config" || a == "-c" {
            if let Some(v) = iter.next() {
                config = Some(PathBuf::from(v));
            }
        }
    }
    (lang, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：parse 接受 zh/en/system（大小写与首尾空白不敏感），拒绝其余。
    #[test]
    fn parses_known_values() {
        assert_eq!(Lang::parse("zh"), Some(Lang::Zh));
        assert_eq!(Lang::parse("EN"), Some(Lang::En));
        assert_eq!(Lang::parse(" system "), Some(Lang::System));
        assert_eq!(Lang::parse("fr"), None);
        assert_eq!(Lang::parse(""), None);
        assert_eq!(Lang::parse("中文"), None);
    }

    /// 契约：locale 折叠——zh 前缀 → Zh，其余（含空/None 形态）→ En。
    #[test]
    fn folds_locale_tags() {
        assert_eq!(Lang::from_locale_tag("zh-CN"), Lang::Zh);
        assert_eq!(Lang::from_locale_tag("zh-Hant-TW"), Lang::Zh);
        assert_eq!(Lang::from_locale_tag("ZH"), Lang::Zh);
        assert_eq!(Lang::from_locale_tag("en-US"), Lang::En);
        assert_eq!(Lang::from_locale_tag("ja-JP"), Lang::En);
        assert_eq!(Lang::from_locale_tag(""), Lang::En);
        assert_eq!(Lang::from_locale_tag("  "), Lang::En);
    }

    /// 契约：resolve 直通非 System 值（不触系统 locale，测试可离线稳定）。
    #[test]
    fn resolve_passes_through_concrete() {
        assert_eq!(Lang::Zh.resolve(), Lang::Zh);
        assert_eq!(Lang::En.resolve(), Lang::En);
    }

    /// 契约：--lang 非法值的 FromStr 错误文案为纯英文——clap 错误骨架
    /// （error:/invalid value…）是库文案不可译，混排中文反而不一致。
    #[test]
    fn from_str_error_message_is_english() {
        let err = "fr".parse::<Lang>().unwrap_err();
        assert_eq!(err, "invalid value (valid: zh | en | system)");
        assert!(!err.chars().any(|c| c > '\u{7F}'), "不得含非 ASCII：{err}");
        // 合法值照常解析
        assert_eq!("en".parse::<Lang>().unwrap(), Lang::En);
    }

    /// 契约：settings 读取容错——缺失/损坏/缺字段/非法值回退 System；
    /// 合法值与桌面端并行新增的未知字段（theme 等）共存时正常提取。
    #[test]
    fn settings_language_tolerates_damage() {
        let dir = std::env::temp_dir().join(format!("quota-cli-lang-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.json");
        let settings = dir.join("settings.json");

        // 无文件 → System
        assert_eq!(lang_from_settings(&cfg), Lang::System);
        // 损坏 JSON → System
        std::fs::write(&settings, "{ not json").unwrap();
        assert_eq!(lang_from_settings(&cfg), Lang::System);
        // 缺 language 字段（桌面端并行写入的其他字段）→ System
        std::fs::write(&settings, r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(lang_from_settings(&cfg), Lang::System);
        // 非法值 → System（跟随系统比强制语言更友好）
        std::fs::write(&settings, r#"{"language":"fr"}"#).unwrap();
        assert_eq!(lang_from_settings(&cfg), Lang::System);
        // 合法值 + 未知字段共存 → 正常提取
        std::fs::write(&settings, r#"{"language":"en","theme":"dark"}"#).unwrap();
        assert_eq!(lang_from_settings(&cfg), Lang::En);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 契约：优先级——--lang 参数 > settings.json > System。
    #[test]
    fn cli_lang_takes_precedence_over_settings() {
        let dir = std::env::temp_dir().join(format!("quota-cli-langp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"language":"en"}"#).unwrap();
        let cfg = dir.join("config.json");

        assert_eq!(resolve_lang(None, &cfg), Lang::En);
        assert_eq!(resolve_lang(Some(Lang::Zh), &cfg), Lang::Zh);
        // 显式 --lang system 覆盖 settings，仍跟随系统
        assert_eq!(resolve_lang(Some(Lang::System), &cfg), Lang::System);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 契约：argv 预扫描——--lang/--config 的两种形态、子命令后全局位置。
    #[test]
    fn scans_lang_and_config_from_argv() {
        let (lang, config) =
            scan_args(&["quota".into(), "list".into(), "--lang".into(), "en".into()]);
        assert_eq!(lang, Some(Lang::En));
        assert_eq!(config, None);

        let (lang, _) = scan_args(&["quota".into(), "--lang=zh".into(), "query".into()]);
        assert_eq!(lang, Some(Lang::Zh));

        let (lang, config) = scan_args(&[
            "quota".into(),
            "--config".into(),
            "/tmp/c.json".into(),
            "--lang=system".into(),
        ]);
        assert_eq!(lang, Some(Lang::System));
        assert_eq!(config, Some(PathBuf::from("/tmp/c.json")));

        let (lang, config) = scan_args(&[
            "quota".into(),
            "-c".into(),
            "x.json".into(),
            "natives".into(),
        ]);
        assert_eq!(lang, None);
        assert_eq!(config, Some(PathBuf::from("x.json")));

        // 非法 --lang 值：预扫描置 None（严格报错留给 clap）
        let (lang, _) = scan_args(&["quota".into(), "--lang".into(), "fr".into()]);
        assert_eq!(lang, None);
    }
}
