//! 终端交互薄层：隐藏读 key、多行 JSON 粘贴读取。
//!
//! 纯 IO，不含业务判定（可测逻辑下沉到调用方）；
//! key 一律经 [`Zeroizing`] 包装，drop 时擦除内存。

use std::io::{BufRead, Write};

use zeroize::Zeroizing;

/// 隐藏回显读取一行密钥。`prompt` 完整文案由调用方给出（含"输入不回显"提示）。
///
/// stdin 为终端时经 rpassword 关闭回显；管道/重定向场景
/// （`echo $KEY | quota set-key id`，spec §3）直接读一行——
/// rpassword 在 Windows 上固定从控制台读，管道下会死等键盘，
/// 故先探测 stdin 形态再选读取方式。
/// 返回值已 trim（粘贴误差常见的首尾空白不进入密文）。
pub fn read_secret(prompt: &str) -> std::io::Result<Zeroizing<String>> {
    use std::io::IsTerminal;

    let mut err = std::io::stderr();
    writeln!(err, "{prompt}").ok();
    err.flush().ok();

    let line = if std::io::stdin().is_terminal() {
        rpassword::read_password()?
    } else {
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        buf
    };
    Ok(Zeroizing::new(line.trim().to_string()))
}

/// 多行读取 JSON 文本，直到单个空行或 EOF。
///
/// 两种用法等价：交互粘贴（空行结束）、`< file` 重定向（EOF 结束）。
/// 返回累积的原始文本（不含结束空行）。
pub fn read_multiline_json(prompt: &str) -> std::io::Result<String> {
    // 提示走 stderr：数据流（stdout）保持纯净，管道/重定向场景不被污染
    eprintln!("{prompt}");
    eprintln!("（粘贴多行 JSON，输入单独空行结束；或 Ctrl+Z / Ctrl+D 结束输入）");

    let mut buf = String::new();
    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    loop {
        let mut line = String::new();
        let n = locked.read_line(&mut line)?;
        if n == 0 || line.trim().is_empty() {
            break;
        }
        buf.push_str(&line);
    }
    Ok(buf)
}
