//! 退出码三分约定（CLI-spec §4）。
//!
//! | 码 | 含义 |
//! |---|---|
//! | 0 | 全部成功 |
//! | 1 | 存在至少一个确定性失败（需人工介入） |
//! | 2 | 仅存在瞬时失败（可重试） |
//!
//! 确定性失败优先于瞬时失败。clap 用法错误维持 clap 默认退出码 2
//! （Unix 惯例），与查询结果语义不冲突：用法错误发生在任何查询之前。

use quota_core::QueryError;

/// 由查询结果集合计算进程退出码。
pub fn exit_code<T>(results: &[Result<T, QueryError>]) -> i32 {
    if results
        .iter()
        .any(|r| matches!(r, Err(e) if !e.is_transient()))
    {
        1
    } else if results
        .iter()
        .any(|r| matches!(r, Err(e) if e.is_transient()))
    {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：全部成功（含空集合）→ 0。
    #[test]
    fn all_ok_yields_zero() {
        assert_eq!(exit_code(&[Ok(()), Ok(())]), 0);
        assert_eq!(exit_code::<()>(&[]), 0);
    }

    /// 契约：任一确定性失败 → 1。
    #[test]
    fn any_deterministic_yields_one() {
        let results = [
            Ok(()),
            Err(QueryError::transient("timeout")),
            Err(QueryError::deterministic("401")),
        ];
        assert_eq!(exit_code(&results), 1);
    }

    /// 契约：仅瞬时失败 → 2。
    #[test]
    fn only_transient_yields_two() {
        let results = [Ok(()), Err(QueryError::transient("timeout"))];
        assert_eq!(exit_code(&results), 2);
    }

    /// 契约：确定性优先于瞬时（混合时报 1，不报 2）。
    #[test]
    fn deterministic_takes_precedence() {
        let results: Vec<Result<(), QueryError>> = vec![
            Err(QueryError::transient("503")),
            Err(QueryError::deterministic("parse")),
        ];
        assert_eq!(exit_code(&results), 1);
    }
}
