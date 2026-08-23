//! `quota natives`：列出预置平台（来自 core 注册表）。

use quota_core::provider;

use crate::lang::Lang;
use crate::render;
use crate::texts::{T, t};

pub fn run(lang: Lang) -> i32 {
    let metas = provider::metas();
    if metas.is_empty() {
        println!("{}", t(lang, T::NativesEmpty));
        return 0;
    }
    println!("{}", render::natives_table(&metas, lang));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：表格渲染包含注册表全部平台 id（双语表头）。
    #[test]
    fn table_lists_registry_ids() {
        let metas = provider::metas();
        for lang in [Lang::Zh, Lang::En] {
            let table = render::natives_table(&metas, lang);
            for m in &metas {
                assert!(table.contains(m.id), "{lang:?} 缺 {}：{table}", m.id);
            }
            assert!(table.contains(t(lang, T::ColName)), "{lang:?}: {table}");
        }
    }
}
