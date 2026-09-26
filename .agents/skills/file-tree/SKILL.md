---
name: file-tree
description: 在已部署 file-tree 的仓库中，同步新增、删除或移动的文件与目录，更新或查询文件职责及 rel/tags，提交前校验文件树，合并 tree.json 的 Git 冲突，或只读浏览快照时使用。不用于维护 README 等其他手写文件树。
---

# 文件树维护与查询

## 核心约定

- **tree.json 是唯一数据源**：JSON 嵌套 = 目录嵌套（有 `children` 键即文件夹，空目录写 `"children": {}`）；条目类型由 children 判据推导为 `kind` 字段（`"file"`/`"dir"`）随规范化落盘。
- **脚本是唯一写入口**：对 tree.json、tags 词表、AGENTS.md 树块的一切增删改都必须走 `scripts/tree_tool.py`；手改会被 `check` 的规范形态与产物一致性校验当场暴露。
- **渲染目标为 AGENTS.md**：两个标记块（简版树 / 标签词表），有标记则替换内容、无标记自动附加到文件尾部、无 AGENTS.md 则生成最小骨架；`detail` 完整描述只存于 tree.json 供 `get`/`query` 查询，不渲染。
- **多树冲突时以技能为准**：仓库内其他手写文件树一律惰性对待（不同步、不维护、不删除），文件树相关问答与维护只认 tree.json。
- **确定性输出**：每次写操作后自动按字典序（大小写不敏感、码点决胜）重排并以固定格式序列化——紧凑单行 JSON（UTF-8 无 BOM、中文直存、逗号/冒号后无空格、末尾恰好一个 LF），任何机器运行产出字节一致，重复写同一内容 diff 干净。历史数据的两空格缩进排版仍是**合法可检形态**（`check` / `check --strict` 照常通过），无需迁移命令：下一次任意写操作落盘时自动转为紧凑格式，不占撤销历史步。取舍：单行 JSON 体积约减半，但 Git 行级 diff 默认整行变化，review 大改动时 `git diff --word-diff` 更可读。
- 目录收录粒度：目录条目**展开 children** 时其下文件必须全收（check 会报漏）；目录**不展开**（无 children 或空）表示整目录粗粒度收录（如图标集），其下文件不检查。收录目录条目用 `--dir`（批量清单写 `"dir": true`）；未声明而磁盘上是目录时自动识别为目录条目并打印提示，误录成文件条目会被 check 按类型错配报错并给出修正指引。
- **建议固定根名**：简版树首行默认取仓库根目录名——git worktree 检出或目录改名会让根名漂移、跨检出渲染不稳定。初始化录入前执行一次 `root <仓库名>` 固定（存于 tree.json 顶层可选键 `root`）；`root --clear` 恢复自动。

## 按需入口

- **条目、标签、查询和校验**：使用下方命令速查；维护完成后运行 `check --strict`。
- **Git 冲突**：运行 `merge` 前阅读 [两阶段合并参考](references/merge.md)，按机器可读清单提交决定。
- **视图和多文档标记块**：阅读 [视图完整参考](references/views.md)，再使用 `view-*` 命令。
- **只读浏览**：使用文末查看器入口；需要操作或排错细节时阅读 [查看器使用参考](references/viewer.md)。

## 命令速查

