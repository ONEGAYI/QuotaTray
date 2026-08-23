//! 条目 id 生成：6 位 Crockford base32 随机串。
//!
//! 字符集排除易混淆字符（I/L/O/U）；32 整除 256，字节取模无偏。

/// Crockford base32 字符集（32 符号，去掉易混淆的 I L O U）；
/// 32 整除 256，字节取模无偏。
const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// 生成 6 位随机 id。
pub fn generate() -> Result<String, getrandom::Error> {
    let mut buf = [0u8; 6];
    getrandom::fill(&mut buf)?;
    Ok(buf
        .iter()
        .map(|b| ALPHABET[usize::from(b % 32)] as char)
        .collect())
}

/// 生成与现有 id 不冲突的随机 id（6 字符 32^6 ≈ 10 亿空间，碰撞后重生成即可）。
pub fn unique_id(existing: &[String]) -> Result<String, getrandom::Error> {
    loop {
        let id = generate()?;
        if !existing.iter().any(|e| e == &id) {
            return Ok(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：6 位、字符集内、多次生成不重复。
    #[test]
    fn generates_six_chars_from_alphabet() {
        let valid: std::collections::HashSet<char> = ALPHABET.iter().map(|&c| c as char).collect();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let id = generate().unwrap();
            assert_eq!(id.len(), 6, "长度应为 6：{id}");
            assert!(
                id.chars().all(|c| valid.contains(&c)),
                "含字符集外字符：{id}"
            );
            seen.insert(id);
        }
        assert_eq!(seen.len(), 100, "100 次生成出现重复（概率上不应发生）");
    }

    /// 契约：unique_id 结果不与既有列表冲突。
    #[test]
    fn unique_id_avoids_existing() {
        let existing: Vec<String> = (0..50).map(|_| generate().unwrap()).collect();
        for _ in 0..20 {
            let id = unique_id(&existing).unwrap();
            assert!(!existing.contains(&id));
        }
    }
}
