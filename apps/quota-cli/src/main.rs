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
use cmd::history::HistoryRange;
use ctx::Ctx;
use lang::Lang;
use std::path::PathBuf;
use texts::{T, t};

/// `--version` 输出：版本 + 目标架构（与 GUI 更新页共用 core 的 arch_label，
/// 两端展示一致；便携形态不在此探测——version 保持零 IO 快速返回）。
fn version_text() -> String {
    format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        quota_core::update::arch_label()
    )
}

/// quota —— 多平台 AI 账户余额监视器的命令行前端
#[derive(Parser, Debug)]
#[command(
    name = "quota",
    version = version_text(),
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
        /// 写入第二凭据槽（{{apiKey2}}，如 new-api 系站点的用户 ID）
        #[arg(long, value_name = "SLOT")]
        slot: Option<u8>,
    },
    /// 列出预置平台
    Natives,
    /// 峰谷定价：查看 / 自定义 / 清除
    #[command(subcommand)]
    Pricing(PricingCmd),
    /// 模板工具
    #[command(subcommand)]
    Template(TemplateCmd),
    /// 脚本工具
    #[command(subcommand)]
    Script(ScriptCmd),
    /// 外部 Agent 调试工具（无凭据、无网络）
    #[command(subcommand)]
    Assist(AssistCmd),
    /// 凭据保险库
    #[command(subcommand)]
    Vault(VaultCmd),
    /// 查询历史数据（余额/额度走势）
    #[command(subcommand)]
    History(HistoryCmd),
    /// 完整配置跨机器迁移
    #[command(subcommand)]
    Config(ConfigCmd),
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
        /// 条目开启查询代理（settings.json 网络代理端口；验证 CLI 凭据型
        /// 平台访问被墙站点时的代理通道）
        #[arg(long)]
        proxy: bool,
    },
}

