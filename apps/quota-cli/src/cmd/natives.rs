//! `quota natives`：列出预置平台（来自 core 注册表）。

use quota_core::provider;

use crate::render;

pub fn run() -> i32 {
    let metas = provider::metas();
    if metas.is_empty() {
        println!("（无预置平台）");
        return 0;
    }
    println!("{}", render::natives_table(&metas));
    0
}

#[cfg(test)]
mod tests {
    /// 契约：表格渲染包含注册表全部平台 id。
    #[test]
    fn table_lists_registry_ids() {
        let metas = quota_core::provider::metas();
        let table = super::render::natives_table(&metas);
        for m in &metas {
            assert!(table.contains(m.id), "缺 {}：{table}", m.id);
        }
    }
}
