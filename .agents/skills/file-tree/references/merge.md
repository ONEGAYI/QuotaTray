# `tree.json` 的 Git 冲突合并

仅在 Git 将本技能的 `tree.json` 留在未解决的三方冲突状态时使用 `merge`。此命令每次从 Git index 读取共同祖先（stage 1，可缺省）、本分支（stage 2）和对方分支（stage 3），不会解析工作树中的冲突标记，也不依赖后台进程或跨命令内存。

## 第一阶段：自动合并并取得清单

```bash
python .agents/skills/file-tree/scripts/tree_tool.py merge
```

脚本按条目和字段合并可确定的改动。不同分支的改动组合后若使 `rel`、标签或视图引用失效，也会列为待决冲突。

- `status: merged`：没有待决项，已写回规范的紧凑 `tree.json`。
- `status: needs_decisions`：stdout 给出 `schema_version: 1`、`merge_id`、`conflicts` 和 `unresolved`，**不写 `tree.json`**。每条冲突包含按内容指纹生成的 `id`、`reason`、`scope`、`path`、`field`、三个版本的 `{present,value}` 以及允许的 `actions`。`present` 区分字段不存在与 JSON `null`。

## 第二阶段：提交 Agent 的决定

把清单中全部待决项写入 UTF-8 JSON 文件，再调用同一子命令：

```json
{
  "merge_id": "第一阶段输出的 merge_id",
  "decisions": [
    {"id": "清单中的冲突 id", "action": "set", "value": "最终描述"}
  ]
}
```

```bash
python .agents/skills/file-tree/scripts/tree_tool.py merge --decisions <决定文件.json>
```

`action` 可选 `base`、`ours`、`theirs`、`set`；涉及缺失值或引用失效的冲突还允许 `delete`，以每条冲突的 `actions` 为准。`set` 必须提供 `value`。再次调用时脚本**重新读取 Git 三个阶段并重算自动部分**，因此决定文件须累计包含先前所有决定。

决定文件若遗漏、重复或包含未知冲突 id，`merge_id` 已过期，动作不合法，缺少 `value`，或决定产生无效结果，脚本会在 stdout 返回 `status: invalid_decisions` 和带定位、修正提示的 `error`，退出码为 2，且不写 `tree.json`。`decision_mismatch` 的 `missing_ids` / `unknown_ids` 供核对；若先处理一个冲突才显露新的引用冲突，`missing_conflicts` 包含新冲突的完整三方内容。冲突 id 绑定具体内容，不会因为前一项决定变化而指向别的条目。

## 完成条件

`merge` 成功后，先处理 AGENTS.md 等文档自身的合并冲突，再运行 `render` 和 `check --strict`，最后暂存已解决的文件。`merge` 不自动执行 `git add`，不改写 Git index，也不进入普通写命令的 undo 历史。