#[derive(Subcommand, Debug)]
enum AssistCmd {
    /// 输出当前二进制支持的模板/脚本调试契约
    Schema,
    /// 静态校验纯配置或 .qtray-assist.json 诊断包
    Validate {
        #[arg(long, value_enum)]
        mode: cmd::assist::AssistMode,
        #[arg(long, value_name = "PATH")]
        input: PathBuf,
    },
    /// 用脱敏响应样本离线验证取数逻辑
    Simulate {
        #[arg(long, value_enum)]
        mode: cmd::assist::AssistMode,
        #[arg(long, value_name = "PATH")]
        input: PathBuf,
        /// 响应 JSON；缺省时读取诊断包 responseSample
        #[arg(long, value_name = "PATH")]
        response: Option<PathBuf>,
    },
    /// 真实试查一次：复用诊断包 entryId 指向条目的已存凭据（密文不出 vault）
    Test {
        #[arg(long, value_enum)]
        mode: cmd::assist::AssistMode,
        #[arg(long, value_name = "PATH")]
        input: PathBuf,
        /// 覆盖条目 baseUrl（换域试查）
        #[arg(long, value_name = "URL")]
        base_url: Option<String>,
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
    /// 自定义模型库管理（按平台聚类，条目 pricing.model 可选用）
    #[command(subcommand)]
    Model(ModelCmd),
}

#[derive(Subcommand, Debug)]
enum ModelCmd {
    /// 列出平台预置与自定义模型（价格对照）
    List {
        /// native 平台 id（见 quota natives）
        provider: String,
        /// 输出 JSON（供脚本消费）
        #[arg(long)]
        json: bool,
    },
    /// 从 stdin 读 CustomModelDef JSON 添加/覆盖（同 id 覆盖 = 更新）
    Add {
        /// native 平台 id（见 quota natives）
        provider: String,
    },
    /// 删除自定义模型
    Remove {
        /// native 平台 id（见 quota natives）
        provider: String,
        /// 模型 id
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
enum ScriptCmd {
    /// 脚本静态校验（干跑）+ 真实试查一次
    Test {
        /// 复用已存条目的 key（vault 解密）与 base_url
        #[arg(long, conflicts_with = "json", value_name = "ID")]
        entry: Option<String>,
        /// 从 stdin 读脚本配置 JSON（{code, allowInsecure?}；引用 {{apiKey}} 时经 tty 交互输入 key）
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

#[derive(Subcommand, Debug)]
enum HistoryCmd {
    /// 查看条目历史走势（按时间桶聚合）
    Show {
        /// 条目 id
        id: String,
        /// 窗口过滤：5h / weekly 类别、all 全部或窗口键精确匹配；缺省按范围选（24h→5h，7d/30d→周），缺失回退全部
        #[arg(long, value_name = "KEY")]
        window: Option<String>,
        /// 回看范围与聚合粒度（默认 7d）
        #[arg(long, value_enum, default_value = "7d")]
        range: HistoryRange,
        /// 每页行数（默认 20）
        #[arg(long, value_name = "ROWS", value_parser = clap::value_parser!(u64).range(1..=500))]
        page_size: Option<u64>,
        /// 打印指定页后退出（非交互；缺省且在终端下交互翻页）
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..), conflicts_with = "json")]
        page: Option<u64>,
        /// 输出原始点 JSON（不分页不聚合，供脚本消费）
        #[arg(long)]
        json: bool,
    },
    /// 清除历史数据
    Clear {
        /// 条目 id（缺省 = 全部条目）
        id: Option<String>,
        /// 跳过确认
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCmd {
    /// 导出完整配置与凭据到私有迁移包
    Export {
        /// 迁移包输出路径
        #[arg(value_name = "PATH")]
        output: PathBuf,
        /// 跳过敏感文件确认
        #[arg(long)]
        yes: bool,
    },
    /// 从迁移包整体替换当前配置
    Import {
        /// 迁移包输入路径
        #[arg(value_name = "PATH")]
        input: PathBuf,
        /// 跳过整体替换确认
        #[arg(long)]
        yes: bool,
    },
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
            | Command::History(HistoryCmd::Show { json: true, .. })
            | Command::Assist(_)
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
        Command::SetKey { id, slot } => cmd::setkey::run(&ctx, id, slot),
        Command::Natives => cmd::natives::run(ctx.lang),
        Command::Pricing(PricingCmd::Show { id, json }) => cmd::pricing::run_show(&ctx, &id, json),
        Command::Pricing(PricingCmd::Set { id }) => cmd::pricing::run_set(&ctx, &id),
        Command::Pricing(PricingCmd::Clear { id }) => cmd::pricing::run_clear(&ctx, &id),
        Command::Pricing(PricingCmd::Model(ModelCmd::List { provider, json })) => {
            cmd::pricing_models::run_list(&ctx, &provider, json)
        }
        Command::Pricing(PricingCmd::Model(ModelCmd::Add { provider })) => {
            cmd::pricing_models::run_add(&ctx, &provider)
        }
        Command::Pricing(PricingCmd::Model(ModelCmd::Remove { provider, id })) => {
            cmd::pricing_models::run_remove(&ctx, &provider, &id)
        }
        Command::Template(TemplateCmd::Test {
            entry,
            json,
            base_url,
        }) => cmd::template::run(&ctx, entry, json, base_url).await,
        Command::Script(ScriptCmd::Test {
            entry,
            json,
            base_url,
        }) => cmd::script::run(&ctx, entry, json, base_url).await,
        Command::Assist(AssistCmd::Schema) => cmd::assist::run_schema(),
        Command::Assist(AssistCmd::Validate { mode, input }) => {
            cmd::assist::run_validate(mode, input)
        }
        Command::Assist(AssistCmd::Simulate {
            mode,
            input,
            response,
        }) => cmd::assist::run_simulate(mode, input, response),
        Command::Assist(AssistCmd::Test {
            mode,
            input,
            base_url,
        }) => cmd::assist::run_test(&ctx, mode, input, base_url).await,
        Command::Vault(VaultCmd::Status) => cmd::vault::run(&ctx),
        Command::History(HistoryCmd::Show {
            id,
            window,
            range,
            page_size,
            page,
            json,
        }) => cmd::history::run_show(&ctx, id, window, range, page_size, page, json),
        Command::History(HistoryCmd::Clear { id, yes }) => cmd::history::run_clear(&ctx, id, yes),
        Command::Config(ConfigCmd::Export { output, yes }) => {
            cmd::config::run_export(&ctx, output, yes)
        }
        Command::Config(ConfigCmd::Import { input, yes }) => {
            cmd::config::run_import(&ctx, input, yes)
        }
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
        Command::DevSmoke { key_file, proxy } => {
            cmd::devsmoke::run(key_file, proxy, ctx.lang).await
        }
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
    // 与 `quota update` 子命令同口径：代理端口读 settings.json（GUI
    // 设置页写入），保证两入口对 GitHub 的出口一致
    let proxy = update::proxy_url_of(prefs.update_proxy_port);
    let Ok(http) = quota_core::http::ReqwestHttpClient::new_with_proxy(
        std::time::Duration::from_secs(4),
        proxy.as_deref(),
    ) else {
        return;
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        update::check_update(&http, VERSION, update::AssetSelector::installed()),
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
            vec!["quota", "history", "show", "x1"],
            vec!["quota", "history", "show", "x1", "--json"],
            vec!["quota", "history", "show", "x1", "--range", "24h"],
            vec!["quota", "history", "show", "x1", "--range", "7d"],
            vec!["quota", "history", "show", "x1", "--range", "30d"],
            vec!["quota", "history", "show", "x1", "--window", "five_hour"],
            vec!["quota", "history", "show", "x1", "--window", "weekly"],
            vec!["quota", "history", "show", "x1", "--window", "all"],
            vec!["quota", "history", "show", "x1", "--page-size", "50"],
            vec!["quota", "history", "show", "x1", "--page", "2"],
            vec!["quota", "history", "clear"],
            vec!["quota", "history", "clear", "x1"],
            vec!["quota", "history", "clear", "x1", "--yes"],
            vec!["quota", "config", "export", "backup.qtray-export"],
            vec!["quota", "config", "export", "backup.qtray-export", "--yes"],
            vec!["quota", "config", "import", "backup.qtray-export"],
            vec!["quota", "config", "import", "backup.qtray-export", "--yes"],
            vec!["quota", "--config", "c.json", "list"],
            // --lang 三值（全局参数，可置于子命令前后）
            vec!["quota", "--lang", "zh", "list"],
            vec!["quota", "--lang", "en", "query"],
            vec!["quota", "--lang", "system", "list"],
            vec!["quota", "list", "--lang", "en"],
        ] {
            Cli::try_parse_from(args).unwrap_or_else(|e| panic!("应可解析：{e}"));
        }
        // --version 是特殊错误类别（携带版本信息退出），不算用法错误；
        // 输出携带平台标签（与 GUI 更新页共用 core arch_label）
        let e = Cli::try_parse_from(["quota", "--version"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::DisplayVersion);
        assert!(
            e.to_string().contains(quota_core::update::arch_label()),
            "--version 输出应包含平台标签"
        );
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

        // history：非法范围档 / 页容量与页码下界 / --page 与 --json 互斥
        let e =
            Cli::try_parse_from(["quota", "history", "show", "x1", "--range", "3d"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidValue);
        let e = Cli::try_parse_from(["quota", "history", "show", "x1", "--page-size", "0"])
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ValueValidation);
        let e = Cli::try_parse_from(["quota", "history", "show", "x1", "--page", "0"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ValueValidation);
        let e = Cli::try_parse_from(["quota", "history", "show", "x1", "--page", "1", "--json"])
            .unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ArgumentConflict);

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
