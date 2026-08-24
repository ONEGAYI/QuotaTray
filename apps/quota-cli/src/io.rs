//! 终端交互薄层：掩码读 key、多行 JSON 粘贴读取。
//!
//! 纯 IO，不含业务判定（可测逻辑下沉到调用方）；
//! key 一律经 [`Zeroizing`] 包装，drop 时擦除内存。
//! 文案由调用方传语言（prompt 文本 + 内部错误消息均走文案表）。

use std::io::{BufRead, IsTerminal, Write};

use console::{Key, Term};
use zeroize::Zeroizing;

use crate::lang::Lang;
use crate::texts::{T, t};

/// 掩码读取一行密钥：终端输入逐字符回显 `*`（长度可见，内容不可见——
/// 红线约束的是明文不进回显，掩码字符不属于 key 材料）。
///
/// dialoguer 的 `Password` 没有输入期掩码回显（其 `report` 是"完成后
/// 报告"语义），故用 console `read_key` 自实现逐字符循环。
/// 管道/重定向场景（`echo $KEY | quota set-key id`，spec §3）直接读
/// 一行——管道下无"看着屏幕输入"的掩码需求。
/// 返回值已 trim（粘贴误差常见的首尾空白不进入密文）。
pub fn read_secret(prompt: &str, lang: Lang) -> std::io::Result<Zeroizing<String>> {
    if std::io::stdin().is_terminal() {
        read_secret_masked(prompt, lang)
    } else {
        let mut err = std::io::stderr();
        writeln!(err, "{prompt}").ok();
        err.flush().ok();
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        Ok(Zeroizing::new(buf.trim().to_string()))
    }
}

/// 终端掩码输入：字符 → `*`，退格删星号，回车确认，Ctrl+C/Ctrl+D 中止；
/// Ctrl+V 代读剪贴板完成粘贴（raw 模式下 Ctrl+V 到达程序是控制字符
/// SYN，PSReadLine 那种应用层粘贴不存在——这里自己补上）。
fn read_secret_masked(prompt: &str, lang: Lang) -> std::io::Result<Zeroizing<String>> {
    let term = Term::stderr();
    if !term.is_term() {
        return Err(std::io::Error::other(t(lang, T::NotTerminal)));
    }
    eprint!("{prompt}: ");
    std::io::stderr().flush().ok();

    let mut chars: Vec<char> = Vec::new();
    loop {
        match term.read_key()? {
            Key::Char('\u{3}') => {
                term.write_line("")?;
                return Err(std::io::Error::other(t(lang, T::InterruptCtrlC)));
            }
            Key::Char('\u{4}') if chars.is_empty() => {
                term.write_line("")?;
                return Err(std::io::Error::other(t(lang, T::InterruptCtrlD)));
            }
            Key::Char('\u{16}') => {
                if let Err(msg) = paste_clipboard(&term, &mut chars, lang) {
                    eprint!("\n{} ", crate::texts::clipboard_fail(lang, &msg));
                }
            }
            Key::Char(c) => {
                chars.push(c);
                term.write_str("*")?;
            }
            Key::Backspace => {
                if chars.pop().is_some() {
                    term.clear_chars(1)?;
                }
            }
            Key::Enter => break,
            _ => {}
        }
    }
    term.write_line("")?;
    let raw: String = chars.into_iter().collect();
    Ok(Zeroizing::new(raw.trim().to_string()))
}

/// 读剪贴板并作为粘贴内容输入（取首行，到换行即止）。
/// 返回粘贴的字符数；失败原因透出给调用方提示。
fn paste_clipboard(term: &Term, chars: &mut Vec<char>, lang: Lang) -> Result<usize, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("{}{e}", t(lang, T::ClipboardOpenFail)))?;
    let text = clipboard
        .get_text()
        .map_err(|e| format!("{}{e}", t(lang, T::ClipboardReadFail)))?;
    let mut n = 0;
    for c in text.chars() {
        if c == '\r' || c == '\n' {
            break;
        }
        chars.push(c);
        term.write_str("*").ok();
        n += 1;
    }
    Ok(n)
}

/// 多行读取文本（JSON 模板 / JS 脚本代码共用）。
///
/// 交互终端：单个空行结束（粘贴场景的用户无法便捷发 EOF）；
/// 管道/重定向：读到 EOF——**空行不终止**（文件内的空行是内容一部分，
/// JS 代码与美化 JSON 常含空行，截断会静默丢掉后半段）。
pub fn read_multiline_json(prompt: &str, lang: Lang) -> std::io::Result<String> {
    // 提示走 stderr：数据流（stdout）保持纯净，管道/重定向场景不被污染
    eprintln!("{prompt}");
    if std::io::stdin().is_terminal() {
        eprintln!("{}", t(lang, T::MultilineJsonHint));
    }

    let interactive = std::io::stdin().is_terminal();
    let mut buf = String::new();
    let stdin = std::io::stdin();
    let mut locked = stdin.lock();
    loop {
        let mut line = String::new();
        let n = locked.read_line(&mut line)?;
        if n == 0 || (interactive && line.trim().is_empty()) {
            break;
        }
        buf.push_str(&line);
    }
    Ok(buf)
}
