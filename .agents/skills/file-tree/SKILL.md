---
name: file-tree
description: 项目文件树的唯一数据源与维护入口。当需要新增/删除/修改文件与目录的收录条目、查询文件职责（一句话简介或完整描述）、查相关成对文件（rel）、按受控标签（tags）过滤、或提交前核对文件树与磁盘一致性时使用。tree.json 为单一事实源，AGENTS.md 中的树与词表标记块是脚本渲染产物，禁止手改。多树冲突规则：仓库内同时存在其他手写文件树（README、规格文档等）时，以本技能 tree.json 的查询结果为准，对其他文件树惰性对待——不主动同步、不维护它们，只维护技能自身的树。
---

# 文件树维护与查询

## 核心约定

- **tree.json 是唯一数据源**：JSON 嵌套 = 目录嵌套（有 `children` 键即文件夹，空目录写 `"children": {}`）；条目类型由 children 判据推导为 `kind` 字段（`"file"`/`"dir"`）随规范化落盘。
- **脚本是唯一写入口**：对 tree.json、tags 词表、AGENTS.md 树块的一切增删改都必须走 `scripts/tree_tool.py`；手改会被 `check` 的规范形态与产物一致性校验当场暴露。
- **渲染目标为 AGENTS.md**：两个标记块（简版树 / 标签词表），有标记则替换内容、无标记自动附加到文件尾部、无 AGENTS.md 则生成最小骨架；`detail` 完整描述只存于 tree.json 供 `get`/`query` 查询，不渲染。
- **多树冲突时以技能为准**：仓库内其他手写文件树一律惰性对待（不同步、不维护、不删除），文件树相关问答与维护只认 tree.json。
- **确定性输出**：每次写操作后自动按字典序（大小写不敏感、码点决胜）重排并以固定格式序列化（2 空格缩进、LF、UTF-8），任何机器运行产出字节一致，git diff 稳定。
- 目录收录粒度：目录条目**展开 children** 时其下文件必须全收（check 会报漏）；目录**不展开**（无 children 或空）表示整目录粗粒度收录（如图标集），其下文件不检查。
- **建议固定根名**：简版树首行默认取仓库根目录名——git worktree 检出或目录改名会让根名漂移、跨检出渲染不稳定。初始化录入前执行一次 `root <仓库名>` 固定（存于 tree.json 顶层可选键 `root`）；`root --clear` 恢复自动。

## 命令速查

```bash
python .agents/skills/file-tree/scripts/tree_tool.py <命令>

# 新增/更新条目（upsert：未给的字段保留旧值；自动建父目录，写后自动渲染）
add <path> -d "一句话≤20字" [--detail "完整描述行"]... [--rel 相关路径]... [--tags a,b] [--dir]
           [--collapsed|--no-collapsed] [--hidden|--no-hidden]   # 渲染控制，见条目字段

# 批量 upsert（JSON 清单）：一份清单 = 一次数据变更 = 一步撤销历史；任一条非法整批拒绝
add-batch <manifest.json>         # {"entries": [{"path": "a.ts", "desc": "简介"}, ...]}，条目字段同 add

rm <path>                        # 删除条目并修剪变空的父目录
rm-batch <path>...               # 批量删除：同样一次变更一步历史；预校验全部存在，任一缺失整批拒绝
get <path>                       # 查看单条目全部字段
query [--kw 关键词] [--tag 标签] [--rel-of 路径] [--json]   # 组合过滤；--rel-of 反查谁关联到我
tag-add <名> -d "说明"           # 登记受控标签
tag-rm <名>                      # 删除标签（仍被条目使用时拒绝）
undo / redo / history            # 撤销/重做最近的数据变更（默认各留 20 步）/ 查看概要
check [--strict]                 # 全量不变量校验（--strict 时告警也算失败）
render                           # 重渲染 AGENTS.md 两个标记块（缺标记自动附加到尾部）
root [<名字>|--clear]            # 查看/固定/清除渲染根名；未固定时自动取仓库根目录名（建议固定，见核心约定）
```

