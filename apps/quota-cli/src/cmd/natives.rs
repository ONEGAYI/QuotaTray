//! `quota natives`：列出预置平台（来自 core 注册表）。

use quota_core::provider;

use crate::render;
use crate::texts::{T, t};

pub fn run(ctx: &crate::ctx::Ctx) -> i32 {
    let metas = provider::metas();
    if metas.is_empty() {
        println!("{}", t(ctx.lang, T::NativesEmpty));
        return 0;
    }
    // 有效目录（本地读取，零网络）：预置判定与定价命令同源
    let catalog = quota_core::load_effective(&ctx.catalog_dir());
    println!(
        "{}",
        render::natives_table(&metas, &catalog.catalog, ctx.lang)
    );
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::Lang;

    /// 契约：表格渲染包含注册表全部平台 id（双语表头）。
    #[test]
    fn table_lists_registry_ids() {
        let metas = provider::metas();
        for lang in [Lang::Zh, Lang::En] {
            let table = render::natives_table(&metas, quota_core::bundled_catalog(), lang);
            for m in &metas {
                assert!(table.contains(m.id), "{lang:?} 缺 {}：{table}", m.id);
            }
            assert!(table.contains(t(lang, T::ColName)), "{lang:?}: {table}");
        }
    }
}
