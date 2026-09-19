# quota-cli 子树视图

CLI 前端（bin 名 `quota`）的文件树子集。数据源为 file-tree 技能 `tree.json`，下方块由脚本渲染，禁止手改；AGENTS.md 主树中本子树折叠为一行，本页承载明细。

```
<!-- file-tree:tree^id=apps-cli:begin 由脚本渲染，禁止手改 -->
QuotaTray/
└── apps/
    └── quota-cli/ # CLI 前端（子树视图拆出）
        ├── Cargo.toml # CLI crate 清单
        ├── src/       # CLI 源码
        │   ├── cmd/           # 子命令实现（每命令一模块）
        │   │   ├── add.rs             # 交互添加向导
        │   │   ├── assist.rs          # Agent 无凭据调试
        │   │   ├── clear.rs           # 清空全部用户数据命令
        │   │   ├── config.rs          # 配置导入导出
        │   │   ├── devsmoke.rs        # 开发冒烟（仅 debug）
        │   │   ├── edit.rs            # 编辑向导与启停
        │   │   ├── history.rs         # history 命令（M5）
        │   │   ├── list.rs            # 条目列表
        │   │   ├── mod.rs             # 子模块声明
        │   │   ├── natives.rs         # 预置平台表
        │   │   ├── pricing.rs         # 定价查看/写入
        │   │   ├── pricing_catalog.rs # 目录状态与手动更新命令
        │   │   ├── pricing_models.rs  # 自定义模型库管理
        │   │   ├── query.rs           # 并行查询与 watch
        │   │   ├── remove.rs          # 确认删除
        │   │   ├── script.rs          # 脚本试查
        │   │   ├── setkey.rs          # 写入 API key
        │   │   ├── template.rs        # 模板试查
        │   │   ├── update.rs          # 更新检测/下载命令
        │   │   └── vault.rs           # vault 健康检查
        │   ├── ctx.rs         # CLI 上下文
        │   ├── exit.rs        # 退出码三分约定
        │   ├── idgen.rs       # 随机 id 生成
        │   ├── io.rs          # 交互 IO 薄层
        │   ├── lang.rs        # 语言三态与检测
        │   ├── main.rs        # clap 定义与 dispatch
        │   ├── render.rs      # 表格与 JSON 渲染
        │   ├── settings_io.rs # CLI 设置读改写
        │   └── texts.rs       # 双语文案表
        └── tests/     # CLI端到端契约测试
            └── catalog_readonly.rs # 目录只读与来源端测
<!-- file-tree:tree^id=apps-cli:end -->
```
