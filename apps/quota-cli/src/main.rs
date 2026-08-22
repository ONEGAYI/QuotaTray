//! quota —— QuotaTray 命令行前端（M2b）。
//!
//! 业务全部在 quota-core，CLI 只做参数解析、结果呈现与配置管理入口。
//! 退出码三分约定见 [`exit`] 模块文档；clap 用法错误维持 Unix 惯例的 2。

mod cmd;
mod ctx;
mod exit;
mod idgen;
mod io;
mod render;

use clap::{Parser, Subcommand};
use ctx::Ctx;
use std::path::PathBuf;

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
    /// 写入/更新 API key（隐藏输入，不进 shell history）
    SetKey {
        /// 条目 id
        id: String,
    },
    /// 列出预置平台
    Natives,
    /// 模板工具
    #[command(subcommand)]
    Template(TemplateCmd),
    /// 凭据保险库
    #[command(subcommand)]
    Vault(VaultCmd),
    /// 真机冒烟（仅 debug 构建，读 .DevApiKey.json 走完整链路）
    #[cfg(debug_assertions)]
    DevSmoke {
        /// key 文件路径（默认当前目录 .DevApiKey.json）
        #[arg(long, value_name = "PATH")]
        key_file: Option<PathBuf>,
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
    let cli = Cli::parse();
    std::process::exit(run(cli).await);
}

async fn run(cli: Cli) -> i32 {
    let config_path = match cli.config {
        Some(p) => p,
        None => match quota_core::AppConfig::default_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("错误：{e}");
                return 1;
            }
        },
    };
    let ctx = Ctx::production(config_path);

    match cli.command {
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
        Command::Natives => cmd::natives::run(),
        Command::Template(TemplateCmd::Test {
            entry,
            json,
            base_url,
        }) => cmd::template::run(&ctx, entry, json, base_url).await,
        Command::Vault(VaultCmd::Status) => cmd::vault::run(&ctx),
        #[cfg(debug_assertions)]
        Command::DevSmoke { key_file } => cmd::devsmoke::run(key_file).await,
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
}