```bash
python .agents/skills/file-tree/scripts/tree_tool.py <命令>

# 新增/更新条目（upsert：未给的字段保留旧值；自动建父目录，写后自动渲染）
add <path> -d "一句话≤20字" [--detail "完整描述行"]... [--rel 相关路径]... [--tags a,b] [--dir]
           [--collapsed|--no-collapsed] [--hidden|--no-hidden]   # 渲染控制，见条目字段；--dir 收录目录条目，见核心约定·目录收录粒度
           [--git-ignore|--no-git-ignore]                        # 豁免 git 跟踪对照（收录 .gitignore 排除的本地文件），见条目字段

# 批量 upsert（JSON 清单）：一份清单 = 一次数据变更 = 一步撤销历史；任一条非法整批拒绝
add-batch <manifest.json>         # {"entries": [{"path": "a.ts", "desc": "简介"}, {"path": "assets/icons", "desc": "图标集", "dir": true}]}，条目字段同 add

rm <path>                        # 删除条目并修剪变空的父目录
rm-batch <path>...               # 批量删除：同样一次变更一步历史；预校验全部存在，任一缺失整批拒绝
mv <src> <dst>                   # 条目带信息迁移（含子树）：字段原样保留，自动重写指向旧路径的 rel 边；不移动磁盘文件

# 批量迁移（JSON 清单）：一份清单 = 一次数据变更 = 一步撤销历史；批内 src/dst 互斥预校验，任一条非法整批拒绝
mv-batch <manifest.json>         # {"moves": [{"src": "a.ts", "dst": "b/a.ts"}, ...]}
merge [--decisions <文件.json>]    # Git 冲突两阶段处理：自动三方合并；有歧义时输出 Agent 冲突清单并待决定
mark <dir> [--tags a,b] [--tags-mode add|replace] [--git-ignore|--no-git-ignore] [--depth N]
                                  # 子树批量标记（见下）：tags 追加/覆写 + git-ignore 传播，可限深度
get <path> [path...]             # 查看条目全部字段（可多路径批量，条间空行分隔）
query [--kw 关键词] [--tag 标签] [--rel-of 路径] [--under 目录] [--depth N] [--json]
                                  # 组合过滤；--rel-of 反查谁关联到我；--under 限定子树（锚点含入），--depth 相对层数（须与 --under 同用）
tag-add <名> -d "说明"           # 登记受控标签
tag-rm <名>                      # 删除标签（仍被条目使用时拒绝）
view-add <id> ...                 # 登记视图；参数与边界见 references/views.md
view-doc <id> ...                 # 增减绑定文档
view-rm <id> [--purge]            # 删除视图配置，可选清除绑定块
view-list                         # 查看已登记视图
undo / redo / history            # 撤销/重做最近的数据变更（默认各留 20 步）/ 查看概要
check [--strict]                 # 全量不变量校验（--strict 时告警也算失败）
render                           # 重渲染 AGENTS.md 两个标记块（缺标记自动附加到尾部）
root [<名字>|--clear]            # 查看/固定/清除渲染根名；未固定时自动取仓库根目录名（建议固定，见核心约定）
```

**移动（mv）**：条目（含整个子树）带信息迁移——desc/detail/rel/tags 等字段原样保留，全树自动重写指向旧路径的 rel 边（含前缀子路径引用与被移子树内部互指，`rm`+`add` 组合无法保证这一步）。**数据层迁移不碰磁盘文件**：磁盘移动归 `git mv`，两步之间的不一致由 `check` 磁盘对照暴露。目标已存在或位于源子树内（含粗粒度收录的虚拟子树）一律拒绝；源端修剪变空父目录、目标端自动建父链；单步撤销历史。

**批量命令（add-batch / rm-batch / mv-batch）**：一份清单 = 一次数据变更 = 一步撤销历史，批量操作不再逐条挤兑撤销栈（默认各留 20 步）。**整批原子生效**：任一条非法（未知标签、路径冲突、字段类型错误、条目不存在等）整批拒绝，tree.json 与 AGENTS.md 保持原状。add-batch 清单条目字段与单条 `add` 完全同语义（全可选、upsert 保留未给字段），但含未知字段或批内重复路径直接拒绝；`rel` 在整批应用后的最终树上统一校验，**批内条目互引合法**。`rm-batch` 额外预校验批内不得互为祖先-后代（删祖先已覆盖后代），逐条删除保留"修剪变空父目录"语义。`mv-batch` 清单为 `moves` 数组（每条恰含 `src`/`dst` 两个非空字符串）；批内互斥预校验——src 间、dst 间均不得重复或互为祖先-后代，且任一源与目的地不得落在其他移动的源或目的地路径上（双向互斥，**不支持批内移动链**，各条校验语义以初始树为准）；清单按序应用，结果与逐条同序执行一致；单条级违规由应用期校验拦截、消息自带具体路径。

