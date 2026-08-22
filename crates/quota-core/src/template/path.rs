//! JSONPath 子集：仅支持模板 DSL 所需的取值语法。
//!
//! 语法：`$` 根 → `.name` 对象字段 → `[n]` 数组索引，可链式组合，
//! 如 `$.data.totalBalance`、`$.balance_infos[0].total_balance`。
//! 刻意不支持过滤器/通配符/递归下降——模板是用户输入，
//! 表达力越受限，安全面越小。

use serde_json::Value;

/// 解析路径语法为段序列（不含开头的 `$`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Segment {
    Field(String),
    Index(usize),
}

/// 校验路径语法，返回解析后的段序列；非法语法返回错误描述。
pub(crate) fn parse_path(path: &str) -> Result<Vec<Segment>, String> {
    let rest = path.strip_prefix('$').ok_or("路径必须以 $ 开头")?;
    let mut segments = Vec::new();
    let mut chars = rest.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        match c {
            '.' => {
                let start = i + 1;
                let name = read_field(&mut chars, start, rest)?;
                if name.is_empty() {
                    return Err(format!("位置 {i}：字段名为空"));
                }
                segments.push(Segment::Field(name));
            }
            '[' => {
                let start = i + 1;
                let mut digits = String::new();
                let mut closed = false;
                for (j, c2) in rest[start..].char_indices() {
                    match c2 {
                        '0'..='9' => digits.push(c2),
                        ']' => {
                            closed = true;
                            // 推进外层迭代器到 ']' 之后
                            while let Some(&(k, _)) = chars.peek() {
                                if k <= start + j {
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                            break;
                        }
                        _ => {
                            return Err(format!("位置 {}：方括号内仅支持非负整数索引", start + j));
                        }
                    }
                }
                if !closed {
                    return Err(format!("位置 {start}：方括号未闭合"));
                }
                let idx: usize = digits
                    .parse()
                    .map_err(|_| format!("位置 {start}：索引为空"))?;
                segments.push(Segment::Index(idx));
            }
            _ => {
                return Err(format!(
                    "位置 {i}：仅支持 .字段 与 [索引]，遇到 {c:?}（不支持过滤器/通配符）"
                ));
            }
        }
    }
    Ok(segments)
}

/// 从 JSON 值中按段序列取值；任一段缺失返回 None。
pub(crate) fn resolve<'a>(root: &'a Value, segments: &[Segment]) -> Option<&'a Value> {
    let mut current = root;
    for seg in segments {
        match seg {
            Segment::Field(name) => {
                current = current.get(name)?;
            }
            Segment::Index(idx) => {
                current = current.get(*idx)?;
            }
        }
    }
    Some(current)
}

/// 一步式便捷入口。
pub(crate) fn resolve_path<'a>(root: &'a Value, path: &str) -> Result<Option<&'a Value>, String> {
    let segments = parse_path(path)?;
    Ok(resolve(root, &segments))
}

fn read_field(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
    start: usize,
    rest: &str,
) -> Result<String, String> {
    let mut end = start;
    while let Some(&(k, c)) = chars.peek() {
        if c == '.' || c == '[' {
            break;
        }
        end = k + c.len_utf8();
        chars.next();
    }
    Ok(rest[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 语法契约：字段、索引、链式组合均可解析。
    #[test]
    fn parses_valid_paths() {
        assert_eq!(
            parse_path("$.data.totalBalance").unwrap(),
            vec![
                Segment::Field("data".into()),
                Segment::Field("totalBalance".into())
            ]
        );
        assert_eq!(
            parse_path("$.a[0].b[12]").unwrap(),
            vec![
                Segment::Field("a".into()),
                Segment::Index(0),
                Segment::Field("b".into()),
                Segment::Index(12)
            ]
        );
        assert_eq!(parse_path("$").unwrap(), Vec::<Segment>::new());
    }

    /// 语法契约：不支持的表达式一律拒绝（过滤器、通配符、负索引、缺 $）。
    #[test]
    fn rejects_unsupported_syntax() {
        for bad in [
            "data.totalBalance", // 缺 $
            "$..totalBalance",   // 递归下降
            "$.data[*]",         // 通配符
            "$.data[?(@.x)]",    // 过滤器
            "$.data[-1]",        // 负索引
            "$.",                // 空字段名
            "$.data[abc]",       // 非数字索引
            "$.data[0",          // 未闭合
        ] {
            assert!(parse_path(bad).is_err(), "{bad} 应被拒绝");
        }
    }

    /// 解析契约：对象与数组取值、缺失路径返回 None。
    #[test]
    fn resolves_values() {
        let root = json!({
            "data": {"totalBalance": "42.50"},
            "list": [{"x": 1}, {"x": 2}]
        });
        assert_eq!(
            resolve_path(&root, "$.data.totalBalance").unwrap().unwrap(),
            &json!("42.50")
        );
        assert_eq!(
            resolve_path(&root, "$.list[1].x").unwrap().unwrap(),
            &json!(2)
        );
        assert!(resolve_path(&root, "$.data.missing").unwrap().is_none());
        assert!(resolve_path(&root, "$.list[5]").unwrap().is_none());
        // 索引用在对象上 / 字段用在数组上 → None
        assert!(resolve_path(&root, "$.data[0]").unwrap().is_none());
        assert!(resolve_path(&root, "$.list.x").unwrap().is_none());
    }
}
