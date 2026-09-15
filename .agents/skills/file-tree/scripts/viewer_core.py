"""独立快照的内存读模型：一次加载、按需查询、绝对只读。

被 viewer.py（HTTP 服务）使用。设计约束：
- 快照独立（G02）：只需一个 tree.json 路径，不依赖源码、.git 或 AGENTS.md，
  不做任何磁盘校验；
- 绝对只读（G04）：构造时读取一次后不再触盘，无任何写入口；
- 内存持有（G13）：解析一次建索引，children/detail 按需从内存返回，
  不在每个请求重新解析整份文件；
- 语义一致：复用 tree_tool 的模块级纯函数（排序/路径校验/目录判据/
  结构校验/节点定位 find_node/git-ignore 有效值 effective_git_ignore），
  与核心工具的读写语义保持同源，不在此复刻第二份逻辑。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from tree_tool import (  # noqa: E402
    ToolError,
    effective_git_ignore,
    find_node,
    is_dir,
    normalize_data,
    sort_key,
    split_rel_path,
    walk_entries,
)

DEFAULT_ROOT_NAME = "tree"  # 快照无 root 键时的展示根名（viewer 无仓库上下文）
DEFAULT_PAGE_SIZE = 50  # 搜索分页默认每页条数
MAX_PAGE_SIZE = 200  # 搜索分页每页上限（防止一页拉全量，G14 按需）


def _is_positive_int(value) -> bool:
    """严格正整数判定（bool 是 int 子类，显式排除）。"""
    return isinstance(value, int) and not isinstance(value, bool) and value >= 1


class ViewerError(Exception):
    """查看器确定性错误：消息面向用户可读，status 为对应 HTTP 状态码。

    400 = 请求非法（路径不合法、不是目录、缺参数）；404 = 条目不存在；
    500 = 快照文件无法读取。
    """

    def __init__(self, message: str, status: int = 400):
        super().__init__(message)
        self.status = status


class Snapshot:
    """tree.json 的一次性内存快照：构造时解析并建索引，此后不再读盘。

    加载走 json.loads 直读（新旧排版、CRLF 均可），结构校验复用
    normalize_data——它返回重建的新对象、不写回文件，只读安全；
    排版是否规范（canonical 形态）不作为加载门槛。
    """

    def __init__(self, tree_json: Path):
        tree_json = Path(tree_json)
        try:
            text = tree_json.read_bytes().decode("utf-8")
        except OSError as exc:
            raise ViewerError(f"无法读取快照文件: {tree_json}（{exc}）", 500) from exc
        except UnicodeDecodeError as exc:
            raise ViewerError(f"快照不是 UTF-8 编码: {exc}", 400) from exc
        try:
            data = json.loads(text)
        except json.JSONDecodeError as exc:
            raise ViewerError(f"快照不是合法 JSON: {exc}", 400) from exc
        except RecursionError as exc:
            raise ViewerError("快照嵌套层级过深，无法解析（RecursionError）", 400) from exc
        try:
            normalized = normalize_data(data)
        except ToolError as exc:
            raise ViewerError(f"快照结构校验失败: {exc}", 400) from exc
        except RecursionError as exc:
            raise ViewerError("快照嵌套层级过深，无法解析（RecursionError）", 400) from exc

        self.source = str(tree_json)
        self.root_name: str = normalized.get("root") or DEFAULT_ROOT_NAME
        self.tags: dict[str, str] = dict(normalized.get("tags", {}))
        self.tree: dict = normalized["tree"]
        # G13 内存索引：路径索引（walk_entries 规范序产出，dict 保序 = 确定性遍历序）
        self._nodes: dict[str, dict] = dict(walk_entries(self.tree, []))
        # 标签索引：tag -> 命中路径列表（按 walk 序 append，序即确定性）
        self._tag_index: dict[str, list[str]] = {}
        # 反向关联索引：rel 目标 -> 引用者路径列表（等价于 query(rel_of=...) 全树扫描，
        # 精确字符串成员匹配、无路径归一化；悬空目标同样入键，只是无人能查询到它）
        self._reverse_rel: dict[str, list[str]] = {}
        for path, node in self._nodes.items():
            for tag in node.get("tags", []):
                self._tag_index.setdefault(tag, []).append(path)
            for target in node.get("rel", []):
                self._reverse_rel.setdefault(target, []).append(path)
        dirs = sum(1 for node in self._nodes.values() if is_dir(node))
        self.counts = {"dirs": dirs, "files": len(self._nodes) - dirs, "total": len(self._nodes)}

    # ------------------------------------------------------------------
    # 查询（全部从内存索引返回，无磁盘 IO）
    # ------------------------------------------------------------------

    def find(self, path: str) -> dict | None:
        """按路径定位节点；不存在或路径中段是文件返回 None。

        复用 tree_tool.find_node 公共纯函数（唯一实现）：沿段下探，目录
        判据用 children 键（不采信 kind 字段，兼容早于 kind 引入的历史数据）。
        """
        return find_node(self.tree, self._parts(path))

    def children(self, path: str) -> list[dict]:
        """目录子项摘要列表，按 sort_key 规范序；空 path 表示根级。

        hidden / collapsed 条目照常返回——界面展示状态与 JSON 标志分离
        （G11），浏览端不因标志过滤条目。
        """
        node = self.find(path)
        if node is None:
            raise ViewerError(f"条目不存在: {path or '(根)'}", 404)
        if not is_dir(node):
            raise ViewerError(f"不是目录，没有子项: {path}", 400)
        out = []
        for name, child in sorted(node["children"].items(), key=lambda kv: sort_key(kv[0])):
            out.append(
                {
                    "name": name,
                    "path": self._join(path, name),
                    "kind": "dir" if is_dir(child) else "file",
                    "desc": child.get("desc", ""),
                    "hidden": bool(child.get("hidden", False)),
                    "collapsed": bool(child.get("collapsed", False)),
                    # 三态：None = 键缺省继承祖先，False = 显式退出豁免，True = 豁免
                    "git_ignore": child.get("git-ignore"),
                    "child_count": len(child["children"]) if is_dir(child) else None,
                }
            )
        return out

    def detail(self, path: str) -> dict:
        """单条目完整详情：字段与 tree_tool query --json 同口径。

        git_ignore 拆为 explicit（键缺省 None/显式 false/显式 true）与
        effective（沿祖先链就近覆写后的有效值），三态不混为一态（G07）。
        rel 每条附 exists 标记：目标不在快照中可识别、不致命（G09 预留）。
        backrefs 为反向关联（谁引用了我），由反向索引派生，与
        tree_tool query(rel_of=path) 结果等价；引用者必然在树中，exists 恒真。
        """
        parts = self._parts(path)
        if not parts:
            raise ViewerError("根不是条目，请选择具体条目查看详情", 400)
        node = self.find(path)
        if node is None:
            raise ViewerError(f"条目不存在: {path}", 404)
        return {
            "path": "/".join(parts),
            "name": parts[-1],
            "kind": "dir" if is_dir(node) else "file",
            "desc": node.get("desc", ""),
            "detail": list(node.get("detail", [])),
            "rel": [
                {"path": target, "exists": target in self._nodes}
                for target in node.get("rel", [])
            ],
            "backrefs": [
                {"path": source, "exists": True}
                for source in self._reverse_rel.get("/".join(parts), [])
            ],
            "tags": list(node.get("tags", [])),
            "collapsed": bool(node.get("collapsed", False)),
            "hidden": bool(node.get("hidden", False)),
            "git_ignore": {
                "explicit": node.get("git-ignore"),
                "effective": self._effective_git_ignore(parts),
            },
            "child_count": len(node["children"]) if is_dir(node) else None,
        }

    def search(
        self,
        kw: str | None = None,
        tag: str | None = None,
        under: str | None = None,
        depth: int | None = None,
        page: int = 1,
        page_size: int = DEFAULT_PAGE_SIZE,
    ) -> dict:
        """组合搜索（G08）：kw/tag/under/depth 各条件 AND，与 tree_tool.query 同语义。

        - kw：casefold 子串，覆盖 path + desc + detail（多行 join 空格）；
        - tag：tags 数组精确成员匹配（走标签索引，候选集本身是 walk 序）；
        - under：段级前缀比较（锚点必须是树中目录、自身含入；src 不纳 src2）；
        - depth：相对锚点层数上限（≥1 整数，须与 under 同用；锚点自身相对深度 0）；
        - 结果顺序 = walk_entries 规范序（确定性）；分页对同一序列切片，不漏不重。
        越界页返回空 results、total 照常报告；空结果 total=0 / total_pages=0。
        """
        if not _is_positive_int(page):
            raise ViewerError(f"page 必须是正整数: {page!r}", 400)
        if not _is_positive_int(page_size) or page_size > MAX_PAGE_SIZE:
            raise ViewerError(
                f"page_size 必须是 1..{MAX_PAGE_SIZE} 的整数: {page_size!r}", 400
            )
        under_parts: list[str] | None = None
        if under is not None and under != "":
            under_parts = self._parts(under)
            anchor = self.find(under)
            if anchor is None:
                raise ViewerError(f"under 不是树中目录条目: {under}", 404)
            if not is_dir(anchor):
                raise ViewerError(f"under 不是目录，没有子树: {under}", 400)
        if depth is not None:
            if under_parts is None:
                raise ViewerError("depth 须与 under 同用（限定目录子树的相对层数）", 400)
            if not _is_positive_int(depth):
                raise ViewerError(f"depth 必须是正整数: {depth!r}", 400)
        kw_fold = kw.casefold() if kw else None

        # 候选集：有标签条件时走标签索引（walk 序），否则全量路径索引
        candidates = self._tag_index.get(tag, []) if tag else self._nodes
        hits: list[tuple[str, dict]] = []
        for path in candidates:
            node = self._nodes[path]
            parts = path.split("/")
            if under_parts is not None and parts[: len(under_parts)] != under_parts:
                continue
            if depth is not None and len(parts) - len(under_parts) > depth:
                continue
            if kw_fold is not None:
                haystack = " ".join(
                    [path, node.get("desc", ""), " ".join(node.get("detail", []))]
                ).casefold()
                if kw_fold not in haystack:
                    continue
            hits.append((path, node))

        total = len(hits)
        total_pages = (total + page_size - 1) // page_size
        start = (page - 1) * page_size
        return {
            "query": {
                "kw": kw or None,
                "tag": tag or None,
                "under": under or None,
                "depth": depth if depth is not None else None,
            },
            "total": total,
            "total_pages": total_pages,
            "page": page,
            "page_size": page_size,
            "results": [
                self._hit_summary(path, node) for path, node in hits[start : start + page_size]
            ],
        }

    def root_info(self) -> dict:
        """页面初始化信息：根名、标签词表、条目计数、快照来源路径。"""
        return {
            "root": self.root_name,
            "tags": self.tags,
            "counts": self.counts,
            "source": self.source,
        }

    # ------------------------------------------------------------------
    # 内部
    # ------------------------------------------------------------------

    def _parts(self, path: str | None) -> list[str]:
        """路径拆段（含合法性校验）；空串/None 视为根（返回空段列表）。"""
        if path is None or path == "":
            return []
        try:
            return split_rel_path(path)
        except ToolError as exc:
            raise ViewerError(str(exc), 400) from exc

    @staticmethod
    def _join(prefix: str, name: str) -> str:
        return f"{prefix}/{name}" if prefix else name

    @staticmethod
    def _hit_summary(path: str, node: dict) -> dict:
        """搜索结果条目：字段集与 tree_tool query --json 同口径（snake_case 命名
        沿用本查看器 API 习惯，git_ignore 三态 null/false/true 不混为一态）。"""
        return {
            "path": path,
            "kind": "dir" if is_dir(node) else "file",
            "desc": node.get("desc", ""),
            "detail": list(node.get("detail", [])),
            "rel": list(node.get("rel", [])),
            "tags": list(node.get("tags", [])),
            "collapsed": bool(node.get("collapsed", False)),
            "hidden": bool(node.get("hidden", False)),
            "git_ignore": node.get("git-ignore"),
        }

    def _effective_git_ignore(self, parts: list[str]) -> bool:
        """git-ignore 有效值：复用 tree_tool.effective_git_ignore（唯一实现）。

        沿祖先链（含自身）最近一次显式设置生效；全链缺省则不豁免（False）。
        parts 非空（detail 对根已提前拒绝），join 回路径串走公共函数的
        路径拆段与就近覆写逻辑。
        """
        return effective_git_ignore(self.tree, "/".join(parts))