**子树批量标记（mark）**：以树中已展开 children 的目录条目为锚点，把 tags 与 git-ignore 一次性传播到子树（锚点自身与子树外不动），一次变更 = 一步撤销历史。分工：**tags 作用于子树全部条目（含目录）**——tags 无继承语义，目录条目也是 `query --tag` 的对象；**git-ignore 仅落文件条目且只给"未表态"者表态**——已有显式设置（true/false）的条目是个体意图不覆写，true 方向还自动跳过 git 已跟踪文件（落 true 即矛盾标记，check 必报），false 方向不跳 tracked（落显式 false 恰是退出祖先豁免的修复动作）；目录不落标记，否则就近覆写继承会穿透 depth 限制。tags 两种模式：`add` 并集追加（默认）、`replace` 整体替换（`--tags ""` 即清空）；`--no-git-ignore` 落盘显式 false，可批量退出子树豁免。`--depth N` 限定相对锚点层数（1 = 直接子级），缺省全深度。输出 tags / git-ignore 受影响条数与跳过条数。

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
| `git-ignore` | bool，可选 | 豁免"必须被 git 跟踪"的对照，用于收录不走 git 版本控制的本地文件（如大体积产物）。check 改为只校验磁盘存在，并要求确实排除在 git 之外（实际被跟踪、或未被 .gitignore 覆盖均报错）。继承就近覆写：有效值取沿祖先链（含自身）最近一次显式设置，显式 `false` 让子条目/子树退出祖先豁免（如 `!` 反排除规则下走 git 的个别文件）；true/false 均落盘，缺省不落盘 = 继承 |
| `children` | object | 目录子条目；有此键即目录 |
| `dir` | bool，add/add-batch 命令标志（非落盘字段） | 收录为目录条目，落盘体现为 `children` 键；磁盘上是目录的路径未声明时自动识别为目录条目并打印提示 |

**字段完整性检测**：`check` 对每个条目做全量字段校验——未知字段、字段类型错误、缺 `desc` 报为错误；`desc` 为空或超长、文件条目缺 `detail` 报为告警（`--strict` 下告警也视为失败）。`collapsed`/`hidden` 类型不是布尔、文件条目带 `collapsed` 报为错误。技能目录内自身测试产生的 `__pycache__` 豁免"未收录"告警（运行时缓存）；仓库其他位置的 `__pycache__` 照常报。

**渲染控制只影响展示**：`collapsed`/`hidden` 仅改变 AGENTS.md 简版树的渲染形态——tree.json 数据始终全量，`get`/`query` 照常可查，`check` 的磁盘对照与产物一致性校验也不受影响（隐藏 ≠ 删除，隐藏条目漏录磁盘文件照样报错）。

**git-ignore 只改校验口径，不改展示**：简版树照常渲染豁免条目，`get`/`query` 照常可查。`check` 对豁免条目不要求被 git 跟踪，但磁盘必须存在；同时反向校验排除态——实际被 git 跟踪（标记与实况矛盾），或未被跟踪但 `.gitignore` 没有覆盖（git status 会持续显示 untracked，易被 `git add .` 误收）均报错，错误消息给出修正出路（`git rm --cached` / 补 ignore 规则 / 移除标记）。继承就近覆写：祖先标记 `true` 后，子条目可用显式 `false`（`--no-git-ignore`）退出豁免、其子树跟随；缺省（键不落盘）= 继承祖先。`query --json` 的 `git-ignore` 输出三态保真：`null`=缺省继承、`false`=显式退出、`true`=豁免。

## 渲染产物索引

默认简版树与标签词表由脚本渲染到 AGENTS.md 的两个标记块；完整描述留在 tree.json，通过 get/query 查询。

需要建立子树视图、绑定多文档标记块或排查视图渲染时，阅读 [视图完整参考](references/views.md)。

## 只读查看器（GUI）

需要浏览或搜索 `tree.json` 快照时，运行技能自带的只读网页查看器。快照可在仓库外；运行需要 Python 3.8+ 与现代浏览器，无需 Node：

```bash
python .agents/skills/file-tree/scripts/viewer.py <tree.json 路径> [--port N] [--host H]
```

查看器仅浏览数据，不写 `tree.json`、不转换格式、不生成撤销历史。需要刷新、远端访问、界面操作、启动排错或兼容性细节时，再读 [查看器使用参考](references/viewer.md)。
