//! quota —— QuotaTray 命令行前端（M2b）。
//!
//! 业务全部在 quota-core，CLI 只做参数解析、结果呈现与配置管理入口。
//! 退出码三分约定见 [`exit`] 模块文档；clap 用法错误维持 Unix 惯例的 2。
//!
//! i18n：语言优先级 `--lang` > settings.json > 系统（见 [`lang`]）；
//! help / 用法错误的文案由 [`texts::apply_help_lang`] 在解析前按选定
//! 语言覆盖——为此 main 采用两阶段解析（先预扫描语言再交 clap）。

mod cmd;
mod ctx;
mod exit;
mod idgen;
mod io;
mod lang;
mod render;
mod settings_io;
mod texts;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use ctx::Ctx;
use lang::Lang;
use std::path::PathBuf;
use texts::{T, t};

/// quota —— 多平台 AI 账户余额监视器的命令行前端
#[derive(Parser, Debug)]
#[command(
    name = "quota",
    version,
    about = "多平台 AI 账户余额监视器的命令行前端"
)]
struct Cli {
    /// 配置文件路径（默认 ~/.quotatray/config.json；不影响 vault 主密钥位置）
    #[arg(short, long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// 界面语言（本次运行覆盖 settings.json；缺省跟随 settings.json / 系统）
    #[arg(long, global = true, value_name = "zh|en|system")]
    lang: Option<Lang>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 列出全部供应商条目及状态
    List {
        /// 输出 providers 的 JSON（含凭据密文字段 api_key_enc，非明文）
        #[arg(long)]
        json: bool,
    },
    /// 查询全部或指定条目
    Query {
        /// 条目 id（缺省 = 全部 enabled 条目）
        ids: Vec<String>,
        /// 输出 JSON（供脚本消费）
        #[arg(long, conflicts_with = "watch")]
        json: bool,
        /// 轮询模式，每轮重绘表格，Ctrl+C 退出
        #[arg(long)]
        watch: bool,
        /// 轮询间隔（分钟），默认 5（仅在 --watch 下有效）
        #[arg(long, value_name = "MINUTES", requires = "watch", value_parser = clap::value_parser!(u64).range(1..))]
        interval: Option<u64>,
    },
    /// 添加供应商（交互向导，或 --json 从 stdin 读入）
    Add {
        /// 从 stdin 读 ProviderEntry 的 JSON（api_key_enc 必须缺省/为空）
        #[arg(long)]
        json: bool,
    },
    /// 编辑条目（向导，回车=保持不变；--enable/--disable 走非交互路径）
    Edit {
        /// 条目 id
        id: String,
        /// 启用条目
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        /// 禁用条目
        #[arg(long)]
        disable: bool,
    },
    /// 删除条目
    Remove {
        /// 条目 id
        id: String,
        /// 跳过确认提示
        #[arg(long)]
        yes: bool,
    },
    /// 写入/更新 API key（星号掩码输入，不进 shell history）
    SetKey {
        /// 条目 id
        id: String,
    },
    /// 列出预置平台
    Natives,
    /// 峰谷定价：查看 / 自定义 / 清除
    #[command(subcommand)]
    Pricing(PricingCmd),
    /// 模板工具
    #[command(subcommand)]
    Template(TemplateCmd),
    /// 凭据保险库
    #[command(subcommand)]
    Vault(VaultCmd),
    /// 检测 GitHub release 新版本，可选下载安装包
    Update {
        /// 只检测不下载
        #[arg(long)]
        check: bool,
        /// 跳过下载确认
        #[arg(long)]
        yes: bool,
        /// 安装包保存目录（默认当前目录）
        #[arg(long, value_name = "DIR")]
        output: Option<PathBuf>,
    },
    /// 真机冒烟（仅 debug 构建，读 .DevApiKey.json 走完整链路）
    #[cfg(debug_assertions)]
    DevSmoke {
        /// key 文件路径（默认当前目录 .DevApiKey.json）
        #[arg(long, value_name = "PATH")]
        key_file: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum PricingCmd {
    /// 查看条目生效峰谷定价（当前判定 + 价格对照 + 时段）
    Show {
        /// 条目 id
        id: String,
        /// 输出 JSON（供脚本消费）
        #[arg(long)]
        json: bool,
    },
    /// 从 stdin 读 PricingConfig JSON 设为自定义（字段级覆盖预置）
    Set {
        /// 条目 id
        id: String,
    },
    /// 清除自定义峰谷定价（回退预置）
    Clear {
        /// 条目 id
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum TemplateCmd {
    /// 模板静态校验 + 真实试查一次
    Test {
        /// 复用已存条目的 key（vault 解密）与 base_url
        #[arg(long, conflicts_with = "json", value_name = "ID")]
        entry: Option<String>,
        /// 从 stdin 读模板 JSON（引用 {{apiKey}} 时经 tty 交互输入 key）
        #[arg(long)]
        json: bool,
        /// 覆盖 baseUrl 变量
        #[arg(long, value_name = "URL")]
        base_url: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum VaultCmd {
    /// 主密钥健康检查（系统凭据库可读性）
    Status,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    // 两阶段解析：先用预扫描的语言（--lang > --config 推导的 settings >
    // 默认路径）覆盖命令面文案，再用**翻译后的 command** 做匹配——
    // DisplayHelp / 用法错误在匹配时即时渲染，必须让 clap 拿到已翻译的 cmd。
    // 注：clap 内置错误骨架（error:/Usage:/For more information…）是库
    // 文案无法翻译，属生态限制，已知悉——可译面仅限 about/help/值解析消息。
    let (scan_lang, scan_config) = lang::scan_args(&args);
    let help_lang = lang::resolve_lang(
        scan_lang,
        scan_config
            .or_else(|| quota_core::AppConfig::default_path().ok())
            .as_deref()
            .unwrap_or(std::path::Path::new("")),
    )
    .resolve();
    let mut cmd = texts::apply_help_lang(Cli::command(), help_lang);

    let cli = match cmd.try_get_matches_from_mut(&args) {
        Ok(matches) => Cli::from_arg_matches(&matches).expect("clap 已校验 matches"),
        Err(err) => {
            // --help/--version 携带渲染结果退出码 0；用法错误为 2（clap 惯例）
            let _ = err.print();
            std::process::exit(err.exit_code());
        }
    };
    std::process::exit(run(cli).await);
}

async fn run(cli: Cli) -> i32 {
    let config_path = match cli.config {
        Some(p) => p,
        None => match quota_core::AppConfig::default_path() {
            Ok(p) => p,
            Err(e) => {
                // 默认路径不可得时语言只能走系统检测（settings 无从推导）
                eprintln!("{}{e}", t(Lang::System.resolve(), T::Err));
                return 1;
            }
        },
    };

    let lang = lang::resolve_lang(cli.lang, &config_path).resolve();
    let ctx = Ctx::production(config_path, lang);

    // 启动更新提示的两个豁免：--json 输出模式（stdout 是机器可读流，
    // 提示只能走 stderr 也会干扰脚本日志）；update 子命令自身（避免重复检测）。
    let json_mode = matches!(
        &cli.command,
        Command::List { json: true }
            | Command::Query { json: true, .. }
            | Command::Add { json: true }
    );
    let is_update_cmd = matches!(&cli.command, Command::Update { .. });

    let code = match cli.command {
        Command::List { json } => cmd::list::run(&ctx, json),
        Command::Query {
            ids,
            json,
            watch,
            interval,
        } => cmd::query::run(&ctx, ids, json, watch, interval).await,
        Command::Add { json } => cmd::add::run(&ctx, json),
        Command::Edit {
            id,
            enable,
            disable,
        } => cmd::edit::run(&ctx, id, enable, disable),
        Command::Remove { id, yes } => cmd::remove::run(&ctx, id, yes),
        Command::SetKey { id } => cmd::setkey::run(&ctx, id),
        Command::Natives => cmd::natives::run(ctx.lang),
        Command::Pricing(PricingCmd::Show { id, json }) => cmd::pricing::run_show(&ctx, &id, json),
        Command::Pricing(PricingCmd::Set { id }) => cmd::pricing::run_set(&ctx, &id),
        Command::Pricing(PricingCmd::Clear { id }) => cmd::pricing::run_clear(&ctx, &id),
        Command::Template(TemplateCmd::Test {
            entry,
            json,
            base_url,
        }) => cmd::template::run(&ctx, entry, json, base_url).await,
        Command::Vault(VaultCmd::Status) => cmd::vault::run(&ctx),
        Command::Update { check, yes, output } => {
            cmd::update::run(
                &ctx,
                cmd::update::UpdateArgs {
                    check_only: check,
                    yes,
                    output,
                },
            )
            .await
        }
        #[cfg(debug_assertions)]
        Command::DevSmoke { key_file } => cmd::devsmoke::run(key_file, ctx.lang).await,
    };

    if !json_mode && !is_update_cmd {
        auto_update_hint(&ctx).await;
    }
    code
}

/// 启动时后台更新提示：与 GUI 同语义的 `due_check` 判定（24h 节流 +
/// 每日到点），5s 超时静默失败，仅 stderr 一行。检测过（含失败）即写回
/// `update_last_check`——否则断网期间每条命令都要白等 5 秒。
async fn auto_update_hint(ctx: &Ctx) {
    use quota_core::update::{self, VERSION};

    let prefs = settings_io::load_prefs(&ctx.config_path);
    let now = settings_io::now_ms();
    if !update::due_check(
        prefs.update_check_enabled,
        prefs.update_last_check,
        &prefs.update_check_time,
        now,
    ) {
        return;
    }
    let Ok(http) = quota_core::http::ReqwestHttpClient::new(std::time::Duration::from_secs(4))
    else {
        return;
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        update::check_update(&http, VERSION),
    )
    .await;
    let _ = settings_io::write_last_check(&ctx.config_path, now);
    if let Ok(Ok(update::UpdateStatus::Available { version, .. })) = result {
        eprintln!("{}", texts::update_hint_available(ctx.lang, &version));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    /// 契约：全部子命令可被解析（命令面快照）。
    #[test]
    fn parses_all_subcommands() {
        for args in [
            vec!["quota", "list"],
            vec!["quota", "list", "--json"],
            vec!["quota", "query"],
            vec!["quota", "query", "a", "b"],
            vec!["quota", "query", "--json"],
            vec!["quota", "query", "--watch"],
            vec!["quota", "query", "--watch", "--interval", "10"],
            vec!["quota", "add"],
            vec!["quota", "add", "--json"],
            vec!["quota", "edit", "x1"],
            vec!["quota", "edit", "x1", "--enable"],
            vec!["quota", "edit", "x1", "--disable"],
            vec!["quota", "remove", "x1"],
            vec!["quota", "remove", "x1", "--yes"],
            vec!["quota", "set-key", "x1"],
            vec!["quota", "natives"],
            vec!["quota", "pricing", "show", "x1"],
            vec!["quota", "pricing", "show", "x1", "--json"],
            vec!["quota", "pricing", "set", "x1"],
            vec!["quota", "pricing", "clear", "x1"],
            vec!["quota", "template", "test"],
            vec!["quota", "template", "test", "--entry", "x1"],
            vec!["quota", "template", "test", "--json"],
            vec![
                "quota",
                "template",
                "test",
                "--entry",
                "x1",
                "--base-url",
                "https://a.com",
            ],
            vec!["quota", "vault", "status"],
            vec!["quota", "--config", "c.json", "list"],
            // --lang 三值（全局参数，可置于子命令前后）
            vec!["quota", "--lang", "zh", "list"],
            vec!["quota", "--lang", "en", "query"],
            vec!["quota", "--lang", "system", "list"],
            vec!["quota", "list", "--lang", "en"],
        ] {
            Cli::try_parse_from(args).unwrap_or_else(|e| panic!("应可解析：{e}"));
        }
        // --version 是特殊错误类别（携带版本信息退出），不算用法错误
        let e = Cli::try_parse_from(["quota", "--version"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::DisplayVersion);
    }

    /// 契约：互斥与非法参数被拒。
    #[test]
    fn rejects_conflicting_args() {
        let e = Cli::try_parse_from(["quota", "query", "--json", "--watch"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ArgumentConflict);

        let e = Cli::try_parse_from(["quota", "edit", "x", "--enable", "--disable"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ArgumentConflict);

        let e = Cli::try_parse_from(["quota", "template", "test", "--entry", "x", "--json"])
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ArgumentConflict);

        let e = Cli::try_parse_from(["quota", "query", "--watch", "--interval", "0"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ValueValidation);

        // --interval 仅在 --watch 下有效
        let e = Cli::try_parse_from(["quota", "query", "--interval", "3"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::MissingRequiredArgument);
        assert!(Cli::try_parse_from(["quota", "query", "--watch", "--interval", "3"]).is_ok());
    }

    /// 契约：--lang 非法值被 clap 拒绝（值解析错误）。
    #[test]
    fn rejects_invalid_lang_value() {
        let e = Cli::try_parse_from(["quota", "--lang", "fr", "list"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ValueValidation);
        assert!(Cli::try_parse_from(["quota", "--lang", "zh", "list"]).is_ok());
    }

    /// 契约：dev-smoke 仅 debug 构建存在。
    #[test]
    fn dev_smoke_availability_follows_build_profile() {
        let parsed = Cli::try_parse_from(["quota", "dev-smoke"]);
        #[cfg(debug_assertions)]
        {
            let cli = parsed.expect("debug 构建应存在 dev-smoke");
            assert!(matches!(cli.command, Command::DevSmoke { .. }));
        }
        #[cfg(not(debug_assertions))]
        {
            assert_eq!(parsed.unwrap_err().kind(), ErrorKind::InvalidSubcommand);
        }
    }

    /// 契约：--config 为全局参数（可置于子命令前）。
    #[test]
    fn config_flag_is_global() {
        let cli = Cli::try_parse_from(["quota", "--config", "/tmp/x.json", "natives"]).unwrap();
        assert_eq!(cli.config.unwrap(), PathBuf::from("/tmp/x.json"));
    }

    /// 契约：--lang 为全局参数，解析为三态值；缺省为 None（走 settings.json）。
    #[test]
    fn lang_flag_parses_three_states() {
        let cli = Cli::try_parse_from(["quota", "--lang", "en", "natives"]).unwrap();
        assert_eq!(cli.lang, Some(Lang::En));

        let cli = Cli::try_parse_from(["quota", "natives", "--lang", "system"]).unwrap();
        assert_eq!(cli.lang, Some(Lang::System));

        let cli = Cli::try_parse_from(["quota", "natives"]).unwrap();
        assert_eq!(cli.lang, None);
    }
}
