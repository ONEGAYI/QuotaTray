"""file-tree 技能：项目文件树唯一维护入口。

数据源 tree.json：顶层 {tags, tree, views}，tree 嵌套 = 目录嵌套（有 children 键即目录）。
条目字段固定顺序 kind / desc / detail / rel / tags / collapsed / hidden / git-ignore / children；
kind 由 children 判据推导（"file"/"dir"），规范化时无条件落盘供机器消费，
不参与渲染、手改会被纠正；本脚本是唯一写入口，所有写命令执行后自动按
确定性字典序（casefold + 码点决胜）规范化并重渲染产物。
tree.json 持久化为紧凑 JSON（UTF-8 无 BOM、中文直存、无缩进、分隔符无空格、
末尾恰好一个 LF）；历史的两空格缩进排版仍是合法可检形态（check 照常通过），
不需要迁移命令——下一次任意写操作落盘时自动转为紧凑格式，不占撤销历史步。

渲染目标分两层：默认视图 = AGENTS.md 的两个标记块（简版树 / 标签词表），
块内有标记则替换标记间内容；无标记则附加到文件尾部（带小节标题）；
AGENTS.md 不存在则生成最小骨架。子树视图 = tree.json 顶层 views 键登记的
渲染配置（id + 过滤器 + 渲染覆盖 + 绑定文档清单），把数据的切片投影（剪影）
渲染到各绑定文档的带 id 标记块（代码围栏包裹，仅树块无 tags 块）。过滤器为
五种节点（and/or/not/under/tag）的表达式树，在 tree.json 全量条目上求值（not
为补集语义）；CLI 快捷参数编译为规范表达式树落盘（多锚点并集、锚点×标签
交集、排除差集），完整布尔组合走 --filter 清单文件；引用（under 树中目录、
tag 已登记标签）在 view-add 写盘前预检拒绝。剪影把选中集投回全树结构：选中
条目正常渲染（简介照常、collapsed 生效），仅含选中后代的未选中祖先目录作
路径骨架（只作容器不显示简介、不折叠），首行为全局 root 名，hidden 条目在
无覆盖的视图不出现，空选中集渲染仅剩根名行。视图级渲染覆盖（render_overrides）
按路径索引改写该视图内的 hidden/collapsed 有效值——优先级：视图覆盖 > 条目
全局字段 > 默认值，布尔双向（可反向覆盖：全局 hidden=true 在本视图显示、
全局 collapsed=true 在本视图展开）；覆盖只影响渲染可见性，不改求值集合、不
改条目字段、不影响其他视图与默认视图；hidden 为祖先优先剪枝语义（祖先有效
hidden=true 时其后代覆盖不再查询），骨架目录的 collapsed 覆盖被忽略（强制
展开不破例）；CLI 快捷参数 --collapse/--expand/--hide/--show 编译或 --overrides
清单承载多覆盖项；引用预检（路径在树中、collapsed 指向目录）在 view-add 写盘
前拒绝，rm 后悬空是合法中间态——渲染静默忽略、check 报错、不阻塞数据操作。
一个视图 id 可绑定一个或多个文档：全部绑定文档中的块内容完全相同（同一渲染
产物镜像），任何数据变更后全部镜像同步刷新；绑定清单的增量增删走 view-doc
（--rm 解绑默认保留块为孤儿，不再刷新）；视图删除走 view-rm（默认仅删配置
保留块，--purge 连带按清单逐一删块：每文档删前校验恰好一个，原子拒绝半删
状态）。同一文档内同 id 出现多于一个块属病态：相关命令（view-doc、view-add、
触发渲染的数据命令）报错并指明文档路径与块数，不做猜测性修复。
check 对视图做两级诊断：错误 = 绑定文档缺块 / 同 id 多块 / 块内容与渲染
产物漂移（消息附纠正出路）；告警 = 全仓库 .md 扫到未登记 id 的孤儿标记块
（豁免技能目录与代码围栏内示意行；块自身包裹围栏不算豁免围栏）。
detail 完整描述只存于 tree.json 供查询，不渲染。渲染控制字段只影响树渲染：
目录 collapsed=true 折叠（目录行带 … 不展开 children）；条目 hidden=true
整体隐藏（含子树）；两者默认 false（不落盘），数据、查询与 check 校验始终
全量不受影响。
校验控制字段 git-ignore=true 则豁免"必须被 git 跟踪"的对照（收录 .gitignore
排除的本地文件，如大体积产物）：check 只校验磁盘存在，并要求确实排除在
git 之外（未被跟踪且被 ignore 规则覆盖，二者违反其一均报错）。继承为就近
覆写：有效值取沿祖先链（含自身）最近一次显式设置，显式 false 可让子条目/
子树退出祖先豁免（true/false 均落盘，缺省不落盘 = 继承），数据、查询与
渲染不受影响。
仓库内其他手写文件树惰性对待：以本技能 tree.json 的查询结果为准，
不主动同步维护它们。

用法：
  python tree_tool.py add <path> -d 描述 [--detail 行]... [--rel 路径]... [--tags a,b] [--dir]
                         [--collapsed|--no-collapsed] [--hidden|--no-hidden]
                         [--git-ignore|--no-git-ignore]
  python tree_tool.py add-batch <manifest.json>
  python tree_tool.py rm <path>
  python tree_tool.py rm-batch <path>...
  python tree_tool.py mv <src> <dst>
  python tree_tool.py mv-batch <manifest.json>
  python tree_tool.py get <path>...             # 查看条目（可多路径批量）
  python tree_tool.py query [--kw 关键词] [--tag 标签] [--rel-of 路径] [--under 目录] [--depth N] [--json]
  python tree_tool.py mark <dir> [--tags a,b] [--tags-mode add|replace]
                         [--git-ignore|--no-git-ignore] [--depth N]
  python tree_tool.py tag-add <名> -d 说明
  python tree_tool.py tag-rm <名>
  python tree_tool.py view-add <id> (--under <目录>)... [--tag <标签>]... [--exclude <目录>]...
                         [--filter <清单.json>] [--doc <路径> [--line N]]
                         [--collapse <目录> | --expand <目录> | --hide <路径> | --show <路径>]...
                         [--overrides <清单.json>]
  python tree_tool.py view-doc <id> (--add <路径> [--line N] | --rm <路径>)
  python tree_tool.py view-rm <id> [--purge]
  python tree_tool.py view-list
  python tree_tool.py undo | redo | history
  python tree_tool.py check [--strict]
  python tree_tool.py render
  python tree_tool.py root [<名>|--clear]

撤销历史（默认 20 步）存放于 git 私有区 <gitdir>/file-tree/history.json：
不被 git 追踪、不入库、clone 不携带；非 git 仓库退化为技能目录 .history.json。
视图命令进同一撤销栈：快照含数据与受影响文档全文，回滚连同渲染产物一并恢复。
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

SKILL_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = SKILL_DIR.parents[2]

FIELD_ORDER = ["kind", "desc", "detail", "rel", "tags", "collapsed", "hidden", "git-ignore", "children"]

TREE_BEGIN = "<!-- file-tree:tree:begin 由脚本渲染，禁止手改 -->"
TREE_END = "<!-- file-tree:tree:end -->"
TAGS_BEGIN = "<!-- file-tree:tags:begin 由脚本渲染，禁止手改 -->"
TAGS_END = "<!-- file-tree:tags:end -->"
FENCE = "```"

# 视图 id：小写字母/数字开头，允许连字符与下划线，总长 1-64；default 为默认视图保留字
VIEW_ID_RE = re.compile(r"[a-z0-9][a-z0-9_-]{0,63}")
VIEW_ID_RESERVED = "default"
# 过滤器表达式树的节点算子（spec 一期 schema 定稿；求值与渲染消费已全量支持五种节点）
FILTER_OPS = frozenset({"and", "or", "not", "under", "tag"})
# 孤儿标记块的宽松行匹配（check 告警级扫描用）：比整行精确匹配宽——手写笔误
# 恰是形态不精确的标记行；id 段排除冒号/尖括号/空白，防吞注释后半文案
VIEW_MARKER_BEGIN_RE = re.compile(r"^\s*<!--\s*file-tree:tree\^id=([^:>\s]+):begin")
VIEW_MARKER_END_RE = re.compile(r"^\s*<!--\s*file-tree:tree\^id=[^:>\s]+:end")


DESC_MAX = 20
HISTORY_LIMIT = 20  # undo/redo 各自保留的最大步数


def resolve_git_dir(repo_root: Path) -> Path | None:
    """定位有效的 git 私有目录：普通仓库的 .git/，或 worktree 的 gitdir: 指针。

    以 <gitdir>/HEAD 存在为准——空 .git 目录或无效指针不算仓库（也绝不创建 .git）。
    """
    dot = repo_root / ".git"
    gitdir: Path | None = None
    if dot.is_dir():
        gitdir = dot
    elif dot.is_file():
        first_line = dot.read_text(encoding="utf-8", errors="replace").strip().splitlines()
        if first_line and first_line[0].startswith("gitdir:"):
            target = Path(first_line[0][len("gitdir:") :].strip())
            gitdir = target if target.is_absolute() else repo_root / target
    if gitdir is not None and (gitdir / "HEAD").is_file():
        return gitdir
    return None


def default_history_path(repo_root: Path, skill_dir: Path) -> Path:
    """历史存放：git 私有区（不被追踪/不入库/clone 不携带）；非 git 仓库退化为技能目录本地文件。"""
    gitdir = resolve_git_dir(repo_root)
    if gitdir is not None:
        return gitdir / "file-tree" / "history.json"
    return skill_dir / ".history.json"


class ToolError(Exception):
    """确定性错误：路径非法、条目缺失、不变量冲突等。"""


def sort_key(name: str):
    """大小写不敏感字母序，码点决胜——跨机器确定性排序。"""
    return (name.casefold(), name)


def split_rel_path(path: str) -> list[str]:
    """校验仓库相对路径并拆段；拒绝绝对路径与 '..'/'.' 段。"""
    if not isinstance(path, str) or not path.strip():
        raise ToolError(f"路径为空: {path!r}")
    normalized = path.replace("\\", "/")
    if normalized.startswith("/"):
        raise ToolError(f"拒绝绝对路径: {path}")
    if len(normalized) > 1 and normalized[1] == ":":  # Windows 盘符
        raise ToolError(f"拒绝绝对路径: {path}")
    parts = [p for p in normalized.split("/") if p]
    for part in parts:
        if part in ("..", "."):
            raise ToolError(f"路径含 '{part}' 段: {path}")
    if not parts:
        raise ToolError(f"路径为空: {path!r}")
    return parts


def _normalize_node(node: dict) -> dict:
    if not isinstance(node, dict):
        raise ToolError(f"条目不是对象: {node!r}")
    out: dict = {}
    known = set(FIELD_ORDER)
    for field in FIELD_ORDER:
        if field == "kind" or field not in node:
            continue  # kind 不采信输入值，由 children 判据推导
        value = node[field]
        if field == "desc":
            if not isinstance(value, str):
                raise ToolError(f"desc 必须是字符串: {value!r}")
            out["desc"] = value  # 空串保留，表示待补，由 check 告警
        elif field == "detail":
            if not isinstance(value, list) or not all(isinstance(x, str) and x for x in value):
                raise ToolError(f"detail 必须是非空字符串数组: {value!r}")
            if value:
                out["detail"] = list(value)  # 语义顺序，不排序；空列表移除
        elif field in ("rel", "tags"):
            if not isinstance(value, list) or not all(isinstance(x, str) for x in value):
                raise ToolError(f"{field} 必须是字符串数组: {value!r}")
            cleaned = sorted(set(value), key=sort_key)
            if cleaned:
                out[field] = cleaned
        elif field in ("collapsed", "hidden"):
            if not isinstance(value, bool):
                raise ToolError(f"{field} 必须是布尔值: {value!r}")
            if value:
                out[field] = True  # false 为默认值，不落盘
        elif field == "git-ignore":
            if not isinstance(value, bool):
                raise ToolError(f"git-ignore 必须是布尔值: {value!r}")
            out["git-ignore"] = value  # 三态：键在即显式设置（false 覆写祖先豁免），缺省不落盘 = 继承
        elif field == "children":
            if not isinstance(value, dict):
                raise ToolError(f"children 必须是对象: {value!r}")
            out["children"] = {
                name: _normalize_node(child)
                for name, child in sorted(value.items(), key=lambda kv: sort_key(kv[0]))
            }
    if "collapsed" in out and "children" not in out:
        raise ToolError("collapsed 仅用于目录条目（文件条目请用 hidden）")
    for key in sorted((k for k in node if k not in known), key=sort_key):
        out[key] = node[key]  # 未知字段排序附尾，由 check 报错暴露手改
    ordered = {"kind": "dir" if "children" in out else "file"}
    ordered.update(out)
    return ordered


def _normalize_filter(expr) -> dict:
    """校验并规范化过滤器表达式树节点（五种：and/or/not/under/tag）。

    under 路径归一为正斜杠形式；不做跨数据校验（路径在树中、标签已登记）——
    那是创建入口（view-add）的职责，数据层只锁结构。
    """
    if not isinstance(expr, dict):
        raise ToolError(f"过滤器节点必须是对象: {expr!r}")
    op = expr.get("op")
    if not isinstance(op, str) or op not in FILTER_OPS:
        raise ToolError(f"过滤器节点 op 非法（须为 {sorted(FILTER_OPS)}）: {expr!r}")
    unknown = [k for k in expr if k not in ("op", "path", "tag", "children", "child")]
    if unknown:
        raise ToolError(f"过滤器节点含未知字段 {unknown}: {expr!r}")
    if op == "under":
        if not isinstance(expr.get("path"), str):
            raise ToolError(f"under 节点缺 path 字符串: {expr!r}")
        return {"op": "under", "path": "/".join(split_rel_path(expr["path"]))}
    if op == "tag":
        if not isinstance(expr.get("tag"), str) or not expr["tag"]:
            raise ToolError(f"tag 节点缺非空 tag 字符串: {expr!r}")
        return {"op": "tag", "tag": expr["tag"]}
    if op in ("and", "or"):
        children = expr.get("children")
        if not isinstance(children, list) or not children:
            raise ToolError(f"{op} 节点须为非空 children 数组: {expr!r}")
        return {"op": op, "children": [_normalize_filter(c) for c in children]}
    # not
    if not isinstance(expr.get("child"), dict):
        raise ToolError(f"not 节点缺 child 对象: {expr!r}")
    return {"op": "not", "child": _normalize_filter(expr["child"])}


def _normalize_overrides(overrides) -> dict:
    """校验并规范化 render_overrides：路径归一排序、字段定序（collapsed/hidden）。

    只锁结构（字段名、布尔类型），不做跨数据校验（路径在树中、collapsed 指向
    目录）——那是 view-add 写盘预检与 check 的职责（T1 校验时机裁定）。
    false 是语义值（双向覆盖），保留落盘；空对象项报错（无字段即无语义）；
    整体空集剔除由调用方处理（与 docs 空省略键一致）。
    """
    if not isinstance(overrides, dict):
        raise ToolError(f"render_overrides 必须是对象（路径 → 覆盖字段）: {overrides!r}")
    out: dict = {}
    for path, spec in sorted(overrides.items(), key=lambda kv: sort_key(kv[0])):
        if not isinstance(spec, dict) or not spec:
            raise ToolError(f"render_overrides['{path}'] 必须是非空对象（至少含 collapsed/hidden 一个字段）")
        unknown = [k for k in spec if k not in ("collapsed", "hidden")]
        if unknown:
            raise ToolError(f"render_overrides['{path}'] 含未知字段 {unknown}（仅支持 collapsed/hidden）")
        for field in ("collapsed", "hidden"):
            if field in spec and not isinstance(spec[field], bool):
                raise ToolError(f"render_overrides['{path}'] 的 {field} 必须是布尔值: {spec[field]!r}")
        cleaned = {f: spec[f] for f in ("collapsed", "hidden") if f in spec}
        out["/".join(split_rel_path(path))] = cleaned
    return out


def _normalize_views(views) -> dict:
    """校验并规范化 views：id 排序、实体字段定稿（filter + docs + render_overrides）、空集剔除。

    render_overrides 是视图级渲染覆盖（二期）：按路径索引改写该视图内条目的
    hidden/collapsed 有效值（视图覆盖 > 条目全局字段 > 默认值，布尔双向）。
    空 docs 省略键（配置先行的视图合法），空 render_overrides 同样省略键。
    """
    if not isinstance(views, dict):
        raise ToolError(f"views 必须是对象: {views!r}")
    out: dict = {}
    for view_id, spec in sorted(views.items(), key=lambda kv: sort_key(kv[0])):
        if view_id == VIEW_ID_RESERVED:
            raise ToolError("视图 id 'default' 是默认视图保留字，不可登记")
        if not isinstance(view_id, str) or not VIEW_ID_RE.fullmatch(view_id):
            raise ToolError(f"视图 id 语法非法（[a-z0-9][a-z0-9_-]{{0,63}}，禁保留字 default）: {view_id!r}")
        if not isinstance(spec, dict):
            raise ToolError(f"视图 {view_id} 配置必须是对象: {spec!r}")
        unknown = [k for k in spec if k not in ("filter", "docs", "render_overrides")]
        if unknown:
            raise ToolError(f"视图 {view_id} 配置含未知字段 {unknown}")
        if "filter" not in spec:
            raise ToolError(f"视图 {view_id} 缺 filter")
        docs = spec.get("docs", [])
        if not isinstance(docs, list) or not all(isinstance(d, str) for d in docs):
            raise ToolError(f"视图 {view_id} 的 docs 必须是字符串数组")
        entity = {"filter": _normalize_filter(spec["filter"])}
        cleaned_docs = sorted({"/".join(split_rel_path(d)) for d in docs}, key=sort_key)
        if cleaned_docs:
            entity["docs"] = cleaned_docs
        cleaned_overrides = _normalize_overrides(spec["render_overrides"]) if "render_overrides" in spec else {}
        if cleaned_overrides:
            entity["render_overrides"] = cleaned_overrides
        out[view_id] = entity
    return out


def normalize_data(data: dict) -> dict:
    if not isinstance(data, dict):
        raise ToolError("数据顶层不是对象")
    out: dict = {}
    if "root" in data:  # 用成员判定而非 get：显式 root:null 也是病态，必须报错
        root = data["root"]
        if not isinstance(root, str) or not root:
            raise ToolError("root 必须是非空字符串（固定渲染根名；清除请用 root --clear）")
        out["root"] = root  # 置于最前：根名是简版树的第一行
    tags = data.get("tags", {})
    if not isinstance(tags, dict):
        raise ToolError("tags 必须是对象")
    kept = {name: desc for name, desc in sorted(tags.items(), key=lambda kv: sort_key(kv[0])) if desc}
    if kept:
        out["tags"] = kept
    tree = data.get("tree", {})
    if not isinstance(tree, dict):
        raise ToolError("tree 必须是对象")
    out["tree"] = {
        name: _normalize_node(child)
        for name, child in sorted(tree.items(), key=lambda kv: sort_key(kv[0]))
    }
    for key in sorted((k for k in data if k not in ("tags", "tree", "root")), key=sort_key):
        if key == "views":
            views = _normalize_views(data[key])  # 结构化键：校验 + 规范化，非透传
            if views:
                out["views"] = views
            continue
        out[key] = data[key]
    return out


def dumps_canonical(data: dict) -> str:
    """规范序列化（现行）：紧凑 JSON——无缩进、分隔符无额外空格、中文直存，末尾恰好一个 LF。"""
    return json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n"


def dumps_canonical_legacy(data: dict) -> str:
    """规范序列化（历史两空格缩进）：仅供 check 兼容判定与旧样本构造，写入路径不再使用。"""
    return json.dumps(data, ensure_ascii=False, indent=2) + "\n"


def canonical_form(text: str) -> str | None:
    """返回 text 的规范序列化形态："compact"（新紧凑）/ "legacy"（旧两空格缩进）/ None（非规范）。

    CRLF 归一内聚于此（与 check 同口径）：先把行尾归一为 LF，再解析并规范化，
    与两种规范输出做序列化文本级比较——键序、缩进、空字段等排版偏差都判否，
    只比较 JSON 对象相等不足以通过；解析失败或结构非法同样判 None。
    check 与部署器升级路径共用本判定（is_canonical_text 为其薄封装），
    避免双格式接受标准漂移。
    """
    normalized = text.replace("\r\n", "\n")
    try:
        data = normalize_data(json.loads(normalized))
    except (json.JSONDecodeError, ToolError):
        return None
    for form, out in (("compact", dumps_canonical(data)), ("legacy", dumps_canonical_legacy(data))):
        if normalized == out:
            return form
    return None


def is_canonical_text(text: str) -> bool:
    """判定 text 是否为脚本规范的两种序列化形态之一（新紧凑 / 旧两空格缩进）。"""
    return canonical_form(text) is not None


def _find_block(lines: list[str], begin: str, end: str, ordered: bool = False) -> tuple[int, int]:
    """定位 begin/end 标记行下标（整行精确匹配）；ordered 时校验顺序。

    标记块的定位三段形状（缺失/顺序）全库共用；ordered=False 供只读提取
    （如 block_content）沿用历史行为：不校验顺序，交调用方上下文兜底。
    """
    try:
        b, e = lines.index(begin), lines.index(end)
    except ValueError as exc:
        raise ToolError(f"缺标记块 {begin} / {end}（文件被改坏？）") from exc
    if ordered and e <= b:
        raise ToolError(f"标记块顺序错误: {begin} 在 {end} 之后")
    return b, e


def replace_block(text: str, begin: str, end: str, content: str) -> str:
    """把 begin/end 标记行之间的内容整体替换为 content（标记行独占一行）。"""
    lines = text.split("\n")
    b, e = _find_block(lines, begin, end, ordered=True)
    return "\n".join(lines[: b + 1] + content.split("\n") + lines[e:])


def block_content(text: str, begin: str, end: str) -> str:
    lines = text.split("\n")
    b, e = _find_block(lines, begin, end)
    return "\n".join(lines[b + 1 : e])


def view_tree_markers(view_id: str) -> tuple[str, str]:
    """视图子树标记块起止行（整行精确匹配，与默认块同款注释文案）。"""
    return (
        f"<!-- file-tree:tree^id={view_id}:begin 由脚本渲染，禁止手改 -->",
        f"<!-- file-tree:tree^id={view_id}:end -->",
    )


def views_candidate(data: dict, views: dict, view_id: str, spec: dict) -> dict:
    """构建"替换单个视图实体"的候选数据：其余视图逐实体浅拷贝保留，目标实体整体替换。"""
    return {**data, "views": {k: dict(v) for k, v in views.items() if k != view_id} | {view_id: spec}}


def fenced_block_lines(begin: str, end: str, content: str) -> list[str]:
    """代码围栏包裹的标记块行序列（与默认简版树块同款形态）。"""
    return [FENCE, begin, *content.split("\n"), end, FENCE]


def append_view_block(text: str, begin: str, end: str, content: str) -> str:
    """把视图块追加到文档尾部：块外上方至少一行空行，末尾恰好一个 LF。"""
    block = "\n".join(fenced_block_lines(begin, end, content))
    return text.rstrip("\n") + "\n\n" + block + "\n"


def insert_block_at_line(text: str, begin: str, end: str, content: str, line: int) -> str:
    """把围栏首行落到第 N 行（1-based 承诺落点），越界报错拒绝而非钳制。

    上方空行保证：原第 N-1 行已是空行 → 插在当前第 N 行内容之前（fence 落 N）；
    非空 → 插入序列带前导空行、整体插在当前第 N-1 行内容之前（空行占第 N-1 行、
    fence 仍落 N、原第 N-1 行内容后移）。文档头（N=1）无上方约束。
    下方空行保证：块后首行非空则补一个空行。
    """
    lines = text.split("\n")
    if not isinstance(line, int) or isinstance(line, bool) or not (1 <= line <= len(lines)):
        raise ToolError(f"--line {line!r} 越界（文档共 {len(lines)} 行，1-based，须落在 1..{len(lines)}）")
    prefix_lines: list[str] = []
    start = line  # 插入点：插在当前第 start 行内容之前（1-based）
    if line > 1 and lines[line - 2] != "":
        prefix_lines = [""]
        start = line - 1  # 前导空行占第 N-1 行，插入点随之上移一行
    before, after = lines[: start - 1], lines[start - 1 :]
    if after and after[0] != "":
        after = [""] + after
    return "\n".join(before + prefix_lines + fenced_block_lines(begin, end, content) + after)


def remove_view_block(text: str, begin: str, end: str) -> str:
    """删除标记块（整行精确匹配），连带包裹围栏与紧邻上方的一个空行。

    上下空行是插入时补的保障行：删除块时连同上方那一行一并移除最接近
    还原插入前形态；围栏不存在（被手改破坏）时退化为只删 begin..end。
    拼接点收敛：删除区间上方若残留原有空行（插入时被迫补前导空行的场景），
    与下方保障空行相接会成双空行——去掉下方那个，删除动作自身不产生粘连。
    """
    lines = text.split("\n")
    b, e = _find_block(lines, begin, end, ordered=True)
    start = b - 1 if b > 0 and lines[b - 1] == FENCE else b
    stop = e + 2 if e + 1 < len(lines) and lines[e + 1] == FENCE else e + 1
    if start > 0 and lines[start - 1] == "":
        start -= 1
    if start > 0 and stop < len(lines) and lines[start - 1] == "" and lines[stop] == "":
        stop += 1  # 上方残留空行与下方保障空行相接：去其一，防双空行粘连
    return "\n".join(lines[:start] + lines[stop:])


def is_dir(node: dict) -> bool:
    return "children" in node


def walk_entries(children: dict, prefix: list[str]):
    """按规范序深度遍历，产出 (完整路径, 节点)。"""
    for name, node in sorted(children.items(), key=lambda kv: sort_key(kv[0])):
        path = "/".join(prefix + [name])
        yield path, node
        if is_dir(node) and node["children"]:
            yield from walk_entries(node["children"], prefix + [name])


def render_children_lines(children: dict, prefix: str) -> list[str]:
    """渲染目录子级块（简版树与剪影共用）：hidden 跳过、collapsed 折叠、按最宽 stem 对齐。"""

    items = [
        (name, node)
        for name, node in sorted(children.items(), key=lambda kv: sort_key(kv[0]))
        if not node.get("hidden")
    ]
    if not items:
        return []
    stems = []
    for i, (name, node) in enumerate(items):
        connector = "└── " if i == len(items) - 1 else "├── "
        suffix = "/" if is_dir(node) else ""
        if suffix and node.get("collapsed") and node["children"]:
            suffix = "/…"
        stems.append(prefix + connector + name + suffix)
    column = max(len(s) for s in stems) + 1  # '#' 所在列
    lines = []
    for i, ((name, node), stem) in enumerate(zip(items, stems)):
        cont_prefix = prefix + ("    " if i == len(items) - 1 else "│   ")
        if node.get("desc"):
            lines.append(stem + " " * (column - len(stem)) + "# " + node["desc"])
        else:
            lines.append(stem)
        if is_dir(node) and node["children"] and not node.get("collapsed"):
            lines.extend(render_children_lines(node["children"], cont_prefix))
    return lines


def render_tree(root_name: str, tree: dict) -> str:
    """渲染简版树：注释为 desc 单行；hidden 条目整体跳过，collapsed 目录带 … 折叠。

    列对齐：每个父目录的 children 块内按最宽 stem 对齐，'#' 固定在 width+1 列。
    """
    lines = [root_name + "/"]
    lines.extend(render_children_lines(tree, ""))
    return "\n".join(lines)


def _effective_flag(overrides: dict | None, path: str, field: str, node: dict) -> bool:
    """渲染控制字段的有效值：视图覆盖（render_overrides）> 条目全局字段 > 默认值 False。

    hidden 与 collapsed 同用此链——布尔双向（覆盖 false 可反向撤销全局 true），
    覆盖 true 可在全局缺省时单独生效。
    """
    if overrides:
        spec = overrides.get(path)
        if spec is not None and field in spec:
            return spec[field]
    return bool(node.get(field))


def render_silhouette(root_name: str, tree: dict, selected: set[str], overrides: dict | None = None) -> str:
    """选中集投回全树结构的剪影渲染。

    选中条目正常渲染（简介照常、选中目录 collapsed 折叠生效）；仅含选中后代
    的未选中祖先目录作路径骨架——只作容器：无简介、忽略 collapsed 强制展开
    （折叠会让剪影丢内容）。hidden 条目在任何视图不出现（hidden 目录连同
    子树整体跳过，与简版树同语义）；空选中集仅剩首行全局 root 名。
    overrides 为视图级渲染覆盖（render_overrides）：按路径索引改写 hidden/
    collapsed 的有效值（视图覆盖 > 全局字段 > 默认值，布尔双向），只影响
    渲染可见性——不改求值集合、不改数据；祖先有效 hidden=true 先行剪枝，
    其后代的覆盖不再查询（与全局 hidden 同语义）。
    """
    skeleton: set[str] = set()
    for path in selected:
        parts = path.split("/")
        for i in range(1, len(parts)):
            ancestor = "/".join(parts[:i])
            if ancestor not in selected:
                skeleton.add(ancestor)
    lines = [root_name + "/"]
    lines.extend(_silhouette_lines(tree, "", [], selected, skeleton, overrides))
    return "\n".join(lines)


def _silhouette_lines(
    children: dict, prefix: str, parts: list[str], selected: set[str], skeleton: set[str],
    overrides: dict | None = None,
) -> list[str]:
    """剪影的单层子级渲染（与 render_children_lines 同款对齐）：仅渲染选中条目与骨架祖先。

    hidden 节点连同子树跳过；骨架目录不显示简介且不折叠；选中条目简介照常、
    目录 collapsed 生效（折叠即不再下钻，与简版树一致）。hidden/collapsed 取
    有效值（视图覆盖 > 全局字段 > 默认值），骨架目录的 collapsed 覆盖被忽略
    （强制展开规则不破例）。
    """
    items = []
    for name, node in sorted(children.items(), key=lambda kv: sort_key(kv[0])):
        path = "/".join(parts + [name])
        if _effective_flag(overrides, path, "hidden", node):
            continue
        if path in selected:
            items.append((name, node, True))
        elif path in skeleton:
            items.append((name, node, False))
    if not items:
        return []
    stems = []
    for i, (name, node, is_sel) in enumerate(items):
        connector = "└── " if i == len(items) - 1 else "├── "
        suffix = "/" if is_dir(node) else ""
        if suffix and is_sel and node["children"] and _effective_flag(overrides, "/".join(parts + [name]), "collapsed", node):
            suffix = "/…"  # 仅选中目录可折叠；骨架目录必须展开到选中后代
        stems.append(prefix + connector + name + suffix)
    column = max(len(s) for s in stems) + 1
    lines = []
    for i, ((name, node, is_sel), stem) in enumerate(zip(items, stems)):
        cont_prefix = prefix + ("    " if i == len(items) - 1 else "│   ")
        collapsed = _effective_flag(overrides, "/".join(parts + [name]), "collapsed", node)
        if is_sel and node.get("desc"):
            lines.append(stem + " " * (column - len(stem)) + "# " + node["desc"])
        else:
            lines.append(stem)
        if is_dir(node) and node["children"] and not (is_sel and collapsed):
            lines.extend(_silhouette_lines(node["children"], cont_prefix, parts + [name], selected, skeleton, overrides))
    return lines


def iter_filter_refs(filt: dict):
    """产出表达式树中的字面引用：(路径, None) 为 under、(None, 标签名) 为 tag。"""
    op = filt["op"]
    if op == "under":
        yield filt["path"], None
    elif op == "tag":
        yield None, filt["tag"]
    elif op in ("and", "or"):
        for child in filt["children"]:
            yield from iter_filter_refs(child)
    else:  # not
        yield from iter_filter_refs(filt["child"])


def compile_filter(unders, tags, excludes) -> dict:
    """CLI 快捷参数编译为规范表达式树：多锚点并集(or)、锚点×标签交集(and)、排除差集(and+not)。

    规范形态：and children 依次为 锚点组（多锚点以 or 包裹，单锚点退化为裸 under
    节点）→ 标签 → 排除（not 包裹），各组内按确定性排序去重；仅一个维度时退化
    为裸节点。调用方保证 unders 与 tags 至少一项非空（纯排除无被减对象）。
    """
    u_sorted = sorted({"/".join(split_rel_path(u)) for u in unders}, key=sort_key)
    t_sorted = sorted(set(tags), key=sort_key)
    e_sorted = sorted({"/".join(split_rel_path(e)) for e in excludes}, key=sort_key)
    children: list[dict] = []
    if len(u_sorted) == 1:
        children.append({"op": "under", "path": u_sorted[0]})
    elif u_sorted:
        children.append({"op": "or", "children": [{"op": "under", "path": u} for u in u_sorted]})
    children.extend({"op": "tag", "tag": t} for t in t_sorted)
    children.extend({"op": "not", "child": {"op": "under", "path": e}} for e in e_sorted)
    if len(children) == 1:
        return children[0]
    return {"op": "and", "children": children}


def compile_overrides(overrides, collapse, expand, hide, show) -> dict:
    """覆盖来源合并为 render_overrides：清单（overrides）或快捷参数编译，二选一。

    快捷参数编译（路径归一、组内确定性）：--collapse/--expand → collapsed
    true/false，--hide/--show → hidden true/false；同路径不同字段合并、同
    路径同字段冲突报错。返回 {} 表示无覆盖（upsert 时移除键）。
    """
    shortcut = [
        ("/".join(split_rel_path(p)), field, value)
        for p, field, value in [
            *((c, "collapsed", True) for c in (collapse or [])),
            *((e, "collapsed", False) for e in (expand or [])),
            *((h, "hidden", True) for h in (hide or [])),
            *((s, "hidden", False) for s in (show or [])),
        ]
    ]
    if overrides is not None:
        if shortcut:
            raise ToolError("--overrides 清单与快捷参数（--collapse/--expand/--hide/--show）互斥，二选一")
        return overrides  # 原样交由 normalize 校验归一
    merged: dict[str, dict] = {}
    for path, field, value in shortcut:
        spec = merged.setdefault(path, {})
        if field in spec and spec[field] is not value:
            raise ToolError(f"路径 {path} 的 {field} 覆盖冲突（同一路径同字段不可既 true 又 false）")
        spec[field] = value
    return merged


def eval_filter(filt: dict, tree: dict) -> set[str]:
    """在 tree.json 全量条目上求值过滤器表达式树，返回选中路径集合。

    under = 锚点自身含入的前缀子树；tag = 带该标签的条目；not = 全量条目
    （含目录）上的补集；and/or 为子树求值的交/并。hidden 不在此层处理
    （渲染层统一跳过，选中集语义与数据层一致）。
    """
    op = filt["op"]
    if op == "under":
        anchor = filt["path"]
        return {p for p, _ in walk_entries(tree, []) if p == anchor or p.startswith(anchor + "/")}
    if op == "tag":
        return {p for p, node in walk_entries(tree, []) if filt["tag"] in node.get("tags", [])}
    if op == "and":
        return set.intersection(*(eval_filter(c, tree) for c in filt["children"]))
    if op == "or":
        return set.union(*(eval_filter(c, tree) for c in filt["children"]))
    return {p for p, _ in walk_entries(tree, [])} - eval_filter(filt["child"], tree)


def find_node(tree: dict, parts: list[str]) -> dict | None:
    """沿段定位节点；不存在或路径中段是文件则返回 None。

    公共纯函数：核心工具与 viewer_core 共用的唯一实现（#23 审查
    Standards-1），只读消费方不得复刻第二份逻辑。
    """
    node: dict = {"children": tree}
    for part in parts:
        if not is_dir(node) or part not in node["children"]:
            return None
        node = node["children"][part]
    return node


def is_tree_dir(tree: dict, path: str) -> bool:
    """path 是否为树中已存在的目录条目（under 引用预检与渲染层悬空判定共用）。"""
    node = find_node(tree, split_rel_path(path))
    return node is not None and is_dir(node)


def effective_git_ignore(tree: dict, path: str) -> bool:
    """path 的 git-ignore 有效值：就近覆写——沿祖先链（含自身）最近一次显式设置生效，均缺省则不豁免。

    公共纯函数：核心工具 check 与 viewer_core detail 共用的唯一实现
    （#23 审查 Standards-1）；path 中段是文件时无更深祖先可继承，落 False。
    """
    value = False
    cursor: dict = {"children": tree}
    for part in split_rel_path(path):
        if not is_dir(cursor):
            break  # 中途段是文件条目：无更深的祖先设置可继承（与 find_node 同防御）
        child = cursor["children"].get(part)
        if child is None:
            break
        if "git-ignore" in child:
            value = child["git-ignore"]
        cursor = child
    return value


class TreeTool:
    def __init__(
        self,
        tree_json: Path,
        agents_md: Path,
        repo_root: Path,
        root_name: str,
        history_path: Path,
        history_limit: int = HISTORY_LIMIT,
        legacy_history_paths: tuple[Path, ...] = (),
    ):
        self.tree_json = tree_json
        self.agents_md = agents_md
        self.repo_root = repo_root
        self.root_name = root_name
        self.history_path = history_path
        self.history_limit = history_limit
        self.legacy_history_paths = legacy_history_paths  # 历史旧位置（如 git 初始化前的退化位置），保存时收敛删除
        self.git_files_override: set[str] | None = None
        self.git_tracked_override: set[str] | None = None

    # ---------- 数据读写（唯一写入口） ----------

    def load(self) -> dict:
        try:
            text = self.tree_json.read_text(encoding="utf-8")
        except FileNotFoundError as exc:
            raise ToolError(f"tree.json 不存在: {self.tree_json}") from exc
        try:
            return json.loads(text)
        except json.JSONDecodeError as exc:
            raise ToolError(f"tree.json 解析失败: {exc}") from exc

    def write_data(self, data: dict) -> None:
        self.tree_json.parent.mkdir(parents=True, exist_ok=True)
        with self.tree_json.open("w", encoding="utf-8", newline="\n") as f:
            f.write(dumps_canonical(normalize_data(data)))

    # ---------- undo/redo（全量快照双栈，历史不入版本库） ----------

    def _load_history(self) -> dict:
        """按 canonical → legacy 顺序找历史：git 初始化前落在技能目录的旧历史仍可读。"""
        candidates = [self.history_path] + [p for p in self.legacy_history_paths if p != self.history_path]
        for path in candidates:
            try:
                hist = json.loads(path.read_text(encoding="utf-8"))
            except (FileNotFoundError, json.JSONDecodeError):
                continue
            if isinstance(hist, dict) and "undo" in hist and "redo" in hist:
                return hist
        return {"undo": [], "redo": []}

    def _save_history(self, hist: dict) -> None:
        """永远写 canonical 位置，并收敛删除 legacy 残留（迁移不产生可被追踪的旧文件）。"""
        self.history_path.parent.mkdir(parents=True, exist_ok=True)
        with self.history_path.open("w", encoding="utf-8", newline="\n") as f:
            f.write(json.dumps(hist, ensure_ascii=False))
        for legacy in self.legacy_history_paths:
            if legacy == self.history_path:
                continue
            try:
                legacy.unlink()
            except OSError:
                pass  # 删除失败不阻断；check 会持续提示待收敛

    def _trim(self, stack: list) -> list:
        return stack[-self.history_limit :] if len(stack) > self.history_limit else stack

    def _record_undo(self, op: str, docs: dict[str, str] | None = None) -> None:
        """数据变更前调用：快照当前态入 undo 栈，截断 redo 分支。历史写入失败仅告警不阻断。

        docs 为受影响渲染文档的变更前全文快照（仓库相对路径 → 内容）：仅产物
        无法由数据纯函数重推导的命令需要（如 view-add 的块插入位置/删除）；
        块内容类产物可由 render 重算，不需要快照。
        """
        hist = self._load_history()
        entry: dict = {"op": op, "data": self.load()}
        if docs:
            entry["docs"] = docs
        hist["undo"].append(entry)
        hist["undo"] = self._trim(hist["undo"])
        hist["redo"] = []
        try:
            self._save_history(hist)
        except OSError as exc:
            print(f"警告: 历史写入失败，本次操作不可撤销: {exc}", file=sys.stderr)

    def history_summary(self) -> tuple[list[str], list[str]]:
        """返回 (可撤销操作列表, 可重做操作列表)，从旧到新。"""
        hist = self._load_history()
        return (
            [e.get("op", "?") for e in hist["undo"]],
            [e.get("op", "?") for e in hist["redo"]],
        )

    def _doc_path(self, rel: str) -> Path:
        return self.repo_root.joinpath(*split_rel_path(rel))

    def _read_doc(self, rel: str) -> str | None:
        """读仓库内文档全文；不存在返回 None（快照与恢复方约定跳过）。"""
        try:
            return self._doc_path(rel).read_text(encoding="utf-8")
        except FileNotFoundError:
            return None

    def _write_doc(self, rel: str, text: str) -> None:
        path = self._doc_path(rel)
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("w", encoding="utf-8", newline="\n") as f:
            f.write(text)

    def _current_docs_snapshot(self, rels) -> dict[str, str]:
        """按文档清单采集当前全文快照（缺失文档跳过）。"""
        return {rel: text for rel, text in ((r, self._read_doc(r)) for r in rels) if text is not None}

    def undo(self) -> str:
        hist = self._load_history()
        if not hist["undo"]:
            raise ToolError("没有可撤销的操作")
        entry = hist["undo"].pop()
        redo_entry: dict = {"op": entry["op"], "data": self.load()}
        if "docs" in entry:  # 对向快照：操作后的文档现状，供 redo 完整回放（含块位置）
            redo_entry["docs"] = self._current_docs_snapshot(entry["docs"])
        hist["redo"].append(redo_entry)
        hist["redo"] = self._trim(hist["redo"])
        self.write_data(entry["data"])
        for rel, text in entry.get("docs", {}).items():
            self._write_doc(rel, text)  # 渲染产物一并恢复（render 幂等，不会再次改动）
        self.render()
        self._save_history(hist)
        return entry["op"]

    def redo(self) -> str:
        hist = self._load_history()
        if not hist["redo"]:
            raise ToolError("没有可重做的操作")
        entry = hist["redo"].pop()
        undo_entry: dict = {"op": entry["op"], "data": self.load()}
        if "docs" in entry:
            undo_entry["docs"] = self._current_docs_snapshot(entry["docs"])
        hist["undo"].append(undo_entry)
        hist["undo"] = self._trim(hist["undo"])
        self.write_data(entry["data"])
        for rel, text in entry.get("docs", {}).items():
            self._write_doc(rel, text)
        self.render()
        self._save_history(hist)
        return entry["op"]

    # ---------- 条目操作 ----------

    def get(self, path: str) -> dict:
        parts = split_rel_path(path)
        node = find_node(self.load()["tree"], parts)
        if node is None:
            raise ToolError(f"条目不存在: {path}")
        return node

    def _resolve_for_write(self, data: dict, path: str) -> tuple[list[str], dict]:
        parts = split_rel_path(path)
        node: dict = {"children": data["tree"]}
        for part in parts[:-1]:
            child = node["children"].get(part)
            if child is None:
                child = {"desc": "", "children": {}}  # 自动建父链，desc 待补由 check 告警
                node["children"][part] = child
            if not is_dir(child):
                raise ToolError(f"路径中段是文件: {path}（冲突于 '{part}'）")
            node = child
        return parts, node

    def _apply_add(self, data, path, desc=None, detail=None, rel=None, tags=None, is_dir_entry=False, collapsed=None, hidden=None, git_ignore=None) -> str | None:
        """在内存 data 上应用单条 add 的校验与变换（不校验 rel、不落盘）。

        新建条目未声明目录而磁盘上是目录时自动收录为目录条目并返回提示文案；
        已存在条目不隐式翻转类型（存量错配由 check 报）。
        """
        vocab = set(data.get("tags", {}))
        if tags:
            unknown = [t for t in tags if t not in vocab]
            if unknown:
                raise ToolError(f"未知标签 {unknown}，先 tag-add 登记再使用")
        parts, parent = self._resolve_for_write(data, path)
        name = parts[-1]
        note: str | None = None
        node = parent["children"].get(name)
        if node is None:
            if not is_dir_entry and self.repo_root.joinpath(*parts).is_dir():
                # 目录路径录成文件条目没有合法存续场景（git ls-files 不列目录，check 必报错）
                is_dir_entry = True
                note = f"提示: {path} 磁盘上是目录，已按目录条目收录（未展开 children；需展开时逐个 add 其下文件）"
            node = {"desc": "", "children": {}} if is_dir_entry else {"desc": ""}
            parent["children"][name] = node
        elif is_dir_entry and not is_dir(node):
            raise ToolError(f"已存在同名文件条目，不能改成目录: {path}")
        if desc is not None:
            node["desc"] = desc
        if detail is not None:
            node["detail"] = [d for d in detail if d]
        if rel is not None:
            # 统一规范为正斜杠形式落盘，与 check 的精确字符串比较收敛（非规范分隔符不再漏过）
            node["rel"] = ["/".join(split_rel_path(r)) for r in rel]
        if tags is not None:
            node["tags"] = list(tags)
        if collapsed is not None:
            if collapsed:
                if not is_dir(node):
                    raise ToolError(f"collapsed 仅用于目录条目: {path}")
                node["collapsed"] = True
            else:
                node.pop("collapsed", None)
        if hidden is not None:
            if hidden:
                node["hidden"] = True
            else:
                node.pop("hidden", None)
        if git_ignore is not None:
            node["git-ignore"] = git_ignore  # 显式 false 也落盘：就近覆写祖先豁免（三态语义见 normalize）
        return note

    def _validate_rel(self, data, path, rel) -> None:
        """rel 引用校验：不为空串、不指自身、目标必须在树中。批量在整批应用后统一调用，批内互引合法。"""
        for r in rel:
            if not r:
                raise ToolError(f"rel 不能为空串: {path}")
            parts_r = split_rel_path(r)
            if parts_r == split_rel_path(path):
                raise ToolError(f"rel 不能指向自身: {path}")
            if find_node(data["tree"], parts_r) is None:
                raise ToolError(f"rel 目标不在树中（先 add 目标或修正路径）: {r}")

    def add(self, path, desc=None, detail=None, rel=None, tags=None, is_dir_entry=False, collapsed=None, hidden=None, git_ignore=None) -> None:
        data = self.load()
        if rel:
            self._validate_rel(data, path, rel)
        note = self._apply_add(data, path, desc=desc, detail=detail, rel=rel, tags=tags,
                               is_dir_entry=is_dir_entry, collapsed=collapsed, hidden=hidden, git_ignore=git_ignore)
        self._record_undo(f"add {path}")
        self.write_data(data)
        if note:
            print(note)

    def _remove_entry(self, data, parts, path=None) -> None:
        """删除 parts 指向的条目并修剪变空的父目录链（根不删），不落盘。"""
        parent = find_node(data["tree"], parts[:-1])  # parts[:-1]==[] 时返回根包装
        if parent is None or not is_dir(parent) or parts[-1] not in parent["children"]:
            raise ToolError(f"条目不存在: {path or '/'.join(parts)}")
        del parent["children"][parts[-1]]
        nodes: list[dict] = [{"children": data["tree"]}]
        for part in parts[:-1]:
            nodes.append(nodes[-1]["children"][part])
        for i in range(len(nodes) - 1, 0, -1):
            if not nodes[i]["children"]:
                del nodes[i - 1]["children"][parts[i - 1]]

    def rm(self, path) -> None:
        parts = split_rel_path(path)
        data = self.load()
        self._remove_entry(data, parts, path)
        self._record_undo(f"rm {path}")
        self.write_data(data)

    # ---------- 移动（数据层迁移，不碰磁盘文件） ----------

    def mv(self, src, dst) -> int:
        """条目带信息迁移（含整个子树），返回重写的 rel 边数。磁盘文件移动归 git mv，check 磁盘对照兜底。"""
        data = self.load()
        n = self._apply_mv(data, src, dst)
        self._record_undo(f"mv {src} -> {dst}")
        self.write_data(data)
        return n

    def _apply_mv(self, data, src, dst) -> int:
        """在内存 data 上应用 mv 的校验与变换（不落盘），返回重写的 rel 边数。"""
        src_parts = split_rel_path(src)
        dst_parts = split_rel_path(dst)
        src_key = "/".join(src_parts)
        dst_key = "/".join(dst_parts)
        if dst_key == src_key:
            raise ToolError(f"源与目标相同: {src}")
        node = find_node(data["tree"], src_parts)
        if node is None:
            raise ToolError(f"条目不存在: {src}")
        if len(dst_parts) > len(src_parts) and dst_parts[: len(src_parts)] == src_parts:
            raise ToolError(f"目标不得位于源子树内（先移出再入内）: {src} ⊃ {dst}")
        if find_node(data["tree"], dst_parts) is not None:
            raise ToolError(f"目标条目已存在（mv 不覆盖，覆盖式更新用 add）: {dst}")
        _, dst_parent = self._resolve_for_write(data, dst)
        # 先挂载后摘除：同父重命名且源是父目录唯一孩子时，先摘会把共同父目录修剪后以空骨架重建、丢失其信息
        dst_parent["children"][dst_parts[-1]] = node
        self._remove_entry(data, src_parts, src)
        # 不做全树 rel 兜底校验：它会让树上任何既有悬空（rm 的合法产物）阻塞无关的 mv，
        # 而 mv 正是修复悬空的手段。重写本身是保存在性映射（旧目标在树中则新目标必在）；
        # 唯一例外是源端父链修剪——指向被修剪祖先的边会悬空（同 rm 口径，由 check 报 E 兜底）
        return self._rewrite_rel(data, src_key, dst_key)

    def _rewrite_rel(self, data, old_key, new_key) -> int:
        """全树把指向 old_key（含以其为前缀的子路径）的 rel 边重写为 new_key，返回重写的边数。"""
        n = 0
        for _path, node in walk_entries(data["tree"], []):
            rel = node.get("rel")
            if not rel:
                continue
            rewritten = []
            for r in rel:
                if r == old_key or r.startswith(old_key + "/"):
                    rewritten.append(new_key + r[len(old_key):])
                    n += 1
                else:
                    rewritten.append(r)
            if rewritten != rel:
                node["rel"] = rewritten
        return n

    # ---------- 批量（一次变更 = 一步历史，整批原子生效） ----------

    BATCH_ENTRY_FIELDS = frozenset({"path", "desc", "detail", "rel", "tags", "dir", "collapsed", "hidden", "git-ignore"})

    def _normalize_batch_entry(self, idx: int, entry) -> dict:
        """清单条目 → _apply_add 参数：字段全可选（语义同单条 add），类型不符即拒绝。"""
        if not isinstance(entry, dict):
            raise ToolError(f"add-batch 第 {idx} 条不是对象: {entry!r}")
        unknown = [k for k in entry if k not in self.BATCH_ENTRY_FIELDS]
        if unknown:
            raise ToolError(f"add-batch 条目含未知字段 {unknown}: {entry.get('path')!r}")
        path = entry.get("path")
        if not isinstance(path, str) or not path:
            raise ToolError(f"add-batch 第 {idx} 条 path 缺失或非字符串")

        def opt_list(field: str):
            val = entry.get(field)
            if val is None:
                return None
            if not isinstance(val, list) or not all(isinstance(x, str) for x in val):
                raise ToolError(f"add-batch 条目 {path} 的 {field} 须为字符串数组")
            return val

        desc = entry.get("desc")
        if desc is not None and not isinstance(desc, str):
            raise ToolError(f"add-batch 条目 {path} 的 desc 须为字符串")
        is_dir_entry = entry.get("dir")
        if is_dir_entry is None:  # 显式 null 与缺省同义（与 collapsed/hidden 一致）
            is_dir_entry = False
        if not isinstance(is_dir_entry, bool):
            raise ToolError(f"add-batch 条目 {path} 的 dir 须为布尔")
        spec = {"path": path, "desc": desc, "detail": opt_list("detail"), "rel": opt_list("rel"),
                "tags": opt_list("tags"), "is_dir_entry": is_dir_entry,
                "collapsed": None, "hidden": None}
        for field in ("collapsed", "hidden", "git-ignore"):
            val = entry.get(field)
            if val is not None and not isinstance(val, bool):
                raise ToolError(f"add-batch 条目 {path} 的 {field} 须为布尔")
            # JSON 键 "git-ignore" 映射为参数名 git_ignore（连字符不是合法标识符）
            spec["git_ignore" if field == "git-ignore" else field] = val
        return spec

    def add_batch(self, entries) -> int:
        """批量 upsert：内存上应用全部条目后一次快照、一次落盘；任一条非法整批拒绝（原子）。"""
        if not isinstance(entries, list) or not entries:
            raise ToolError("add-batch 清单须为非空 entries 数组")
        specs = [self._normalize_batch_entry(i + 1, e) for i, e in enumerate(entries)]
        seen: set[str] = set()
        for spec in specs:  # 判重用归一化路径（与 rm_batch 一致）：反斜杠/双斜杠变体同判
            key = "/".join(split_rel_path(spec["path"]))
            if key in seen:
                raise ToolError(f"批内重复路径: {spec['path']}")
            seen.add(key)
        data = self.load()
        notes: list[str] = []
        for spec in specs:
            note = self._apply_add(data, **spec)
            if note:
                notes.append(note)
        # rel 在最终树上统一校验：批内条目互引合法（check 的 rel 不变量同样在落盘前收口）
        for spec in specs:
            if spec["rel"]:
                self._validate_rel(data, spec["path"], spec["rel"])
        self._record_undo(f"add-batch {len(specs)} 条")
        self.write_data(data)
        # 提示只在落盘成功后打印：整批拒绝时无输出，与原子语义一致
        for note in notes:
            print(note)
        return len(specs)

    def rm_batch(self, paths) -> int:
        """批量删除：预校验（全部存在、无重复、无祖先-后代包含）后统一删除，任一非法整批拒绝。"""
        if not isinstance(paths, (list, tuple)) or not paths:
            raise ToolError("rm-batch 至少需要一个路径")
        for p in paths:
            if not isinstance(p, str) or not p:
                raise ToolError(f"rm-batch 路径非法: {p!r}")
        joined = ["/".join(split_rel_path(p)) for p in paths]
        if len(set(joined)) != len(joined):
            dup = sorted({p for p in joined if joined.count(p) > 1})
            raise ToolError(f"批内重复路径: {', '.join(dup)}")
        for a in joined:
            for b in joined:
                if a != b and b.startswith(a + "/"):
                    raise ToolError(f"批内路径互为祖先-后代（删祖先即覆盖后代）: {a} ⊃ {b}")
        all_parts = [split_rel_path(p) for p in paths]
        data = self.load()
        for parts in all_parts:  # 预校验全部存在，避免删一半才发现缺失
            parent = find_node(data["tree"], parts[:-1])
            if parent is None or not is_dir(parent) or parts[-1] not in parent["children"]:
                raise ToolError(f"条目不存在: {'/'.join(parts)}")
        for parts in all_parts:
            self._remove_entry(data, parts)
        self._record_undo(f"rm-batch {len(all_parts)} 条")
        self.write_data(data)
        return len(all_parts)

    MOVE_ENTRY_FIELDS = frozenset({"src", "dst"})

    def _normalize_move_entry(self, idx: int, entry) -> dict:
        """清单条目 → {src, dst}：恰含两个非空字符串字段，未知字段拒绝（同 add-batch 严格性）。"""
        if not isinstance(entry, dict):
            raise ToolError(f"mv-batch 第 {idx} 条不是对象: {entry!r}")
        unknown = [k for k in entry if k not in self.MOVE_ENTRY_FIELDS]
        if unknown:
            raise ToolError(f"mv-batch 条目含未知字段 {unknown}: {entry.get('src')!r}")
        src, dst = entry.get("src"), entry.get("dst")
        for field, val in (("src", src), ("dst", dst)):
            if not isinstance(val, str) or not val:
                raise ToolError(f"mv-batch 第 {idx} 条 {field} 缺失或非字符串")
        return {"src": src, "dst": dst}

    def mv_batch(self, moves) -> tuple[int, int]:
        """批量迁移：预校验批内 src/dst 双向互斥后逐条 _apply_mv，任一非法整批拒绝（原子）。

        返回 (条数, 重写边数)；重写边数按重写动作累计，批内叠加改写计多次（与逐条执行合计一致）。
        """
        if not isinstance(moves, list) or not moves:
            raise ToolError('mv-batch 清单须为非空 moves 数组，如 {"moves": [{"src": "a.ts", "dst": "b/a.ts"}]}')
        specs = [self._normalize_move_entry(i + 1, e) for i, e in enumerate(moves)]
        srcs = ["/".join(split_rel_path(s["src"])) for s in specs]
        dsts = ["/".join(split_rel_path(s["dst"])) for s in specs]
        if len(set(srcs)) != len(srcs):
            raise ToolError("批内 src 重复（多条移动同一源）")
        if len(set(dsts)) != len(dsts):
            raise ToolError("批内 dst 重复（多条移动到同一目的地）")
        for a in srcs:
            for b in srcs:
                if a != b and b.startswith(a + "/"):
                    raise ToolError(f"批内 src 互为祖先-后代（移祖先已覆盖后代）: {a} ⊃ {b}")
        for a in dsts:
            for b in dsts:
                if a != b and b.startswith(a + "/"):
                    raise ToolError(f"批内 dst 互为祖先-后代: {a} ⊃ {b}")
        for i, d in enumerate(dsts):
            for j, s in enumerate(srcs):
                if i != j and (d == s or d.startswith(s + "/")):
                    raise ToolError(f"目的地落在批内其他移动的源路径上（不支持移动链/嵌套目的地）: {d}")
        for i, s in enumerate(srcs):
            for j, d in enumerate(dsts):
                if i != j and (s == d or s.startswith(d + "/")):
                    raise ToolError(f"源路径落在批内其他移动的目的地上（后续条会看见前序结果）: {s}")
        # 单条四关（src==dst / src 存在 / dst 不存在 / 无自嵌套）不做静态预校验：上述双向互斥
        # 保证校验等价——src 不因前序挂载而出现、dst 不因前序修剪/挂载而变化，应用期校验即
        # 初始树校验；变换结果与逐条同序执行一致（dst 父链可能因前序修剪后重建为空骨架）
        data = self.load()
        edges = 0
        for spec in specs:
            edges += self._apply_mv(data, spec["src"], spec["dst"])
        self._record_undo(f"mv-batch {len(specs)} 条")
        self.write_data(data)
        return len(specs), edges

    # ---------- 子树批量标记 ----------

    def mark(self, dir_path, tags=None, tags_mode="add", git_ignore=None, depth=None) -> tuple[int, int, int]:
        """子树批量标记（一次变更单步历史）：tags 作用于子树全部条目（含目录），
        git-ignore 仅落文件条目且只给"未表态"者表态——显式设置（true/false）是个体
        意图不覆写；true 方向还跳过 git 已跟踪文件（落 true 即矛盾标记，check 必报），
        false 方向不跳（tracked 文件落显式 false 恰是退出祖先豁免的修复动作）。

        tags add=并集、replace=整体替换（空列表=清空）；depth 为相对锚点层数上限
        （1=直接子级），缺省全深度。目录条目不落 git-ignore——就近覆写继承下目录
        标记会穿透 depth 限制。返回 (tags 受影响条数, git-ignore 受影响条数, 跳过条数)；
        锚点自身与子树外条目不动。
        """
        if tags is None and git_ignore is None:
            raise ToolError("至少给一个动作参数（--tags 或 --git-ignore）")
        if tags_mode not in ("add", "replace"):
            raise ToolError(f"tags-mode 须为 add 或 replace: {tags_mode}")
        if depth is not None and depth < 1:
            raise ToolError(f"depth 必须是正整数: {depth}")
        parts = split_rel_path(dir_path)
        data = self.load()
        anchor = find_node(data["tree"], parts)
        if anchor is None or not is_dir(anchor):
            raise ToolError(f"锚点不是树中目录条目: {dir_path}")
        if not anchor["children"]:
            raise ToolError(f"目录未展开 children（粗粒度收录），无可传播条目: {dir_path}")
        if tags:
            vocab = set(data.get("tags", {}))
            unknown = [t for t in tags if t not in vocab]
            if unknown:
                raise ToolError(f"未知标签 {unknown}，先 tag-add 登记再使用")
        tracked = self._git_tracked() if git_ignore else None  # 仅 true 方向需要；不可用时无从跳过
        n_tags = n_git = n_skip = 0

        def apply(children: dict, prefix: list[str], rel_depth: int) -> None:
            nonlocal n_tags, n_git, n_skip
            if depth is not None and rel_depth > depth:
                return  # 剪枝：超出限定深度不再下探
            for name, child in children.items():
                if tags is not None:
                    old = child.get("tags", [])
                    if tags_mode == "add":
                        new = sorted(set(old) | set(tags), key=sort_key)
                    else:
                        new = sorted(set(tags), key=sort_key)
                    if new != old:
                        child["tags"] = new  # 空列表由 write_data 规范化移除键
                        n_tags += 1
                if git_ignore is not None and not is_dir(child):
                    path = "/".join(prefix + [name])
                    if "git-ignore" in child or (tracked is not None and git_ignore and path in tracked):
                        n_skip += 1  # 显式设置是个体表态不覆写；true 不落 tracked 文件
                    else:
                        child["git-ignore"] = git_ignore
                        n_git += 1
                if is_dir(child) and child["children"]:
                    apply(child["children"], prefix + [name], rel_depth + 1)

        apply(anchor["children"], parts, 1)
        self._record_undo(f"mark {dir_path}")
        self.write_data(data)
        return n_tags, n_git, n_skip

    # ---------- 词表 ----------

    def tag_add(self, name: str, desc: str) -> None:
        data = self.load()
        if name in data.get("tags", {}):
            raise ToolError(f"标签已存在: {name}")
        data.setdefault("tags", {})[name] = desc
        self._record_undo(f"tag-add {name}")
        self.write_data(data)

    def tag_rm(self, name: str) -> None:
        data = self.load()
        if name not in data.get("tags", {}):
            raise ToolError(f"标签不存在: {name}")
        in_use = [
            path
            for path, node in walk_entries(data["tree"], [])
            if name in node.get("tags", [])
        ]
        if in_use:
            raise ToolError(f"标签仍在使用，先清理条目: {', '.join(in_use)}")
        del data["tags"][name]
        self._record_undo(f"tag-rm {name}")
        self.write_data(data)

    # ---------- 根名（固定渲染首行，防 worktree 检出目录名漂移） ----------

    def current_root_name(self) -> tuple[str, str | None]:
        """返回 (生效根名, 自定义根名或 None)。未设置时自动取仓库根目录名。"""
        custom = self.load().get("root")
        return (custom or self.root_name), custom

    def set_root(self, name) -> None:
        if not isinstance(name, str) or not name:
            raise ToolError("root 名字必须是非空字符串")
        data = self.load()
        self._record_undo(f"root {name}")
        data["root"] = name
        self.write_data(data)

    def clear_root(self) -> None:
        data = self.load()
        if "root" not in data:
            raise ToolError("未设置自定义根名（当前已是自动模式）")
        self._record_undo("root --clear")
        del data["root"]
        self.write_data(data)

    # ---------- 查询 ----------

    def query(self, kw=None, tag=None, rel_of=None, under=None, depth=None) -> list[tuple[str, dict]]:
        """组合过滤。under 限定目录子树（锚点自身含入）；depth 为相对锚点层数上限，
        须与 under 同用——两者共同实现"这块目录前几层"的批量查询。"""
        data = self.load()
        under_parts: list[str] | None = None
        if under is not None:
            under_parts = split_rel_path(under)
            anchor = find_node(data["tree"], under_parts)
            if anchor is None or not is_dir(anchor):
                raise ToolError(f"--under 不是树中目录条目: {under}")
        if depth is not None:
            if under_parts is None:
                raise ToolError("--depth 须与 --under 同用（限定目录子树的相对层数）")
            if depth < 1:
                raise ToolError(f"--depth 必须是正整数: {depth}")
        results = []
        for path, node in walk_entries(data["tree"], []):
            parts = path.split("/")
            if under_parts is not None and parts[: len(under_parts)] != under_parts:
                continue
            if depth is not None and len(parts) - len(under_parts) > depth:
                continue
            if kw is not None:
                haystack = " ".join(
                    [path, node.get("desc", ""), " ".join(node.get("detail", []))]
                ).casefold()
                if kw.casefold() not in haystack:
                    continue
            if tag is not None and tag not in node.get("tags", []):
                continue
            if rel_of is not None and rel_of not in node.get("rel", []):
                continue
            results.append((path, node))
        return results

    # ---------- 渲染 ----------

    def render_brief_tree(self) -> str:
        name, _custom = self.current_root_name()
        return render_tree(name, self.load()["tree"])

    def render_tags_table(self) -> str:
        tags = self.load().get("tags", {})
        lines = ["| 标签 | 说明 |", "| --- | --- |"]
        for name, desc in sorted(tags.items(), key=lambda kv: sort_key(kv[0])):
            lines.append(f"| `{name}` | {desc} |")
        return "\n".join(lines)

    def _blocks(self) -> list[tuple[str, str, str, bool, object]]:
        """AGENTS.md 的两个渲染块：(begin, end, 附加时的小节标题, 是否 code fence, 内容函数)。"""
        return [
            (TREE_BEGIN, TREE_END, "## 文件树（简版速览）", True, self.render_brief_tree),
            (TAGS_BEGIN, TAGS_END, "## 文件树标签词表", False, self.render_tags_table),
        ]

    def _render_default_view(self) -> list[Path]:
        """渲染默认视图：AGENTS.md 的两个无 id 标记块（隐式视图，不占配置）。"""
        try:
            text = self.agents_md.read_text(encoding="utf-8")
        except FileNotFoundError:
            text = "# AGENTS\n"  # 无 AGENTS.md 时生成最小骨架
        new_text = text
        for begin, end, title, fenced, content_fn in self._blocks():
            content = content_fn()
            if begin in new_text:
                new_text = replace_block(new_text, begin, end, content)
            elif end in new_text:
                raise ToolError(f"{self.agents_md} 存在孤立结束标记（文件被改坏？）: {end}")
            else:
                parts = ["", title, ""]
                if fenced:
                    parts.append("```")
                parts += [begin, content, end]
                if fenced:
                    parts.append("```")
                new_text = new_text.rstrip("\n") + "\n" + "\n".join(parts) + "\n"
        if new_text != text or not self.agents_md.exists():
            with self.agents_md.open("w", encoding="utf-8", newline="\n") as f:
                f.write(new_text)
            return [self.agents_md]
        return []

    # ---------- 视图（多文档子树渲染） ----------

    @staticmethod
    def _filter_summary(filt: dict) -> str:
        """过滤器表达式树的单行摘要（view-list 展示用）。"""
        op = filt.get("op")
        if op == "under":
            return f"under {filt['path']}"
        if op == "tag":
            return f"tag {filt['tag']}"
        if op in ("and", "or"):
            return f"{op}(" + ", ".join(TreeTool._filter_summary(c) for c in filt["children"]) + ")"
        if op == "not":
            return f"not({TreeTool._filter_summary(filt['child'])})"
        return repr(filt)

    def _validate_view_filter(self, data: dict, filt: dict) -> None:
        """过滤器引用预检（写盘前；T1 校验时机裁定：只在创建入口，不进 normalize_data）。

        under 引用必须是树中已存在的目录条目、tag 必须已在词表登记，否则拒绝
        ——不落盘、不留撤销历史。数据操作（rm/mv 等）不经过此处，悬空配置是
        合法中间态，由渲染层跳过 + check 诊断兜底。
        """
        for path, tag in iter_filter_refs(filt):
            if path is not None:
                node = find_node(data["tree"], split_rel_path(path))
                if node is None or not is_dir(node):
                    raise ToolError(f"过滤器 under 引用不是树中目录条目: {path}")
            elif tag not in data.get("tags", {}):
                raise ToolError(f"过滤器 tag 引用未登记标签: {tag}（先 tag-add 登记再使用）")

    def _validate_view_overrides(self, data: dict, overrides: dict) -> None:
        """渲染覆盖引用预检（写盘前；与过滤器预检同时机）：路径必须在树中、
        collapsed 覆盖必须指向目录条目（与全局 collapsed 字段同严格性）。

        悬空覆盖（rm 后的合法中间态）由渲染层静默忽略 + check 诊断兜底，
        不阻塞数据操作。
        """
        for path in sorted(overrides, key=sort_key):
            node = find_node(data["tree"], split_rel_path(path))
            if node is None:
                raise ToolError(f"render_overrides 引用不在树中: {path}")
            if "collapsed" in overrides[path] and not is_dir(node):
                raise ToolError(f"render_overrides 的 collapsed 仅可用于目录条目（文件条目请用 hidden）: {path}")

    def _view_renderable(self, view_id: str, spec: dict, data: dict, quiet: bool = False) -> str | None:
        """返回剪影内容；不可渲染时打印告警并返回 None——渲染管线跳过该视图而非整体失败。

        悬空 under 引用（数据操作后的合法中间态，如锚点被 rm）跳过渲染，坏配置
        留给 view-list 呈现与 check 诊断；tag 引用未登记不在此拦截——自然求值为
        空集（空剪影仅剩根名行）。quiet=True 供 check 复用求值而不打渲染期
        告警（check 以自己的两级消息呈现）；render_overrides 原样传入剪影
        渲染：悬空覆盖项按路径查表天然落空（静默忽略），check 归诊断。
        """
        filt = spec.get("filter", {})
        dangling = sorted(
            {p for p, _tag in iter_filter_refs(filt) if p is not None and not is_tree_dir(data["tree"], p)},
            key=sort_key,
        )
        if dangling:
            if not quiet:
                print(f"警告: 视图 {view_id} 过滤器 under 引用不在树中或非目录: {', '.join(dangling)}，跳过渲染", file=sys.stderr)
            return None
        name, _custom = self.current_root_name()
        return render_silhouette(name, data["tree"], eval_filter(filt, data["tree"]), spec.get("render_overrides"))

    @staticmethod
    def _plan_view_block(text: str, begin: str, end: str, content: str, target_line: int | None, doc_rel: str) -> str:
        """单文档的块放置计划（纯函数）：已存在块原地替换或按行重定位，缺失则插入/追加。

        重定位（target_line 给定且块已存在）：先删除旧块（含围栏与紧邻空行），
        行号以删除后的文档为基准；dry-run 调用即校验（越界/孤立标记在此抛错）。
        病态拦截：同一文档内同 id 多于一个块（>1 个 begin 标记行）报错并指明
        文档路径与块数，不做猜测性修复；doc_rel 仅供错误信息定位。
        """
        n_blocks = text.split("\n").count(begin)
        if n_blocks > 1:
            raise ToolError(
                f"文档 {doc_rel} 存在 {n_blocks} 个同 id 标记块（恰好 1 个才可操作），"
                f"不做猜测性修复，请手改删除多余块后重试: {begin}"
            )
        if begin in text:
            if target_line is not None:
                return insert_block_at_line(
                    remove_view_block(text, begin, end), begin, end, content, target_line
                )
            return replace_block(text, begin, end, content)
        if end in text:
            raise ToolError(f"存在孤立结束标记（文件被改坏？）: {end}")
        if target_line is not None:
            return insert_block_at_line(text, begin, end, content, target_line)
        return append_view_block(text, begin, end, content)

    def _render_view(self, view_id: str, spec: dict, data: dict, target_line: int | None = None, target_doc: str | None = None) -> list[Path]:
        """把视图剪影渲染到全部绑定文档（镜像：块内容完全相同）。

        块已存在且 target_line 给定 → 重定位（删除后按行插入；行号以删除旧块后
        的文档为基准）；块已存在无 target_line → 原地替换内容；块不存在 →
        按 target_line 插入或追加尾部。target_line 仅作用于 target_doc 指定的
        文档（未指定时作用于全部绑定文档，兼容单文档清单的旧调用方）——
        view-doc --add 借此只让新文档按行落位、既有镜像原地刷新不重定位。
        绑定文档缺失时跳过（不凭空创建）。
        """
        content = self._view_renderable(view_id, spec, data)
        if content is None:
            return []
        begin, end = view_tree_markers(view_id)
        updated: list[Path] = []
        for doc_rel in spec.get("docs", []):
            path = self._doc_path(doc_rel)
            try:
                text = path.read_text(encoding="utf-8")
            except FileNotFoundError:
                print(f"警告: 视图 {view_id} 绑定文档不存在，跳过: {doc_rel}", file=sys.stderr)
                continue
            line_for_doc = target_line if (target_doc is None or doc_rel == target_doc) else None
            new_text = self._plan_view_block(text, begin, end, content, line_for_doc, doc_rel)
            if new_text != text:
                with path.open("w", encoding="utf-8", newline="\n") as f:
                    f.write(new_text)
                updated.append(path)
        return updated

    def render(self) -> list[Path]:
        updated: list[Path] = list(self._render_default_view())
        data = self.load()
        for view_id, spec in sorted(data.get("views", {}).items(), key=lambda kv: sort_key(kv[0])):
            updated += self._render_view(view_id, spec, data)
        return updated

    def _commit_view_change(self, view_id: str, candidate: dict, undo_label: str, undo_docs: dict | None,
                            target_line: int | None = None, target_doc: str | None = None) -> None:
        """视图配置变更的落盘尾部三步：撤销快照 → 写数据 → 重读渲染。

        重读落盘后的状态渲染（fresh），配置与渲染产物同源于磁盘；
        target_line/target_doc 语义同 _render_view（均为 None = 全部
        绑定文档原地刷新）。view-add / view-doc 三条写路径共用此管线。
        """
        self._record_undo(undo_label, docs=undo_docs)
        self.write_data(candidate)
        fresh = self.load()
        self._render_view(view_id, fresh["views"][view_id], fresh, target_line=target_line, target_doc=target_doc)

    def view_add(self, view_id: str, unders=None, tags=None, excludes=None, filt=None, doc=None, line=None,
                 overrides=None, collapse=None, expand=None, hide=None, show=None) -> None:
        """登记/更新视图：过滤器表达式树 + 渲染覆盖 + 绑定文档。

        过滤器来源二选一：filt（清单文件承载的任意布尔组合）或快捷参数编译
        （unders 多锚点并集、× tags 交集、减 excludes 差集 → 规范表达式树），
        两者互斥。渲染覆盖来源同样二选一：overrides（清单 dict）或快捷参数
        （collapse/expand → collapsed true/false、hide/show → hidden true/false），
        编译为 render_overrides 落盘（优先级：视图覆盖 > 条目全局字段 > 默认值，
        布尔双向）。写盘前预检：表达式结构规范化、引用校验（under 须树中目录、
        tag 须已登记、覆盖路径须在树中且 collapsed 指向目录）——拒绝不落盘不留
        历史；选中 0 条告警放行（空视图）。同 id 重复执行 = upsert：过滤器、
        绑定文档清单与覆盖配置整体替换为本次参数（不给覆盖参数 = 移除键）；
        块已存在时默认原地更新，--line 给定时重定位。一次变更 = 一步撤销历史
        （快照含受影响文档全文，undo/redo 连同块位置一并恢复）。
        """
        if line is not None and doc is None:
            raise ToolError("--line 须与 --doc 同用（行号是绑定文档内的落位参数）")
        if not isinstance(view_id, str) or not VIEW_ID_RE.fullmatch(view_id):
            raise ToolError(f"视图 id 语法非法（[a-z0-9][a-z0-9_-]{{0,63}}）: {view_id!r}")
        if view_id == VIEW_ID_RESERVED:
            raise ToolError("视图 id 'default' 是默认视图保留字，不可登记")
        unders = list(unders or [])
        tags = list(tags or [])
        excludes = list(excludes or [])
        if filt is not None:
            if unders or tags or excludes:
                raise ToolError("--filter 清单与快捷参数（--under/--tag/--exclude）互斥，二选一")
            expr = filt
        else:
            if not unders and not tags:
                raise ToolError("过滤器缺范围来源：至少给一个 --under 或 --tag，或改用 --filter 清单承载完整表达式")
            expr = compile_filter(unders, tags, excludes)
        expr = _normalize_filter(expr)  # 结构校验 + 归一（清单原始 JSON 收敛为规范形态）
        ov = compile_overrides(overrides, collapse, expand, hide, show)
        data = self.load()
        self._validate_view_filter(data, expr)  # 引用预检：拒绝保持原子（T1 拒绝原子性模式）
        self._validate_view_overrides(data, ov)
        docs_list: list[str] = []
        if doc is not None:
            doc_rel = "/".join(split_rel_path(doc))
            if not self._doc_path(doc_rel).is_file():
                raise ToolError(f"绑定文档不存在（先创建文档再登记视图）: {doc_rel}")
            docs_list = [doc_rel]
        if not eval_filter(expr, data["tree"]):
            print(f"警告: 视图 {view_id} 过滤器选中 0 条（空视图：渲染仅剩根名行）")
        spec = {"filter": expr}
        if docs_list:
            spec["docs"] = docs_list
        if ov:
            spec["render_overrides"] = ov
        candidate = views_candidate(data, data.get("views", {}), view_id, spec)
        normalize_data(candidate)  # 写前预检（结构非法的手改数据在此拦截，拒绝不留半截历史）
        if docs_list:  # 渲染计划 dry-run：行号越界/孤立标记/同 id 多块等在落盘前暴露，拒绝保持原子
            content = self._view_renderable(view_id, spec, data)
            if content is not None:
                begin, end = view_tree_markers(view_id)
                for doc_rel in docs_list:
                    text = self._read_doc(doc_rel)
                    if text is not None:
                        self._plan_view_block(text, begin, end, content, line, doc_rel)
        self._commit_view_change(
            view_id, candidate, f"view-add {view_id}",
            {doc_rel: self._read_doc(doc_rel)} if docs_list else None,
            target_line=line, target_doc=docs_list[0] if docs_list else None,
        )

    def view_doc(self, view_id: str, add: str | None = None, rm: str | None = None, line: int | None = None) -> None:
        """视图绑定文档的增量管理（视图须已由 view-add 登记）。

        --add：把新文档加入绑定清单并渲染（镜像语义：全清单块内容相同），
        --line 放置语义与 view-add 完全一致且仅作用于新文档（既有镜像原地
        刷新不重定位）；文档中有该 id 的孤儿块时原地激活刷新；重复绑定同一
        文档报错拒绝（重定位块请用 view-add 同 id upsert --line）。
        --rm：从清单解绑某文档并保留其中的块（孤儿状态，不再刷新），剩余
        绑定文档照常刷新保持镜像一致；一次操作只动一个文档。
        单步撤销历史：--add 快照目标文档全文（块位置不可重推导），--rm 不动
        文档无需快照。
        """
        if (add is None) == (rm is None):
            raise ToolError("--add 与 --rm 必须恰给其一")
        if line is not None and add is None:
            raise ToolError("--line 须与 --add 同用（行号是绑定文档内的落位参数）")
        data = self.load()
        views = data.get("views", {})
        if view_id not in views:
            raise ToolError(f"视图不存在: {view_id}（view-doc 只调整既有视图的绑定清单，创建用 view-add）")
        spec = views[view_id]
        if add is not None:
            doc_rel = "/".join(split_rel_path(add))
            if doc_rel in spec.get("docs", []):
                raise ToolError(f"视图 {view_id} 已绑定文档 {doc_rel}（重复绑定；调整块位置用 view-add 同 id --line）")
            if not self._doc_path(doc_rel).is_file():
                raise ToolError(f"绑定文档不存在（先创建文档再绑定视图）: {doc_rel}")
            new_spec = dict(spec)
            new_spec["docs"] = sorted(spec.get("docs", []) + [doc_rel], key=sort_key)
            candidate = views_candidate(data, views, view_id, new_spec)
            normalize_data(candidate)  # 写前预检（保持原子）
            # 渲染计划 dry-run 覆盖扩充后的全清单（不止新文档）：既有文档的
            # 病态多块/孤立标记同样在落盘前暴露；--line 仅校验新文档
            made = self._view_renderable(view_id, new_spec, data)
            if made is not None:
                begin, end = view_tree_markers(view_id)
                for other_rel in new_spec["docs"]:
                    text = self._read_doc(other_rel)
                    if text is not None:
                        self._plan_view_block(text, begin, end, made, line if other_rel == doc_rel else None, other_rel)
            self._commit_view_change(
                view_id, candidate, f"view-doc --add {view_id} {doc_rel}",
                {doc_rel: self._read_doc(doc_rel)},
                target_line=line, target_doc=doc_rel,
            )
        else:
            doc_rel = "/".join(split_rel_path(rm))
            docs = spec.get("docs", [])
            if doc_rel not in docs:
                bound = ", ".join(docs) if docs else "无"
                raise ToolError(f"视图 {view_id} 未绑定文档 {doc_rel}（当前绑定: {bound}）")
            if not self._doc_path(doc_rel).is_file():
                raise ToolError(
                    f"文档不存在: {doc_rel}（解绑保留块需要文档在磁盘上存在；"
                    f"文档已被删除时用 view-add 重建绑定清单移除悬空绑定）"
                )
            remaining = [d for d in docs if d != doc_rel]
            new_spec = {k: v for k, v in spec.items() if k != "docs"}
            if remaining:
                new_spec["docs"] = remaining  # 空 docs 省略键：视图退化为配置先行
            candidate = views_candidate(data, views, view_id, new_spec)
            normalize_data(candidate)
            # --rm 也是数据变更：剩余绑定文档照常刷新保持镜像一致
            # （解绑文档不在清单中，其保留的块不再被触碰；不动解绑文档无需 docs 快照）
            self._commit_view_change(view_id, candidate, f"view-doc --rm {view_id} {doc_rel}", None)

    def view_rm(self, view_id: str, purge: bool = False) -> int:
        """删除视图实体：默认仅从 views 配置删除（各绑定文档的块原样保留为
        孤儿，此后不再被任何渲染刷新）；--purge 连带按绑定清单逐一删除各
        文档中的块，返回实际清理的文档数。

        purge 原子拒绝：删前对全清单做前置校验——文档须在磁盘上存在、该
        id 的块须恰好一个（缺失或多个即报错并指明文档路径与现状），全部
        通过才开始删，不产生半删状态。只删标记行与块内内容（含包裹围栏
        与插入时补的保障空行，拼接点不产生双空行粘连），块外一字不动。
        一次变更 = 一步撤销历史（purge 快照全部绑定文档全文，undo 连同
        配置与块一并恢复）。
        """
        data = self.load()
        views = data.get("views", {})
        if view_id not in views:
            raise ToolError(f"视图不存在: {view_id}")
        docs_list = list(views[view_id].get("docs", []))
        planned: dict[str, str] = {}
        if purge:
            begin, end = view_tree_markers(view_id)
            for doc_rel in docs_list:  # 前置校验 + 变换计划：全部通过才动手
                text = self._read_doc(doc_rel)
                if text is None:
                    raise ToolError(
                        f"文档不存在: {doc_rel}（purge 删块需要文档在磁盘上存在；"
                        f"文档已被删除时其中的块已不在，可改用 view-rm 不带 --purge 仅删配置）"
                    )
                n_blocks = text.split("\n").count(begin)
                if n_blocks == 0:
                    raise ToolError(f"文档 {doc_rel} 缺视图 {view_id} 标记块（purge 按清单逐一删块，缺失即拒绝）: {begin}")
                if n_blocks > 1:
                    raise ToolError(
                        f"文档 {doc_rel} 存在 {n_blocks} 个视图 {view_id} 标记块"
                        f"（恰好 1 个才可 purge），请手改删除多余块后重试: {begin}"
                    )
                planned[doc_rel] = remove_view_block(text, begin, end)  # dry-run：孤立标记在此暴露
        remaining = {k: v for k, v in views.items() if k != view_id}
        candidate = dict(data)
        if remaining:
            candidate["views"] = remaining
        else:
            candidate.pop("views", None)  # 空 views 不落盘：回到无配置仓库形态
        self._record_undo(
            f"view-rm {view_id} --purge" if purge else f"view-rm {view_id}",
            docs=self._current_docs_snapshot(docs_list) if purge else None,
        )
        self.write_data(candidate)
        for doc_rel, new_text in planned.items():  # 数据写入在前、文档删除在后（对齐渲染管线次序）
            self._write_doc(doc_rel, new_text)
        return len(planned)

    def view_list(self) -> list[dict]:
        """全部视图概要：id / 过滤器摘要 / 绑定文档清单与块存在情况（按 id 排序）。"""
        data = self.load()
        result: list[dict] = []
        for view_id, spec in sorted(data.get("views", {}).items(), key=lambda kv: sort_key(kv[0])):
            begin, _end = view_tree_markers(view_id)
            docs = []
            for rel in spec.get("docs", []):
                text = self._read_doc(rel)
                docs.append({"doc": rel, "block": text is not None and begin in text})
            result.append({
                "id": view_id,
                "filter": self._filter_summary(spec.get("filter", {})),
                "docs": docs,
            })
        return result

    # ---------- check ----------

    def _git_files(self) -> set[str] | None:
        if self.git_files_override is not None:
            return self.git_files_override
        if not (self.repo_root / ".git").exists():
            return None
        proc = subprocess.run(
            ["git", "-c", "core.quotepath=off", "ls-files", "--cached", "--others", "--exclude-standard"],
            cwd=self.repo_root,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        if proc.returncode != 0:
            return None
        return {line for line in proc.stdout.splitlines() if line.strip()}

    def _git_tracked(self) -> set[str] | None:
        """git 已跟踪集合（ls-files --cached）：git-ignore 豁免条目的排除态校验基准。"""
        if self.git_tracked_override is not None:
            return self.git_tracked_override
        if not (self.repo_root / ".git").exists():
            return None
        proc = subprocess.run(
            ["git", "-c", "core.quotepath=off", "ls-files", "--cached"],
            cwd=self.repo_root,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        if proc.returncode != 0:
            return None
        return {line for line in proc.stdout.splitlines() if line.strip()}

    def _skill_dir_rel(self) -> str | None:
        """技能目录的仓库相对 posix 路径；不在仓库内（异常部署）返回 None。"""
        try:
            return self.tree_json.parent.relative_to(self.repo_root).as_posix()
        except ValueError:
            return None

    def _is_skill_pycache(self, path: str) -> bool:
        """技能目录内的 __pycache__（契约测试运行产物）：运行时缓存，豁免未收录告警。"""
        skill_rel = self._skill_dir_rel()
        return skill_rel is not None and path.startswith(skill_rel + "/") and "__pycache__/" in path

    def _iter_repo_markdown(self) -> list[tuple[str, Path]]:
        """仓库内全部 .md 文件（跳过 .git 与技能目录），按路径确定性排序。

        走磁盘遍历而非 git 清单：孤儿块恰恰常在未跟踪的手写文档里，扫描
        不应依赖 git 可用性。
        """
        skill_rel = self._skill_dir_rel()
        out: list[tuple[str, Path]] = []
        for dirpath, dirnames, filenames in os.walk(self.repo_root):
            dirnames[:] = sorted(d for d in dirnames if d != ".git")
            for name in filenames:
                if not name.lower().endswith(".md"):
                    continue
                path = Path(dirpath) / name
                rel = path.relative_to(self.repo_root).as_posix()
                if skill_rel is not None and rel.startswith(skill_rel + "/"):
                    continue
                out.append((rel, path))
        out.sort(key=lambda kv: sort_key(kv[0]))
        return out

    def _scan_orphan_view_markers(self, known_ids: set[str]) -> list[str]:
        """全仓库孤儿标记块扫描（告警级）：带 id 的 begin 标记行命中未登记 id 即告警。

        豁免两条：技能目录（技能自身文档的格式示例）；代码围栏内的示意行。
        围栏判定为逐行开关状态机——begin 紧贴围栏开行时视作块自身包裹围栏、
        不豁免（否则 view-rm 保留产物全被漏检）；围栏首行紧跟标记行的"完整
        块形态示意"与真块在文本层不可区分，同样按真块报告。begin 命中后跳读
        至 end 形态行，块内围栏行不扰动状态机。
        """
        warnings: list[str] = []
        for rel, path in self._iter_repo_markdown():
            try:
                text = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue  # 读不了的文件无从诊断，跳过不阻断
            lines = text.replace("\r\n", "\n").split("\n")
            fenced = False
            i = 0
            while i < len(lines):
                line = lines[i]
                if line.lstrip().startswith(FENCE):
                    fenced = not fenced
                    i += 1
                    continue
                m = VIEW_MARKER_BEGIN_RE.match(line)
                if m is None:
                    i += 1
                    continue
                view_id = m.group(1)
                if view_id not in known_ids:
                    prev_fence = i > 0 and lines[i - 1].lstrip().startswith(FENCE)
                    if prev_fence or not fenced:
                        warnings.append(
                            f"W: 未登记视图 id 的孤儿标记块: {rel}:{i + 1} id={view_id}"
                            f"（手写笔误或 view-rm 保留产物；登记用 view-add，或手改删除该块）"
                        )
                j = i + 1
                while j < len(lines) and VIEW_MARKER_END_RE.match(lines[j]) is None:
                    j += 1
                i = j + 1  # 截断文件无 end 时 j 越界，i 随之退出循环
        return warnings

    def check(self, strict: bool = False) -> tuple[list[str], list[str]]:
        errors: list[str] = []
        warnings: list[str] = []
        try:
            raw = self.tree_json.read_text(encoding="utf-8")
        except FileNotFoundError:
            return ([f"E: tree.json 不存在: {self.tree_json}"], [])
        text = raw.replace("\r\n", "\n")
        try:
            data = json.loads(text)
        except json.JSONDecodeError as exc:
            return ([f"E: tree.json 解析失败: {exc}"], [])
        try:
            normalize_data(data)
        except ToolError as exc:
            return ([f"E: tree.json 结构非法: {exc}"], [])
        if not is_canonical_text(text):
            errors.append("E: tree.json 非脚本规范形态（键序/缩进/空字段），请只通过脚本命令修改")

        vocab = data.get("tags", {})
        tree = data.get("tree", {})
        known = set(FIELD_ORDER)
        all_paths: list[str] = []
        file_paths: set[str] = set()
        rel_refs: list[tuple[str, str]] = []

        def validate(node: dict, path: str) -> None:
            unknown = [k for k in node if k not in known]
            if unknown:
                errors.append(f"E: {path or '根'} 含未知字段 {unknown}")
            if "desc" not in node:
                errors.append(f"E: {path} 缺 desc")
            elif not node["desc"]:
                warnings.append(f"W: {path} desc 为空（目录待补一句话介绍）")
            elif len(node["desc"]) > DESC_MAX:
                warnings.append(f"W: {path} desc 超长（{len(node['desc'])}>{DESC_MAX}）")
            if not is_dir(node) and "detail" not in node:
                warnings.append(f"W: {path} 缺 detail（完整描述待补，详版树将回退 desc）")
            for tag in node.get("tags", []):
                if tag not in vocab:
                    errors.append(f"E: {path} 使用未登记标签 '{tag}'")
            for ref in node.get("rel", []):
                rel_refs.append((path, ref))
            if not is_dir(node):
                file_paths.add(path)

        for path, node in walk_entries(tree, []):
            all_paths.append(path)
            validate(node, path)
        path_set = set(all_paths)
        for src, ref in rel_refs:
            if ref == src:
                errors.append(f"E: {src} rel 指向自身")
            elif ref not in path_set:
                errors.append(f"E: {src} rel 目标不在树中: {ref}")

        # 视图渲染覆盖引用（T1 时机裁定：悬空不阻塞数据操作，check 诊断兜底）
        for view_id, spec in sorted(data.get("views", {}).items(), key=lambda kv: sort_key(kv[0])):
            for ov_path in sorted(spec.get("render_overrides", {}), key=sort_key):
                node = find_node(tree, split_rel_path(ov_path))
                if node is None:
                    errors.append(f"E: 视图 {view_id} render_overrides 引用不在树中: {ov_path}")
                elif "collapsed" in spec["render_overrides"][ov_path] and not is_dir(node):
                    errors.append(f"E: 视图 {view_id} render_overrides 的 collapsed 仅可用于目录条目: {ov_path}")

        git_files = self._git_files()
        if git_files is None:
            pass  # 非 git 环境静默跳过磁盘对照，由 CLI 层提示
        else:
            tracked = self._git_tracked()  # tracked ⊆ git_files；None = git 不可用，豁免条目退化为仅校验磁盘存在
            exempt = {p for p in file_paths if effective_git_ignore(tree, p)}
            for missing in sorted(file_paths - git_files - exempt, key=sort_key):
                disk = self.repo_root.joinpath(*split_rel_path(missing))
                if disk.is_dir():
                    # git ls-files 只列文件不列目录：此实况是类型错配而非路径悬空
                    errors.append(f'E: {missing} 磁盘上是目录，树中却是文件条目（add --dir 或清单 "dir": true 修正）')
                elif disk.exists():
                    errors.append(f"E: 树中条目未被 git 跟踪: {missing}")
                else:
                    errors.append(f"E: 树中条目未被 git 跟踪且磁盘不存在: {missing}")

            # git-ignore 豁免条目：只校验磁盘存在，并要求确实排除在 git 之外。
            # 排除态矛盾（被跟踪）不会进上面的差集循环——tracked ⊆ git_files——必须在此单独拦截
            for p in sorted(exempt, key=sort_key):
                disk = self.repo_root.joinpath(*split_rel_path(p))
                if disk.is_dir():
                    errors.append(f'E: {p} 磁盘上是目录，树中却是文件条目（add --dir 或清单 "dir": true 修正）')
                elif not disk.exists():
                    errors.append(f"E: {p} git-ignore 条目磁盘不存在")
                elif tracked is not None and p in tracked:
                    errors.append(f"E: {p} 标记 git-ignore 但实际被 git 跟踪（git rm --cached 或移除标记恢复对照）")
                elif tracked is not None and p in git_files:
                    # 在 git_files 又不在 tracked = 未跟踪且未被 ignore：git status 会持续显示 untracked
                    errors.append(f"E: {p} 标记 git-ignore 但未被 .gitignore 排除（补 ignore 规则或移除标记）")

            def reported_if(f: str) -> bool:
                """祖先整目录收录（在树中但未展开）则不报，否则报未收录。"""
                cursor: dict = {"children": tree}
                parts = split_rel_path(f)
                for part in parts[:-1]:
                    child = cursor["children"].get(part)
                    if child is None:
                        return True
                    if not is_dir(child) or not child["children"]:
                        return False  # 整目录收录
                    cursor = child
                return True

            unrecorded = sorted(
                (
                    f
                    for f in git_files - file_paths
                    if reported_if(f) and not self._is_skill_pycache(f)
                ),
                key=sort_key,
            )
            for f in unrecorded:
                warnings.append(f"W: git 文件未收录进树: {f}")

        # 产物一致性（AGENTS.md 两个标记块）
        try:
            disk = self.agents_md.read_text(encoding="utf-8").replace("\r\n", "\n")
        except FileNotFoundError:
            errors.append(f"E: 渲染产物缺失: {self.agents_md}（运行 render 生成）")
            disk = None
        if disk is not None:
            for begin, end, _title, _fenced, content_fn in self._blocks():
                try:
                    actual = block_content(disk, begin, end)
                except ToolError:
                    errors.append(f"E: {self.agents_md.name} 缺标记块 {begin}（运行 render 附加）")
                    continue
                if actual != content_fn():
                    errors.append(f"E: {self.agents_md.name} 标记块内容与 tree.json 不一致（产物过期或被手改），运行 render")

        # 视图产物一致性（views × 绑定文档）：缺块 / 同 id 多块 / 块内容漂移。
        # 不可渲染视图（锚点悬空等渲染期告警的同类病态）降为告警，与渲染期两级一致
        views = data.get("views", {})
        for view_id, spec in sorted(views.items(), key=lambda kv: sort_key(kv[0])):
            begin, end = view_tree_markers(view_id)
            made = self._view_renderable(view_id, spec, data, quiet=True)
            if made is None:
                warnings.append(
                    f"W: 视图 {view_id} 当前不可渲染（过滤器 under 引用不在树中或非目录），跳过其绑定文档的块内容比对"
                )
            elif not eval_filter(spec.get("filter", {}), data["tree"]):
                # US19 后半句：空选中在 check 告警（识别"过滤器过窄"），空视图渲染仅剩根名行
                warnings.append(f"W: 视图 {view_id} 过滤器选中 0 条（过滤器过窄或数据变更后失去全部条目），空视图渲染仅剩根名行")
            for doc_rel in spec.get("docs", []):
                try:
                    disk_doc = self._doc_path(doc_rel).read_text(encoding="utf-8").replace("\r\n", "\n")
                except FileNotFoundError:
                    errors.append(
                        f"E: 视图 {view_id} 绑定文档不存在: {doc_rel}"
                        f"（恢复文档，或用 view-add 重建绑定清单移除悬空绑定）"
                    )
                    continue
                doc_lines = disk_doc.split("\n")
                n_begin, n_end = doc_lines.count(begin), doc_lines.count(end)
                if n_begin > 1:
                    errors.append(
                        f"E: 视图 {view_id} 文档 {doc_rel} 存在 {n_begin} 个同 id 标记块（恰好 1 个），"
                        f"手改删除多余块后重跑 render"
                    )
                    continue
                if n_begin == 0:
                    if n_end > 0:
                        errors.append(f"E: 视图 {view_id} 文档 {doc_rel} 缺开始标记且残留孤立结束标记（文件被改坏？）")
                    else:
                        errors.append(
                            f"E: 视图 {view_id} 文档 {doc_rel} 缺标记块，重跑 view-add {view_id} 或任一数据命令可纠正"
                        )
                    continue
                if n_end == 0:
                    errors.append(f"E: 视图 {view_id} 文档 {doc_rel} 缺结束标记（文件被改坏？）")
                    continue
                if made is not None and block_content(disk_doc, begin, end) != made:
                    errors.append(
                        f"E: 视图 {view_id} 文档 {doc_rel} 标记块内容与渲染产物不一致（产物过期或被手改），"
                        f"重跑 view-add {view_id} 或任一数据命令可纠正"
                    )

        # 孤儿标记块扫描（全仓库 .md，豁免技能目录与代码围栏内示意行）
        warnings.extend(self._scan_orphan_view_markers(set(views)))

        # 历史位置收敛提示：legacy 残留说明仓库初始化晚于技能使用，待迁移
        for legacy in self.legacy_history_paths:
            if legacy != self.history_path and legacy.exists():
                warnings.append(
                    f"W: 历史文件位于旧位置 {legacy}（仓库在技能使用之后初始化？），"
                    f"执行任一维护命令将自动迁移到 {self.history_path} 并删除旧文件"
                )

        if strict:
            return (errors + [f"E(strict): {w}" for w in warnings], [])
        return errors, warnings


# ---------- CLI ----------


def _cmd_add(tool: TreeTool, args) -> None:
    tool.add(
        args.path,
        desc=args.desc,
        detail=args.detail,
        rel=args.rel,
        tags=[t.strip() for t in args.tags.split(",") if t.strip()] if args.tags else None,
        is_dir_entry=args.dir,
        collapsed=args.collapsed,
        hidden=args.hidden,
        git_ignore=args.git_ignore,
    )
    tool.render()
    print(f"已写入并重渲染: {args.path}")


def _cmd_rm(tool: TreeTool, args) -> None:
    tool.rm(args.path)
    tool.render()
    print(f"已删除并重渲染: {args.path}")


def _cmd_mv(tool: TreeTool, args) -> None:
    n = tool.mv(args.src, args.dst)
    tool.render()
    suffix = f"（重写 {n} 条 rel 边）" if n else ""
    print(f"已迁移并重渲染: {args.src} -> {args.dst}{suffix}")


def _cmd_mv_batch(tool: TreeTool, args) -> None:
    obj = _load_manifest(
        args.manifest,
        "moves",
        '清单顶层须为对象且含 "moves" 数组，如 {"moves": [{"src": "a.ts", "dst": "b/a.ts"}]}',
        value_is_list=True,
    )
    n, edges = tool.mv_batch(obj["moves"])
    tool.render()
    parts = [f"重写 {edges} 条 rel 边", "一次变更，单步历史"] if edges else ["一次变更，单步历史"]
    print(f"已批量迁移并重渲染: {n} 条（{'；'.join(parts)}）")


def _cmd_add_batch(tool: TreeTool, args) -> None:
    obj = _load_manifest(
        args.manifest,
        "entries",
        '清单顶层须为对象且含 "entries" 数组，如 {"entries": [{"path": "a.ts", "desc": "简介"}]}',
        value_is_list=True,
    )
    n = tool.add_batch(obj["entries"])
    tool.render()
    print(f"已批量写入并重渲染: {n} 条（一次变更，单步历史）")


def _cmd_rm_batch(tool: TreeTool, args) -> None:
    n = tool.rm_batch(list(args.paths))
    tool.render()
    print(f"已批量删除并重渲染: {n} 条（一次变更，单步历史）")


def _cmd_root(tool: TreeTool, args) -> None:
    if args.clear and args.name:
        raise ToolError("--clear 与名字不能同时给出")
    if args.clear:
        tool.clear_root()
        tool.render()
        print("已清除自定义根名，恢复自动取仓库根目录名并重渲染")
    elif args.name:
        tool.set_root(args.name)
        tool.render()
        print(f"已固定根名并重渲染: {args.name}")
    else:
        effective, custom = tool.current_root_name()
        if custom is not None:
            print(f"当前根名: {effective}（自定义；--clear 恢复自动）")
        else:
            print(f"当前根名: {effective}（自动 = 仓库根目录名；建议 root <名> 固定，防 worktree 目录名漂移）")


def _cmd_get(tool: TreeTool, args) -> None:
    vocab = tool.load().get("tags", {})
    for i, path in enumerate(args.path):
        if i:
            print()  # 多路径条间空行分隔；单路径输出与历史格式一致
        node = tool.get(path)
        print(path)
        print(f"  类型: {'目录' if is_dir(node) else '文件'}")
        if node.get("collapsed"):
            print("  collapsed: true（简版树折叠渲染，不展开 children）")
        if node.get("hidden"):
            print("  hidden: true（简版树隐藏渲染，条目及子树不出现）")
        if "git-ignore" in node:
            if node["git-ignore"]:
                print("  git-ignore: true（豁免 git 跟踪对照，check 只校验磁盘存在与 git 排除态；子树未覆写则继承）")
            else:
                print("  git-ignore: false（显式退出祖先豁免，check 恢复必须被 git 跟踪的对照；子树未覆写则同样退出）")
        print(f"  desc: {node.get('desc', '')}")
        if node.get("detail"):
            print("  detail:")
            for line in node["detail"]:
                print(f"    - {line}")
        if node.get("rel"):
            print("  rel:")
            for ref in node["rel"]:
                print(f"    - {ref}")
        if node.get("tags"):
            rendered = ", ".join(f"{t}（{vocab.get(t, '?')}）" for t in node["tags"])
            print(f"  tags: {rendered}")


def _cmd_mark(tool: TreeTool, args) -> None:
    # --tags "" 是显式空列表（配合 replace 清空），不折算为 None；缺省（不给参数）才是"不动"
    tags = None if args.tags is None else [t.strip() for t in args.tags.split(",") if t.strip()]
    n_tags, n_git, n_skip = tool.mark(
        args.path,
        tags=tags,
        tags_mode=args.tags_mode,
        git_ignore=args.git_ignore,
        depth=args.depth,
    )
    tool.render()
    # 跳过原因按方向表述：false 方向不跳 tracked（tracked 落显式 false 是修复动作），只剩显式设置一类
    reason = "显式设置/git 已跟踪不覆写" if args.git_ignore else "显式设置不覆写"
    skip_note = f"，跳过 {n_skip} 条（{reason}）" if n_skip else ""
    print(f"已批量标记并重渲染: {args.path}（tags {n_tags} 条，git-ignore {n_git} 条{skip_note}；一次变更，单步历史）")


def _cmd_query(tool: TreeTool, args) -> None:
    results = tool.query(kw=args.kw, tag=args.tag, rel_of=args.rel_of, under=args.under, depth=args.depth)
    if args.json:
        payload = [
            {
                "path": path,
                "kind": "dir" if is_dir(node) else "file",
                "desc": node.get("desc", ""),
                "detail": node.get("detail", []),
                "rel": node.get("rel", []),
                "tags": node.get("tags", []),
                "collapsed": node.get("collapsed", False),
                "hidden": node.get("hidden", False),
                # git-ignore 是三态字段：null=缺省（继承祖先），false=显式退出豁免——二态默认值会把两者拍平
                "git-ignore": node.get("git-ignore"),
            }
            for path, node in results
        ]
        print(json.dumps(payload, ensure_ascii=False, indent=2))
        return
    for path, node in results:
        kind = "/" if is_dir(node) else ""
        tags = f" [{' '.join(node['tags'])}]" if node.get("tags") else ""
        print(f"{path}{kind} — {node.get('desc', '')}{tags}")
    print(f"共 {len(results)} 条")


def _cmd_tag_add(tool: TreeTool, args) -> None:
    tool.tag_add(args.name, args.desc)
    tool.render()
    print(f"已登记标签并重渲染: {args.name}")


def _cmd_tag_rm(tool: TreeTool, args) -> None:
    tool.tag_rm(args.name)
    tool.render()
    print(f"已删除标签并重渲染: {args.name}")


def _load_manifest(manifest_str: str, key: str, shape_msg: str, value_is_list: bool = False) -> dict:
    """读取 JSON 清单并校验顶层形态：对象且含 key 键（value_is_list 时键值还须为数组）。

    读文件 / JSON 解析 / 顶层形态三段错误消息全库统一；键值的语义校验归各命令。
    """
    manifest = Path(manifest_str)
    try:
        raw = manifest.read_text(encoding="utf-8")
    except OSError as exc:
        raise ToolError(f"清单文件不可读: {manifest}（{exc}）") from exc
    try:
        obj = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ToolError(f"清单 JSON 解析失败: {exc}") from exc
    if not isinstance(obj, dict) or key not in obj or (value_is_list and not isinstance(obj[key], list)):
        raise ToolError(shape_msg)
    return obj


def _load_filter_manifest(manifest_str: str):
    """读取 --filter 清单：顶层对象含 filter 键（表达式树原样交由 view-add 校验归一）。"""
    return _load_manifest(
        manifest_str,
        "filter",
        '清单顶层须为对象且含 "filter" 键，如 {"filter": {"op": "under", "path": "apps"}}',
    )["filter"]


def _load_overrides_manifest(manifest_str: str):
    """读取 --overrides 清单：顶层对象含 overrides 键（覆盖表原样交由 view-add 校验归一）。"""
    return _load_manifest(
        manifest_str,
        "overrides",
        '清单顶层须为对象且含 "overrides" 键，如 {"overrides": {"apps/ui": {"collapsed": true}}}',
    )["overrides"]


def _cmd_view_add(tool: TreeTool, args) -> None:
    filt = _load_filter_manifest(args.filter) if args.filter else None
    overrides = _load_overrides_manifest(args.overrides) if args.overrides else None
    tool.view_add(
        args.view_id,
        unders=args.under,
        tags=args.tag,
        excludes=args.exclude,
        filt=filt,
        doc=args.doc,
        line=args.line,
        overrides=overrides,
        collapse=args.collapse,
        expand=args.expand,
        hide=args.hide,
        show=args.show,
    )
    target = f" -> {args.doc}" + (f"（围栏首行第 {args.line} 行）" if args.line else "") if args.doc else ""
    print(f"已登记视图并渲染: {args.view_id}{target}（一次变更，单步历史）")


def _cmd_view_doc(tool: TreeTool, args) -> None:
    tool.view_doc(args.view_id, add=args.add, rm=args.rm, line=args.line)
    if args.add:
        suffix = f"（围栏首行第 {args.line} 行）" if args.line else ""
        print(f"已绑定并渲染镜像块: {args.view_id} -> {args.add}{suffix}（一次变更，单步历史）")
    else:
        print(f"已解绑（文档中的块保留为孤儿，不再刷新）: {args.view_id} -x- {args.rm}（一次变更，单步历史）")


def _cmd_view_rm(tool: TreeTool, args) -> None:
    n = tool.view_rm(args.view_id, purge=args.purge)
    tool.render()
    if args.purge:
        print(f"已删除视图并清理 {n} 个绑定文档中的块: {args.view_id}（一次变更，单步历史）")
    else:
        print(f"已删除视图（各绑定文档中的块保留为孤儿，不再刷新）: {args.view_id}（一次变更，单步历史）")


def _cmd_view_list(tool: TreeTool, args) -> None:
    views = tool.view_list()
    if not views:
        print("无视图（仅默认视图：不带 id 的标记块渲染于 AGENTS.md）")
        return
    print(f"共 {len(views)} 个视图：")
    for v in views:
        print(f"- {v['id']}")
        print(f"    过滤器: {v['filter']}")
        print(f"    绑定文档: {len(v['docs'])}")
        for d in v["docs"]:
            state = "块存在" if d["block"] else "块缺失"
            print(f"      {d['doc']} [{state}]")


def _cmd_undo(tool: TreeTool, args) -> None:
    op = tool.undo()
    print(f"已撤销: {op}（redo 可重做）")


def _cmd_redo(tool: TreeTool, args) -> None:
    op = tool.redo()
    print(f"已重做: {op}")


def _cmd_history(tool: TreeTool, args) -> None:
    undo_ops, redo_ops = tool.history_summary()
    if not undo_ops and not redo_ops:
        print("历史为空（无操作记录）")
        return
    for i, op in enumerate(undo_ops, 1):
        print(f"  {'>' if i == len(undo_ops) else ' '} {i}. 可撤销: {op}")
    for op in reversed(redo_ops):
        print(f"    可重做: {op}")


def _cmd_check(tool: TreeTool, args) -> int:
    errors, warnings = tool.check(strict=args.strict)
    if tool._git_files() is None:
        print("提示: 非 git 仓库或 git 不可用，已跳过与磁盘的对照")
    for line in warnings:
        print(line)
    for line in errors:
        print(line)
    total = len(errors) + len(warnings)
    if total == 0:
        print("check 通过：规范形态、词表、rel、磁盘对照、渲染产物与视图块全部一致")
        return 0
    print(f"check 发现 {len(errors)} 错误 / {len(warnings)} 告警")
    return 1


def _cmd_render(tool: TreeTool, args) -> None:
    updated = tool.render()
    for path in updated:
        print(f"已更新: {path}")
    if not updated:
        print("产物均已最新，无改动")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description="项目文件树唯一维护入口")
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("add", help="新增/更新条目（自动建父目录，写后自动渲染）")
    p.add_argument("path", help="仓库相对路径，如 apps/cli/src/main.rs")
    p.add_argument("-d", "--desc", help="一句话介绍（≤20 字）")
    p.add_argument("--detail", action="append", help="完整描述一行，可重复")
    p.add_argument("--rel", action="append", help="相关文件路径，可重复")
    p.add_argument("--tags", help="逗号分隔的受控标签")
    p.add_argument("--dir", action="store_true", help="收录为目录条目（粗粒度收录不展开 children；磁盘目录未声明时也会自动识别）")
    p.add_argument(
        "--collapsed",
        action=argparse.BooleanOptionalAction,
        help="目录折叠渲染（简版树带 … 不展开 children），--no-collapsed 取消",
    )
    p.add_argument(
        "--hidden",
        action=argparse.BooleanOptionalAction,
        help="隐藏渲染（简版树中条目及子树不出现），--no-hidden 取消",
    )
    p.add_argument(
        "--git-ignore",
        dest="git_ignore",
        action=argparse.BooleanOptionalAction,
        help="豁免 git 跟踪对照（收录 .gitignore 排除的本地大文件等），check 改为校验磁盘存在与 git 排除态；目录标记子树继承，--no-git-ignore 取消",
    )

    p = sub.add_parser("add-batch", help="批量新增/更新（JSON 清单）：一次变更单步历史，任一条非法整批拒绝")
    p.add_argument("manifest", help='清单 JSON 路径，顶层为 {"entries": [{"path": "a.ts", "desc": "简介"}, ...]}')

    p = sub.add_parser("rm", help="删除条目并修剪空父目录")
    p.add_argument("path")

    p = sub.add_parser("rm-batch", help="批量删除条目：一次变更单步历史，任一条不存在整批拒绝")
    p.add_argument("paths", nargs="+", help="仓库相对路径，可多个")

    p = sub.add_parser("mv", help="条目带信息迁移（含子树），自动重写指向旧路径的 rel 边；不移动磁盘文件")
    p.add_argument("src", help="原路径")
    p.add_argument("dst", help="新路径（不得为已存在路径、不得位于源子树内）")

    p = sub.add_parser("mv-batch", help="批量迁移（JSON 清单）：一次变更单步历史；批内 src/dst 互斥预校验，任一条非法整批拒绝")
    p.add_argument("manifest", help='清单 JSON 路径，顶层为 {"moves": [{"src": "a.ts", "dst": "b/a.ts"}, ...]}')

    p = sub.add_parser("get", help="查看条目（可给多个路径批量查看）")
    p.add_argument("path", nargs="+", help="仓库相对路径，可多个")

    p = sub.add_parser("query", help="组合查询")
    p.add_argument("--kw", help="关键词（匹配路径/desc/detail）")
    p.add_argument("--tag", help="标签过滤")
    p.add_argument("--rel-of", dest="rel_of", help="反查：谁关联到此路径")
    p.add_argument("--under", help="限定目录子树（锚点自身含入；须为树中目录条目）")
    p.add_argument("--depth", type=int, help="相对 --under 的层数上限（1=直接子级）；须与 --under 同用")
    p.add_argument("--json", action="store_true", help="机器可读输出")

    p = sub.add_parser("mark", help="子树批量标记：tags 追加/覆写与 git-ignore 传播到目录子树（可限深度）")
    p.add_argument("path", help="树中已展开 children 的目录条目（锚点自身不动）")
    p.add_argument("--tags", help='逗号分隔标签；空串 "" 配 --tags-mode replace 表示清空')
    p.add_argument("--tags-mode", choices=["add", "replace"], default="add", help="add=并集追加（默认），replace=整体替换")
    p.add_argument(
        "--git-ignore",
        dest="git_ignore",
        action=argparse.BooleanOptionalAction,
        help="传播到子树文件条目（目录不落标记，防继承穿透 depth）；--no-git-ignore 落盘显式 false（批量退出豁免）",
    )
    p.add_argument("--depth", type=int, help="相对锚点层数上限（1=直接子级），缺省全深度")

    p = sub.add_parser("tag-add", help="登记受控标签")
    p.add_argument("name")
    p.add_argument("-d", "--desc", required=True)

    p = sub.add_parser("tag-rm", help="删除受控标签（被使用时拒绝）")
    p.add_argument("name")

    p = sub.add_parser(
        "view-add",
        help="登记/更新视图：把过滤器选中集的剪影渲染到绑定文档的带 id 标记块",
    )
    p.add_argument("view_id", help="视图 id：[a-z0-9][a-z0-9_-]{0,63}，保留字 default 不可用")
    p.add_argument("--under", action="append", help="目录锚点（树中已存在的目录条目），可重复：多锚点为并集")
    p.add_argument("--tag", action="append", help="已登记标签，可重复：锚点×标签为交集（多标签同为交集）")
    p.add_argument("--exclude", action="append", help="排除的目录锚点（差集：选中集减其子树），可重复")
    p.add_argument(
        "--filter",
        help='过滤器清单 JSON 路径（任意布尔组合），顶层为 {"filter": {"op": "and", ...}}；与快捷参数互斥',
    )
    p.add_argument("--doc", help="绑定文档（仓库相对路径，需已存在；缺省仅落盘配置不渲染）")
    p.add_argument("--line", type=int, help="围栏首行落点：插在当前第 N 行内容之前（1-based，越界报错）；省略则块存在原地更新、缺失追加文档尾部；同 id 已有块时重定位以删除旧块后的行号为准")
    p.add_argument("--collapse", action="append", metavar="PATH", help="视图内折叠的目录（树中已存在的目录条目），可重复：本视图渲染带 … 不展开")
    p.add_argument("--expand", action="append", metavar="PATH", help="视图内展开的目录（反向覆盖全局 collapsed），可重复")
    p.add_argument("--hide", action="append", metavar="PATH", help="视图内隐藏的条目（含其子树），可重复")
    p.add_argument("--show", action="append", metavar="PATH", help="视图内显示的条目（反向覆盖全局 hidden），可重复")
    p.add_argument(
        "--overrides",
        help='渲染覆盖清单 JSON 路径（多覆盖项走清单），顶层为 {"overrides": {"apps/ui": {"collapsed": true, "hidden": false}}}；'
        "与快捷参数（--collapse/--expand/--hide/--show）互斥；优先级：视图覆盖 > 条目全局字段 > 默认值，布尔双向",
    )

    sub.add_parser("view-list", help="列出全部视图概要（id / 过滤器摘要 / 绑定文档数 / 块存在情况）")

    p = sub.add_parser(
        "view-doc",
        help="调整既有视图的绑定文档清单（增量）：--add 绑定新文档并渲染镜像块，--rm 解绑但保留文档中的块",
    )
    p.add_argument("view_id", help="既有视图 id（创建视图用 view-add）")
    group = p.add_mutually_exclusive_group(required=True)
    group.add_argument("--add", metavar="DOC", help="绑定文档（仓库相对路径，需已存在且未绑定；有孤儿块时原地激活刷新）")
    group.add_argument("--rm", metavar="DOC", help="解绑文档（从清单移除，默认保留文档中的块为孤儿）")
    p.add_argument(
        "--line", type=int,
        help="围栏首行落点（仅与 --add 同用）：插在当前第 N 行内容之前（1-based，越界报错）；省略则块存在原地更新、缺失追加文档尾部；仅作用于本次绑定的文档",
    )

    p = sub.add_parser(
        "view-rm",
        help="删除视图：默认仅删配置（绑定文档中的块保留为孤儿不再刷新）；--purge 连带删除各绑定文档中的块",
    )
    p.add_argument("view_id", help="既有视图 id")
    p.add_argument(
        "--purge", action="store_true",
        help="按绑定清单逐一删除各文档中的块：删前校验每文档恰好一个（缺失或多个即报错原子拒绝），只删标记行与块内内容，块外一字不动",
    )

    p = sub.add_parser("check", help="校验全部不变量")
    p.add_argument("--strict", action="store_true", help="告警也视为失败")

    sub.add_parser("undo", help="撤销最近一次数据变更（恢复后自动重渲染）")
    sub.add_parser("redo", help="重做最近一次撤销")
    sub.add_parser("history", help="查看可撤销/可重做的操作概要")

    sub.add_parser("render", help="重渲染 AGENTS.md 两个标记块（缺标记自动附加到尾部，无文件则生成）")

    p = sub.add_parser("root", help="查看/固定/清除渲染根名（建议初始化后固定，防 worktree 检出目录名漂移）")
    p.add_argument("name", nargs="?", help="固定根名；省略则查看当前")
    p.add_argument("--clear", action="store_true", help="清除自定义根名，恢复自动取仓库根目录名")

    args = parser.parse_args(argv)
    tool = TreeTool(
        tree_json=SKILL_DIR / "tree.json",
        agents_md=REPO_ROOT / "AGENTS.md",
        repo_root=REPO_ROOT,
        root_name=REPO_ROOT.name,
        history_path=default_history_path(REPO_ROOT, SKILL_DIR),
        legacy_history_paths=(SKILL_DIR / ".history.json",),
    )
    handlers = {
        "add": _cmd_add,
        "add-batch": _cmd_add_batch,
        "rm": _cmd_rm,
        "rm-batch": _cmd_rm_batch,
        "mv": _cmd_mv,
        "mv-batch": _cmd_mv_batch,
        "get": _cmd_get,
        "query": _cmd_query,
        "mark": _cmd_mark,
        "tag-add": _cmd_tag_add,
        "tag-rm": _cmd_tag_rm,
        "view-add": _cmd_view_add,
        "view-doc": _cmd_view_doc,
        "view-rm": _cmd_view_rm,
        "view-list": _cmd_view_list,
        "undo": _cmd_undo,
        "redo": _cmd_redo,
        "history": _cmd_history,
        "check": _cmd_check,
        "render": _cmd_render,
        "root": _cmd_root,
    }
    try:
        return handlers[args.command](tool, args) or 0
    except ToolError as exc:
        print(f"错误: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
