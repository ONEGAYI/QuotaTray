//! 终端交互薄层：掩码读 key、多行 JSON 粘贴读取。
//!
//! 纯 IO，不含业务判定（可测逻辑下沉到调用方）；
//! key 一律经 [`Zeroizing`] 包装，drop 时擦除内存。

use std::io::{BufRead, IsTerminal, Write};

use dialoguer::Password;
use zeroize::Zeroizing;

/// 掩码读取一行密钥：终端输入逐字符回显 `*`（长度可见，内容不可见——
/// 红线约束的是明文不进回显，掩码字符不属于 key 材料）。
///
/// stdin 为终端时用 dialoguer `Password`（`set_report`）；管道/重定向场景
/// （`echo $KEY | quota set-key id`，spec §3）直接读一行——终端组件在
/// stdin 被占用的管道下无从交互，且管道场景本就没有"看着屏幕输入"的
/// 掩码需求。
/// 返回值已 trim（粘贴误差常见的首尾空白不进入密文）。
pub fn read_secret(prompt: &str) -> std::io::Result<Zeroizing<String>> {
    if std::io::stdin().is_terminal() {
        let line = Password::new()
            .with_prompt(prompt)
            .report(true)
            .interact()
            .map_err(|e| std::io::Error::other(format!("掩码输入失败：{e}")))?;
        Ok(Zeroizing::new(line.trim().to_string()))
    } else {
        let mut err = std::io::stderr();
        writeln!(err, "{prompt}").ok();
        err.flush().ok();
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        Ok(Zeroizing::new(buf.trim().to_string()))
    }
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
