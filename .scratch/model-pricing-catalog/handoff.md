# QuotaTray 模型与定价目录更新交接

日期：2026-09-10  
工作目录：D:\CODE\Project\_DesktopApps\QuotaTray  
基线分支：main  
基线提交：19bc8fa157212b355e5f18e51614ceb15704e403。

## 目标与本轮范围

用户希望价格和模型变化不再要求发布应用升级。
本轮已准备实施规格、8 张本地票据与本交接文档，未实现功能。

用户最后明确指示：“implement 的部分你不做，把 spec/tickets 建好之后 handoff”。
这限制当前会话的工作范围；接手者应按新会话的实现授权推进，不能把本文件当作自动推送或合并授权。

## 已确认约定

- 人工审核模型和价格数据；审核后自动发布，客户端自动接收并应用。
- 下架模型保留同一模型的最后已知价格，并标注已下架。
- 没有历史价格时显示未知，不使用另一模型的价格。
- 官方更新不覆盖用户自定义模型或手填价格。

已核实的旧行为：pricing.rs 的 resolve_impl 在显式模型未命中时，
可能保留输入标签、使用平台默认模型定价。用户已经理解并接受修正方向。

## 交付物与阅读顺序

1. [规格](spec.md)：产品行为、数据格式、覆盖规则、缓存、并发、三端同步、发布和 V-01 至 V-12 验收。
2. [票据索引与依赖图](README.md)：T-01 至 T-08，含 Blocked by。
3. [首票 T-01](issues/01-catalog-foundation.md)：唯一无前置依赖的目录基础准备票。

以上路径均位于 D:\CODE\Project\_DesktopApps\QuotaTray\.scratch\model-pricing-catalog。
票据 ID 是本地编号，没有创建 GitHub Issue、任务分支或远端跟踪项目。

## 技术默认值与尚未完成的运营事项

以下是规格提出的默认实现方案，不是用户逐项确认的原话：

- 同一 JSON 用作人工审核文件、Raw 分发文件与构建时内置种子。
- 拟定作者路径 data/pricing/v1/catalog.json；目前未创建生产数据文件。
- 拟定源 https://raw.githubusercontent.com/ONEGAYI/QuotaTray/main/data/pricing/v1/catalog.json；
  当前未发布此文件，未验证最终源连通性，不承诺 CDN 即时刷新。
- 自动成功检查间隔 6 小时，失败退避至少 30 分钟；普通 CLI 定价命令自动补检总预算 5 秒。
- CLI JSON 默认不隐式联网；显式 catalog update --json 才联网。
- 模型下架记录随完整目录保留，发布校验拒绝物理删除。
- GUI 每分钟及聚焦/回前台重读本地版本，以接收 CLI 对公共缓存的更新。
- 初始无法证实的核验日期留空；不把本轮抽取日期写成价格核验日期。

T-01 本地实现没有外部阻塞。
正式分发连通性、仓库是否强制人工审核、真实客户端更新验证留给 T-07/T-08。
不得为“补齐流程”主动修改分支保护或批准、合并数据 PR。
Android 继续 Preview；本任务不新增后台服务，也不承诺完整实机验收。

## 源码入口与需要保留的语义

- crates/quota-core/src/pricing.rs：内置预置、模型选择、自定义覆盖与峰谷纯函数。
- apps/quota-cli/src/cmd/pricing.rs、pricing_models.rs：定价展示、模型列表及 JSON 输出。
- apps/quota-desktop/src-tauri/src/commands.rs：native_meta_dtos 与 list_native_metas。
- apps/quota-desktop/src/components/providerPricing.ts：前端定价解析镜像，必须同步缺失/下架语义。
- apps/quota-desktop/src/queries.ts：native-metas 现为 30 秒 staleTime，更新需要事件主动失效。
- crates/quota-core/src/update.rs：双通道客户端、节流与文件写入工具；不复用安装包状态机。
- scripts/fetch_pricing/fetch_pricing.py：部分按量价格候选抓取，窗口和订阅仍人工维护。

现有自定义规则不是逐个价格数字补齐：
用户非空价格档整档覆盖，档内缺值仍缺值；
同 ID 自定义模型优先，而且自定义模型缺价不借官方同名价；
windows 缺省和空数组的含义不同。不要用一句“用户优先”掩盖这些细节。

## 工作树状态与已做验证

会话开始已经存在以下他人或用户未提交改动，本轮没有修改：

- Cargo.lock
- apps/quota-desktop/src-tauri/Cargo.toml

本轮新增 .scratch/model-pricing-catalog 下的规格、索引、票据和 handoff；
通过 file-tree 唯一维护入口登记文档，修改 .agents/skills/file-tree/tree.json。
scratch 条目隐藏渲染，避免将阶段文档展开到项目速览；文档仍被文件树校验。

没有实现代码、测试代码或 CI 工作流改动；没有提交、推送、远端 Issue、发布和部署。
本轮仅验证文档链接、票据依赖、文件树一致性与 diff 格式；
应用构建、Rust/前端测试及 Android 冒烟均未运行，不能宣称功能验证通过。

实际校验结果（2026-09-10）：

- 11 份 Markdown 的内部文件链接均可解析。
- 8 张票据均有验收清单和验证章节；依赖无环、无悬空引用，T-01 为唯一无前置票据。
- python .agents/skills/file-tree/scripts/tree_tool.py check 通过。
- git diff --check 通过；git status 与上述改动范围一致，AGENTS.md 渲染结果未变化。

## Suggested Skills

按接手阶段使用，避免一次加载全部技能：

- 实施单票：C:\Users\64487\.codex\plugins\cache\openai-curated-remote\matt-skills-curated\1.1.0\skills\implement\SKILL.md
- 测试驱动：C:\Users\64487\.codex\plugins\cache\openai-curated-remote\matt-skills-curated\1.1.0\skills\tdd\SKILL.md
- 文件树：D:\CODE\Project\_DesktopApps\QuotaTray\.agents\skills\file-tree\SKILL.md
- GUI 阶段：D:\CODE\Project\_DesktopApps\QuotaTray\.agents\skills\frontend-style-spec\SKILL.md

先阅读仓库 AGENTS.md。代码修改前展示当前票据计划；已有授权无需重复询问。
core 公开 API 变更按项目规则先独立 PR 合入，再执行调用端依赖票。
不探测或设置 hooks，不绕过门禁；中文提交必须带正文。

## 下一步

查看任务包的精确命令（在 PowerShell 运行）：

~~~powershell
code "D:\CODE\Project\_DesktopApps\QuotaTray\.scratch\model-pricing-catalog\README.md"
~~~

给接手者的完整提示：

> 请读取 D:\CODE\Project\_DesktopApps\QuotaTray\.scratch\model-pricing-catalog\handoff.md，
> 然后读取 spec.md、README.md 和 issues/01-catalog-foundation.md。
> 本次开始实施 T-01，先核对当前工作树及基线差异，保护已有 Cargo.lock 和桌面 Cargo.toml 改动。
> 按票据先写契约测试，再实现目录基础与兼容接口；不顺带修改价格。
> T-01 的 core 公开 API 通过独立 PR 交付，完成本票检查后提供验收结果。
> 不自动合并或发布数据，不跳过前置依赖，不重问已经确认的模型下架与数据审核规则。

后续按 T-02 → T-03 推进，再依据依赖图接入各端与发布链。
