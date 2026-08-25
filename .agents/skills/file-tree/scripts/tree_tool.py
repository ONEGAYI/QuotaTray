"""file-tree 技能：项目文件树唯一维护入口。

数据源 tree.json：顶层 {tags, tree}，tree 嵌套 = 目录嵌套（有 children 键即目录）。
条目字段固定顺序 kind / desc / detail / rel / tags / collapsed / hidden / children；
kind 由 children 判据推导（"file"/"dir"），规范化时无条件落盘供机器消费，
不参与渲染、手改会被纠正；本脚本是唯一写入口，所有写命令执行后自动按
确定性字典序（casefold + 码点决胜）规范化并重渲染产物。

渲染目标为 AGENTS.md 的两个标记块（简版树 / 标签词表）：
块内有标记则替换标记间内容；无标记则附加到文件尾部（带小节标题）；
AGENTS.md 不存在则生成最小骨架。detail 完整描述只存于 tree.json 供查询，
不渲染。渲染控制字段只影响 AGENTS.md 简版树：目录 collapsed=true 折叠
（目录行带 … 不展开 children）；条目 hidden=true 整体隐藏（含子树）；
两者默认 false（不落盘），数据、查询与 check 校验始终全量不受影响。
仓库内其他手写文件树惰性对待：以本技能 tree.json 的查询结果为准，
不主动同步维护它们。

用法：
  python tree_tool.py add <path> -d 描述 [--detail 行]... [--rel 路径]... [--tags a,b] [--dir]
                         [--collapsed|--no-collapsed] [--hidden|--no-hidden]
  python tree_tool.py rm <path>
  python tree_tool.py get <path>
  python tree_tool.py query [--kw 关键词] [--tag 标签] [--rel-of 路径] [--json]
  python tree_tool.py tag-add <名> -d 说明
  python tree_tool.py tag-rm <名>
  python tree_tool.py undo | redo | history
  python tree_tool.py check [--strict]
  python tree_tool.py render

撤销历史（默认 20 步）存放于 git 私有区 <gitdir>/file-tree/history.json：
不被 git 追踪、不入库、clone 不携带；非 git 仓库退化为技能目录 .history.json。
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

SKILL_DIR = Path(__file__).resolve().parents[1]
REPO_ROOT = SKILL_DIR.parents[2]

FIELD_ORDER = ["kind", "desc", "detail", "rel", "tags", "collapsed", "hidden", "children"]

TREE_BEGIN = "<!-- file-tree:tree:begin 由脚本渲染，禁止手改 -->"
TREE_END = "<!-- file-tree:tree:end -->"
TAGS_BEGIN = "<!-- file-tree:tags:begin 由脚本渲染，禁止手改 -->"
TAGS_END = "<!-- file-tree:tags:end -->"

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


def normalize_data(data: dict) -> dict:
    if not isinstance(data, dict):
        raise ToolError("数据顶层不是对象")
    out: dict = {}
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
    for key in sorted((k for k in data if k not in ("tags", "tree")), key=sort_key):
        out[key] = data[key]
    return out


def dumps_canonical(data: dict) -> str:
    return json.dumps(data, ensure_ascii=False, indent=2) + "\n"


def replace_block(text: str, begin: str, end: str, content: str) -> str:
    """把 begin/end 标记行之间的内容整体替换为 content（标记行独占一行）。"""
    lines = text.split("\n")
    try:
        b, e = lines.index(begin), lines.index(end)
    except ValueError as exc:
        raise ToolError(f"缺标记块 {begin} / {end}（文件被改坏？）") from exc
    if e <= b:
        raise ToolError(f"标记块顺序错误: {begin} 在 {end} 之后")
    return "\n".join(lines[: b + 1] + content.split("\n") + lines[e:])


def block_content(text: str, begin: str, end: str) -> str:
    lines = text.split("\n")
    try:
        b, e = lines.index(begin), lines.index(end)
    except ValueError as exc:
        raise ToolError(f"缺标记块 {begin} / {end}（文件被改坏？）") from exc
    return "\n".join(lines[b + 1 : e])


def is_dir(node: dict) -> bool:
    return "children" in node


def walk_entries(children: dict, prefix: list[str]):
    """按规范序深度遍历，产出 (完整路径, 节点)。"""
    for name, node in sorted(children.items(), key=lambda kv: sort_key(kv[0])):
        path = "/".join(prefix + [name])
        yield path, node
        if is_dir(node) and node["children"]:
            yield from walk_entries(node["children"], prefix + [name])


def render_tree(root_name: str, tree: dict) -> str:
    """渲染简版树：注释为 desc 单行；hidden 条目整体跳过，collapsed 目录带 … 折叠。

    列对齐：每个父目录的 children 块内按最宽 stem 对齐，'#' 固定在 width+1 列。
    """

    def render_children(children: dict, prefix: str) -> list[str]:
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
                lines.extend(render_children(node["children"], cont_prefix))
        return lines

    lines = [root_name + "/"]
    lines.extend(render_children(tree, ""))
    return "\n".join(lines)


def _find_node(tree: dict, parts: list[str]) -> dict | None:
    """沿段定位节点；不存在或路径中段是文件则返回 None。"""
    node: dict = {"children": tree}
    for part in parts:
        if not is_dir(node) or part not in node["children"]:
            return None
        node = node["children"][part]
    return node


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

    def _record_undo(self, op: str) -> None:
        """数据变更前调用：快照当前态入 undo 栈，截断 redo 分支。历史写入失败仅告警不阻断。"""
        hist = self._load_history()
        hist["undo"].append({"op": op, "data": self.load()})
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

    def undo(self) -> str:
        hist = self._load_history()
        if not hist["undo"]:
            raise ToolError("没有可撤销的操作")
        entry = hist["undo"].pop()
        hist["redo"].append({"op": entry["op"], "data": self.load()})
        hist["redo"] = self._trim(hist["redo"])
        self.write_data(entry["data"])
        self.render()
        self._save_history(hist)
        return entry["op"]

    def redo(self) -> str:
        hist = self._load_history()
        if not hist["redo"]:
            raise ToolError("没有可重做的操作")
        entry = hist["redo"].pop()
        hist["undo"].append({"op": entry["op"], "data": self.load()})
        hist["undo"] = self._trim(hist["undo"])
        self.write_data(entry["data"])
        self.render()
        self._save_history(hist)
        return entry["op"]

    # ---------- 条目操作 ----------

    def get(self, path: str) -> dict:
        parts = split_rel_path(path)
        node = _find_node(self.load()["tree"], parts)
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

    def add(self, path, desc=None, detail=None, rel=None, tags=None, is_dir_entry=False, collapsed=None, hidden=None) -> None:
        data = self.load()
        vocab = set(data.get("tags", {}))
        if tags:
            unknown = [t for t in tags if t not in vocab]
            if unknown:
                raise ToolError(f"未知标签 {unknown}，先 tag-add 登记再使用")
        if rel:
            for r in rel:
                parts_r = split_rel_path(r)
                if parts_r == split_rel_path(path):
                    raise ToolError(f"rel 不能指向自身: {path}")
                if _find_node(data["tree"], parts_r) is None:
                    raise ToolError(f"rel 目标不在树中（先 add 目标或修正路径）: {r}")
        parts, parent = self._resolve_for_write(data, path)
        name = parts[-1]
        node = parent["children"].get(name)
        if node is None:
            node = {"desc": "", "children": {}} if is_dir_entry else {"desc": ""}
            parent["children"][name] = node
        elif is_dir_entry and not is_dir(node):
            raise ToolError(f"已存在同名文件条目，不能改成目录: {path}")
        if desc is not None:
            node["desc"] = desc
        if detail is not None:
            node["detail"] = [d for d in detail if d]
        if rel is not None:
            node["rel"] = list(rel)
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
        self._record_undo(f"add {path}")
        self.write_data(data)

    def rm(self, path) -> None:
        parts = split_rel_path(path)
        data = self.load()
        parent = _find_node(data["tree"], parts[:-1])  # parts[:-1]==[] 时返回根包装
        if parent is None or not is_dir(parent) or parts[-1] not in parent["children"]:
            raise ToolError(f"条目不存在: {path}")
        del parent["children"][parts[-1]]
        # 仅沿本路径修剪变空的父目录（根不删）
        nodes: list[dict] = [{"children": data["tree"]}]
        for part in parts[:-1]:
            nodes.append(nodes[-1]["children"][part])
        for i in range(len(nodes) - 1, 0, -1):
            if not nodes[i]["children"]:
                del nodes[i - 1]["children"][parts[i - 1]]
        self._record_undo(f"rm {path}")
        self.write_data(data)

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

    # ---------- 查询 ----------

    def query(self, kw=None, tag=None, rel_of=None) -> list[tuple[str, dict]]:
        data = self.load()
        results = []
        for path, node in walk_entries(data["tree"], []):
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
        return render_tree(self.root_name, self.load()["tree"])

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

    def render(self) -> list[Path]:
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

    def _is_skill_pycache(self, path: str) -> bool:
        """技能目录内的 __pycache__（契约测试运行产物）：运行时缓存，豁免未收录告警。"""
        try:
            skill_rel = self.tree_json.parent.relative_to(self.repo_root).as_posix()
        except ValueError:
            return False
        return path.startswith(skill_rel + "/") and "__pycache__/" in path

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
            canonical = dumps_canonical(normalize_data(data))
        except ToolError as exc:
            return ([f"E: tree.json 结构非法: {exc}"], [])
        if canonical != text:
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

        git_files = self._git_files()
        if git_files is None:
            pass  # 非 git 环境静默跳过磁盘对照，由 CLI 层提示
        else:
            for missing in sorted(file_paths - git_files, key=sort_key):
                errors.append(f"E: 树中条目未被 git 跟踪且磁盘不存在: {missing}")

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
    )
    tool.render()
    print(f"已写入并重渲染: {args.path}")


def _cmd_rm(tool: TreeTool, args) -> None:
    tool.rm(args.path)
    tool.render()
    print(f"已删除并重渲染: {args.path}")


def _cmd_get(tool: TreeTool, args) -> None:
    node = tool.get(args.path)
    print(args.path)
    print(f"  类型: {'目录' if is_dir(node) else '文件'}")
    if node.get("collapsed"):
        print("  collapsed: true（简版树折叠渲染，不展开 children）")
    if node.get("hidden"):
        print("  hidden: true（简版树隐藏渲染，条目及子树不出现）")
    print(f"  desc: {node.get('desc', '')}")
    if node.get("detail"):
        print("  detail:")
        for line in node["detail"]:
            print(f"    - {line}")
    if node.get("rel"):
        print("  rel:")
        for ref in node["rel"]:
            print(f"    - {ref}")
    vocab = tool.load().get("tags", {})
    if node.get("tags"):
        rendered = ", ".join(f"{t}（{vocab.get(t, '?')}）" for t in node["tags"])
        print(f"  tags: {rendered}")


def _cmd_query(tool: TreeTool, args) -> None:
    results = tool.query(kw=args.kw, tag=args.tag, rel_of=args.rel_of)
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
        print("check 通过：规范形态、词表、rel、磁盘对照、渲染产物全部一致")
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
    p.add_argument("--dir", action="store_true", help="声明为（空）目录条目")
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

    p = sub.add_parser("rm", help="删除条目并修剪空父目录")
    p.add_argument("path")

    p = sub.add_parser("get", help="查看单个条目")
    p.add_argument("path")

    p = sub.add_parser("query", help="组合查询")
    p.add_argument("--kw", help="关键词（匹配路径/desc/detail）")
    p.add_argument("--tag", help="标签过滤")
    p.add_argument("--rel-of", dest="rel_of", help="反查：谁关联到此路径")
    p.add_argument("--json", action="store_true", help="机器可读输出")

    p = sub.add_parser("tag-add", help="登记受控标签")
    p.add_argument("name")
    p.add_argument("-d", "--desc", required=True)

    p = sub.add_parser("tag-rm", help="删除受控标签（被使用时拒绝）")
    p.add_argument("name")

    p = sub.add_parser("check", help="校验全部不变量")
    p.add_argument("--strict", action="store_true", help="告警也视为失败")

    sub.add_parser("undo", help="撤销最近一次数据变更（恢复后自动重渲染）")
    sub.add_parser("redo", help="重做最近一次撤销")
    sub.add_parser("history", help="查看可撤销/可重做的操作概要")

    sub.add_parser("render", help="重渲染 AGENTS.md 两个标记块（缺标记自动附加到尾部，无文件则生成）")

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
        "rm": _cmd_rm,
        "get": _cmd_get,
        "query": _cmd_query,
        "tag-add": _cmd_tag_add,
        "tag-rm": _cmd_tag_rm,
        "undo": _cmd_undo,
        "redo": _cmd_redo,
        "history": _cmd_history,
        "check": _cmd_check,
        "render": _cmd_render,
    }
    try:
        return handlers[args.command](tool, args) or 0
    except ToolError as exc:
        print(f"错误: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