**批量命令（add-batch / rm-batch）**：一份清单 = 一次数据变更 = 一步撤销历史，批量录入不再逐条挤兑撤销栈（默认各留 20 步）。**整批原子生效**：任一条非法（未知标签、路径冲突、字段类型错误、条目不存在等）整批拒绝，tree.json 与 AGENTS.md 保持原状。清单条目字段与单条 `add` 完全同语义（全可选、upsert 保留未给字段），但含未知字段或批内重复路径直接拒绝；`rel` 在整批应用后的最终树上统一校验，**批内条目互引合法**。`rm-batch` 额外预校验批内不得互为祖先-后代（删祖先已覆盖后代），逐条删除保留"修剪变空父目录"语义。

**撤销历史（防误操作）**：每次数据变更前自动快照当前 tree.json 全量；undo 恢复后自动重渲染 AGENTS.md，新操作会截断 redo 分支（编辑器语义）。历史存放于 **git 私有区 `<gitdir>/file-tree/history.json`**——天然不被 git 追踪、不入库、clone 不携带；判定以 `<gitdir>/HEAD` 存在为准（空 `.git` 目录不算仓库），且脚本绝不创建 `.git`；非 git 环境退化为技能目录本地文件 `.history.json`。**仓库初始化晚于技能使用时自动收敛**：加载按 git 私有区 → 旧位置顺序找历史（撤销栈不断裂），保存永远写 git 私有区并删除旧位置文件；check 对待收敛状态给出告警。历史基线是「上一次脚本操作前」，中途手改 tree.json 的内容会随回滚丢失（手改本就被禁止）。

契约测试：`python .agents/skills/file-tree/scripts/tree_tool_test.py`

## 条目字段

| 字段 | 形态 | 语义 |
| --- | --- | --- |
| `kind` | `"file"` / `"dir"`，自动派生 | 条目类型标识：由 children 判据自动推导并落盘，供 `query --json` 等机器消费；不参与渲染，手改会在下次写操作时被规范化纠正 |
| `desc` | string，必填 | 一句话简介（≤20 字，超长 `check` 告警）；渲染简版树；空串表示目录待补 |
| `detail` | string[]，文件条目应填 | 完整描述，存于 tree.json 供 `get`/`query` 查询，不参与渲染；缺失时文件条目 `check` 告警（目录不强制） |
| `rel` | string[]，可选 | 语义相关/成对文件的仓库相对路径（如双语文案成对、测试指向被测文件）；只存正向边，反查用 `query --rel-of`；写入时统一规范为正斜杠形式 |
| `tags` | string[]，可选 | 受控标签，必须已在词表登记（词表渲染于 AGENTS.md 词表块） |
| `collapsed` | bool，目录可选 | 简版树折叠渲染：目录行带 `…` 不展开 children；默认 false（false 不落盘）。仅目录可用，文件条目报错 |
| `hidden` | bool，可选 | 简版树隐藏渲染：条目及整个子树不出现在 AGENTS.md；默认 false（false 不落盘）。文件与目录均可用 |
| `children` | object | 目录子条目；有此键即目录 |

**字段完整性检测**：`check` 对每个条目做全量字段校验——未知字段、字段类型错误、缺 `desc` 报为错误；`desc` 为空或超长、文件条目缺 `detail` 报为告警（`--strict` 下告警也视为失败）。`collapsed`/`hidden` 类型不是布尔、文件条目带 `collapsed` 报为错误。技能目录内自身测试产生的 `__pycache__` 豁免"未收录"告警（运行时缓存）；仓库其他位置的 `__pycache__` 照常报。

**渲染控制只影响展示**：`collapsed`/`hidden` 仅改变 AGENTS.md 简版树的渲染形态——tree.json 数据始终全量，`get`/`query` 照常可查，`check` 的磁盘对照与产物一致性校验也不受影响（隐藏 ≠ 删除，隐藏条目漏录磁盘文件照样报错）。

## 渲染产物索引

简版树（一句话速览）与标签词表两个标记块统一渲染在 **AGENTS.md**（无标记时由 `render` 自动附加到文件尾部）。需要完整描述时用 `get <path>`/`query --kw` 查询 tree.json，不渲染第二棵树。
