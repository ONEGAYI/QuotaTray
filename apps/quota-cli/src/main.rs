use clap::Parser;

/// quota —— QuotaTray 命令行前端
#[derive(Parser)]
#[command(
    name = "quota",
    version,
    about = "多平台 AI 账户余额监视器的命令行前端"
)]
struct Cli {
    // M2 填充子命令：list / query / add / remove / edit / template / script / vault
}

fn main() {
    let _cli = Cli::parse();
    println!(
        "quota {} (QuotaTray CLI) —— 子命令将于 M2 提供",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 烟测：空参数可正常解析（子命令于 M2 加入后此测试随之收紧）。
    #[test]
    fn parses_bare_invocation() {
        Cli::try_parse_from(["quota"]).expect("bare invocation should parse");
    }
}
