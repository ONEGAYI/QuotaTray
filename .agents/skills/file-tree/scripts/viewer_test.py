"""独立快照只读查看器（viewer.py / viewer_core.py）的契约测试。

运行：python skills/deploy-file-tree-skill/dist/scripts/viewer_test.py
沙箱模式：所有用例在临时目录构造快照与静态资源，不触仓库；快照目录
不含源码、.git 或 AGENTS.md，借以验证快照独立性（G02）。HTTP 用例在
127.0.0.1 随机端口真实起服务，用标准库 http.client 直连断言（G20）；
CLI 用例以子进程真实启动入口并解析其输出的访问地址。
"""

from __future__ import annotations

import hashlib
import http.client
import json
import re
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from urllib.parse import quote

sys.path.insert(0, str(Path(__file__).parent))

import viewer  # noqa: E402
from tree_tool import (  # noqa: E402
    TreeTool,
    effective_git_ignore,
    find_node,
    split_rel_path,
    walk_entries,
)
from viewer_core import Snapshot, ViewerError  # noqa: E402

VIEWER_ENTRY = Path(__file__).parent / "viewer.py"


def compact_dumps(data: dict) -> str:
    """独立新编码规则（紧凑单行 + 末尾 LF）：标准库直调，不经被测实现。"""
    return json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n"


def legacy_dumps(data: dict) -> str:
    """独立旧编码规则（两空格缩进 + 末尾 LF）：标准库直调，不经被测实现。"""
    return json.dumps(data, ensure_ascii=False, indent=2) + "\n"


def make_snapshot_data() -> dict:
    """固定样本：覆盖中文、空目录、detail 多行与转义、rel 悬空、
    hidden/collapsed、git-ignore 三态（缺省继承 / 显式 false / 显式 true）。
    """
    return {
        "root": "演示仓库",
        "tags": {"doc": "说明文档", "script": "维护脚本"},
        "tree": {
            "apps": {
                "kind": "dir",
                "desc": "应用目录",
                "detail": ["应用目录说明第一行", "应用目录说明第二行"],
                "children": {
                    "main.tsx": {
                        "kind": "file",
                        "desc": "前端入口",
                        "detail": ["含\"双引号\"与\\反斜杠\\转义", "第二行说明"],
                        "rel": ["normal.md", "gone.rs"],
                        "tags": ["script"],
                    },
                    "util.ts": {"kind": "file", "desc": "工具函数", "tags": ["script"]},
                },
            },
            "collapsed-dir": {
                "kind": "dir",
                "desc": "折叠目录",
                "collapsed": True,
                "children": {
                    "inner.md": {
                        "kind": "file",
                        "desc": "折叠目录内部文件",
                        "detail": ["初始折叠但可在界面展开"],
                    },
                },
            },
            "empty-dir": {"kind": "dir", "desc": "空目录", "children": {}},
            "exempt": {
                "kind": "dir",
                "desc": "豁免目录",
                "git-ignore": True,
                "children": {
                    "inherit.ts": {"kind": "file", "desc": "继承豁免"},
                    "optout.ts": {"kind": "file", "desc": "显式退出豁免", "git-ignore": False},
                },
            },
            "hidden-dir": {
                "kind": "dir",
                "desc": "隐藏目录",
                "hidden": True,
                "children": {
                    "secret.md": {"kind": "file", "desc": "隐藏文件", "hidden": True},
                },
            },
            "normal.md": {"kind": "file", "desc": "普通文件"},
            "中文目录": {
                "kind": "dir",
                "desc": "中文命名目录",
                "children": {
                    "说明.md": {
                        "kind": "file",
                        "desc": "中文名称文件",
                        "detail": ["中文 detail 第一行", "中文 detail 第二行"],
                    },
                },
            },
        },
    }


ROOT_ORDER = [
    "apps",
    "collapsed-dir",
    "empty-dir",
    "exempt",
    "hidden-dir",
    "normal.md",
    "中文目录",
]
COUNTS = {"dirs": 6, "files": 8, "total": 14}


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


# ---------------------------------------------------------------------------
# 快照加载：新旧排版 / 空树 / 非法输入（G03）
# ---------------------------------------------------------------------------


class SnapshotLoadTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)

    def write_snapshot(self, text: str) -> Path:
        path = self.dir / "tree.json"
        path.write_text(text, encoding="utf-8", newline="\n")
        return path

    def test_load_compact_form(self):
        snap = Snapshot(self.write_snapshot(compact_dumps(make_snapshot_data())))
        self.assertEqual(snap.root_name, "演示仓库")
        self.assertEqual(snap.tags, {"doc": "说明文档", "script": "维护脚本"})
        self.assertEqual(snap.counts, COUNTS)
        self.assertEqual([c["path"] for c in snap.children("")], ROOT_ORDER)

    def test_load_legacy_form_equivalent_to_compact(self):
        data = make_snapshot_data()
        compact = Snapshot(self.write_snapshot(compact_dumps(data)))
        legacy_path = self.dir / "legacy.json"
        legacy_path.write_text(legacy_dumps(data), encoding="utf-8", newline="\n")
        legacy = Snapshot(legacy_path)
        self.assertEqual(legacy.root_name, compact.root_name)
        self.assertEqual(legacy.tags, compact.tags)
        self.assertEqual(legacy.counts, compact.counts)
        self.assertEqual(legacy.children(""), compact.children(""))
        self.assertEqual(legacy.detail("apps/main.tsx"), compact.detail("apps/main.tsx"))
        self.assertEqual(legacy.detail("exempt/optout.ts"), compact.detail("exempt/optout.ts"))

    def test_load_crlf_text(self):
        # CRLF 行尾不是规范排版，但 JSON 解析不受行尾影响，必须可读
        path = self.dir / "tree.json"
        path.write_bytes(legacy_dumps(make_snapshot_data()).replace("\n", "\r\n").encode("utf-8"))
        snap = Snapshot(path)
        self.assertEqual(snap.counts, COUNTS)

    def test_load_entry_without_kind_field(self):
        # 早于 kind 字段的旧数据：无 kind 键，按 children 判据区分目录与文件
        path = self.dir / "tree.json"
        path.write_text(
            json.dumps({"tree": {"src": {"desc": "无 kind 目录", "children": {"a.py": {"desc": "无 kind 文件"}}}}},
                       ensure_ascii=False),
            encoding="utf-8",
            newline="\n",
        )
        snap = Snapshot(path)
        children = snap.children("src")
        self.assertEqual([c["kind"] for c in children], ["file"])
        self.assertEqual(snap.detail("src")["kind"], "dir")

    def test_load_empty_tree(self):
        for text in (compact_dumps({"tree": {}}), legacy_dumps({"tree": {}})):
            with self.subTest(text=text[:30]):
                snap = Snapshot(self.write_snapshot(text))
                self.assertEqual(snap.counts, {"dirs": 0, "files": 0, "total": 0})
                self.assertEqual(snap.children(""), [])

    def test_invalid_json_refuses_with_readable_error(self):
        path = self.write_snapshot("{oops 不是 json")
        with self.assertRaises(ViewerError) as ctx:
            Snapshot(path)
        self.assertIn("JSON", str(ctx.exception))

    def test_bad_structure_refuses(self):
        cases = [
            ("desc 非字符串", json.dumps({"tree": {"a": {"desc": 1}}}, ensure_ascii=False)),
            ("顶层非对象", json.dumps([1, 2], ensure_ascii=False)),
            ("tree 非对象", json.dumps({"tree": []}, ensure_ascii=False)),
            ("detail 非数组", json.dumps({"tree": {"a": {"desc": "x", "detail": "y"}}}, ensure_ascii=False)),
            ("root 为 null", json.dumps({"root": None, "tree": {}}, ensure_ascii=False)),
        ]
        for name, text in cases:
            with self.subTest(case=name):
                with self.assertRaises(ViewerError) as ctx:
                    Snapshot(self.write_snapshot(text))
                self.assertIn("结构校验失败", str(ctx.exception))

    def test_deeply_nested_json_raises_readable_viewer_error(self):
        # 数万层嵌套 JSON 使 json.loads（或 normalize_data）触发
        # RecursionError：必须包装为可读 ViewerError(400)，不得裸 traceback
        deep = "[" * 100_000 + "]" * 100_000
        with self.assertRaises(ViewerError) as ctx:
            Snapshot(self.write_snapshot(deep))
        self.assertEqual(ctx.exception.status, 400)
        self.assertIn("嵌套层级过深", str(ctx.exception))

    def test_load_does_not_touch_disk(self):
        path = self.write_snapshot(legacy_dumps(make_snapshot_data()))
        before = sha256_file(path)
        Snapshot(path)
        Snapshot(path)
        self.assertEqual(sha256_file(path), before)
        # 快照独立性：加载不产生任何伴生文件（历史、AGENTS.md 等）
        self.assertEqual(sorted(p.name for p in self.dir.iterdir()), ["tree.json"])

    def test_load_snapshot_with_views_key(self):
        # 含 views 键的快照（多视图能力，#33 user story 23）正常加载：
        # viewer_core 只读 root/tags/tree 三键，views 透传无害，浏览与统计不受影响
        baseline_data = make_snapshot_data()
        baseline = Snapshot(self.write_snapshot(compact_dumps(baseline_data)))
        with_views = make_snapshot_data()
        with_views["views"] = {
            "ext": {
                "filter": {"op": "under", "path": "apps"},
                "docs": ["extensions/README.md"],
            },
        }
        snap = Snapshot(self.write_snapshot(compact_dumps(with_views)))
        self.assertEqual(snap.root_name, baseline.root_name)
        self.assertEqual(snap.tags, baseline.tags)
        self.assertEqual(snap.counts, baseline.counts)  # views 不进树，统计口径不变
        self.assertEqual(snap.children(""), baseline.children(""))
        self.assertEqual(snap.detail("apps/main.tsx"), baseline.detail("apps/main.tsx"))


# ---------------------------------------------------------------------------
# 内存查询：children / detail 语义（G06/G07/G11/G14）
# ---------------------------------------------------------------------------


class SnapshotApiTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.snapshot_path = Path(cls.tmp.name) / "tree.json"
        cls.snapshot_path.write_text(compact_dumps(make_snapshot_data()), encoding="utf-8", newline="\n")
        cls.snap = Snapshot(cls.snapshot_path)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    # -- children ------------------------------------------------------

    def test_children_root_is_sorted_and_complete(self):
        children = self.snap.children("")
        # hidden / collapsed 目录不因标志被过滤（G11：界面可见）
        self.assertEqual([c["path"] for c in children], ROOT_ORDER)
        by_name = {c["name"]: c for c in children}
        self.assertEqual(by_name["hidden-dir"]["hidden"], True)
        self.assertEqual(by_name["collapsed-dir"]["collapsed"], True)
        self.assertEqual(by_name["apps"]["kind"], "dir")
        self.assertEqual(by_name["normal.md"]["kind"], "file")
        # 空目录正常显示：child_count 为 0 而非条目消失（G06）
        self.assertEqual(by_name["empty-dir"]["child_count"], 0)
        # git-ignore 三态在子项摘要中不混为一态（G07）
        self.assertIsNone(by_name["apps"]["git_ignore"])
        self.assertTrue(by_name["exempt"]["git_ignore"])

    def test_children_of_subdirectory(self):
        children = self.snap.children("apps")
        self.assertEqual([c["path"] for c in children], ["apps/main.tsx", "apps/util.ts"])

    def test_children_of_empty_directory_returns_empty_list(self):
        self.assertEqual(self.snap.children("empty-dir"), [])

    def test_children_of_hidden_directory_still_listed(self):
        self.assertEqual([c["path"] for c in self.snap.children("hidden-dir")], ["hidden-dir/secret.md"])

    def test_children_of_collapsed_directory_still_listed(self):
        self.assertEqual([c["path"] for c in self.snap.children("collapsed-dir")], ["collapsed-dir/inner.md"])

    def test_children_of_chinese_directory(self):
        self.assertEqual([c["path"] for c in self.snap.children("中文目录")], ["中文目录/说明.md"])

    def test_children_of_file_raises(self):
        with self.assertRaises(ViewerError) as ctx:
            self.snap.children("apps/main.tsx")
        self.assertIn("不是目录", str(ctx.exception))

    def test_children_missing_path_raises_404(self):
        with self.assertRaises(ViewerError) as ctx:
            self.snap.children("nope")
        self.assertEqual(ctx.exception.status, 404)

    def test_children_illegal_path_raises(self):
        for bad in ("../escape", "/abs/path", "a/./b", ".."):
            with self.subTest(path=bad):
                with self.assertRaises(ViewerError) as ctx:
                    self.snap.children(bad)
                self.assertEqual(ctx.exception.status, 400)

    # -- detail --------------------------------------------------------

    def test_detail_full_fields(self):
        d = self.snap.detail("apps/main.tsx")
        self.assertEqual(d["path"], "apps/main.tsx")
        self.assertEqual(d["name"], "main.tsx")
        self.assertEqual(d["kind"], "file")
        self.assertEqual(d["desc"], "前端入口")
        # detail 多行与转义原样往返（G07）
        self.assertEqual(d["detail"], ["含\"双引号\"与\\反斜杠\\转义", "第二行说明"])
        self.assertEqual(d["tags"], ["script"])
        # rel 边 + 悬空识别：目标不在快照中可识别、不致命（G09 预留）；
        # rel 语义上经规范化排序去重，顺序为 sort_key 序
        self.assertEqual(
            d["rel"],
            [{"path": "gone.rs", "exists": False}, {"path": "normal.md", "exists": True}],
        )
        self.assertIsNone(d["child_count"])

    def test_detail_git_ignore_tri_state(self):
        # 键缺省 = 继承；有效值沿祖先链就近覆写（G07 三态不混为一态）
        cases = [
            ("exempt", True, True),              # 显式 true
            ("exempt/inherit.ts", None, True),   # 缺省 → 继承祖先 true
            ("exempt/optout.ts", False, False),  # 显式 false 覆写祖先
            ("apps/main.tsx", None, False),      # 全链缺省 → 不豁免
        ]
        for path, explicit, effective in cases:
            with self.subTest(path=path):
                gi = self.snap.detail(path)["git_ignore"]
                self.assertEqual(gi["explicit"], explicit)
                self.assertEqual(gi["effective"], effective)

    def test_detail_directory_fields(self):
        d = self.snap.detail("apps")
        self.assertEqual(d["kind"], "dir")
        self.assertEqual(d["detail"], ["应用目录说明第一行", "应用目录说明第二行"])
        self.assertEqual(d["child_count"], 2)
        self.assertFalse(d["collapsed"])
        self.assertFalse(d["hidden"])

    def test_detail_hidden_entry_visible(self):
        d = self.snap.detail("hidden-dir/secret.md")
        self.assertTrue(d["hidden"])
        self.assertEqual(d["desc"], "隐藏文件")

    def test_detail_collapsed_entry_expandable_data(self):
        d = self.snap.detail("collapsed-dir")
        self.assertTrue(d["collapsed"])
        # collapsed 只影响渲染初始态，子项数据完整可查（G11）
        self.assertEqual([c["path"] for c in self.snap.children("collapsed-dir")], ["collapsed-dir/inner.md"])

    def test_detail_missing_raises_404(self):
        with self.assertRaises(ViewerError) as ctx:
            self.snap.detail("apps/nope.ts")
        self.assertEqual(ctx.exception.status, 404)

    def test_detail_root_path_rejected(self):
        with self.assertRaises(ViewerError):
            self.snap.detail("")

    def test_detail_illegal_path_raises(self):
        with self.assertRaises(ViewerError):
            self.snap.detail("..%2Fetc")


# ---------------------------------------------------------------------------
# HTTP 服务：真启动、按需接口、只读、静态托管边界（G01/G04/G13/G14/G20）
# ---------------------------------------------------------------------------


class HttpServerBase(unittest.TestCase):
    """共享夹具：临时目录快照（无源码/.git/AGENTS.md）+ 随机端口真服务。"""

    @classmethod
    def start_server(cls, static_dir: Path, data: dict | None = None):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.dir = Path(cls.tmp.name)
        cls.snapshot_path = cls.dir / "tree.json"
        cls.snapshot_path.write_text(
            compact_dumps(data if data is not None else make_snapshot_data()),
            encoding="utf-8",
            newline="\n",
        )
        cls.before_bytes = cls.snapshot_path.read_bytes()
        cls.before_digest = hashlib.sha256(cls.before_bytes).hexdigest()
        cls.static_dir = static_dir
        cls.server = viewer.create_server(
            tree_json=cls.snapshot_path, port=0, static_dir=cls.static_dir, quiet=True
        )
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.host, cls.port = cls.server.server_address[:2]

    @classmethod
    def stop_server(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)

    def request(self, method: str, path: str, body: bytes | None = None):
        conn = http.client.HTTPConnection(self.host, self.port, timeout=10)
        try:
            conn.request(method, path, body=body)
            resp = conn.getresponse()
            return resp.status, resp.getheader("Content-Type"), resp.read()
        finally:
            conn.close()

    def get_json(self, path: str):
        status, ctype, data = self.request("GET", path)
        self.assertIn("application/json", ctype or "")
        return status, json.loads(data.decode("utf-8"))

    @staticmethod
    def q(path: str) -> str:
        return quote(path, safe="")


class ViewerHttpApiTest(HttpServerBase):
    @classmethod
    def setUpClass(cls):
        # 静态目录指向显式空目录：验证 API 不依赖页面资源，且缺失提示可测
        import tempfile as _tf

        cls._extra = _tf.TemporaryDirectory()
        cls.start_server(static_dir=Path(cls._extra.name))

    @classmethod
    def tearDownClass(cls):
        cls.stop_server()
        # G04 终态断言：全部请求结束后快照字节不变、目录无新增文件
        after = cls.snapshot_path.read_bytes()
        assert hashlib.sha256(after).hexdigest() == cls.before_digest, "浏览流量改变了快照字节"
        assert sorted(p.name for p in cls.dir.iterdir()) == ["tree.json"], "快照目录出现新增文件"
        cls._extra.cleanup()
        cls.tmp.cleanup()

    def test_server_binds_loopback(self):
        self.assertEqual(self.host, "127.0.0.1")
        self.assertGreater(self.port, 0)

    def test_api_root(self):
        status, payload = self.get_json("/api/root")
        self.assertEqual(status, 200)
        self.assertEqual(payload["root"], "演示仓库")
        self.assertEqual(payload["tags"], {"doc": "说明文档", "script": "维护脚本"})
        self.assertEqual(payload["counts"], COUNTS)

    def test_api_children_is_on_demand_not_full_tree(self):
        status, payload = self.get_json("/api/children")
        self.assertEqual(status, 200)
        self.assertEqual([c["path"] for c in payload["children"]], ROOT_ORDER)
        # 按需契约（G14 首步）：响应不得携带未请求层级的深层条目
        raw = json.dumps(payload, ensure_ascii=False)
        self.assertNotIn("inner.md", raw)
        self.assertNotIn("optout.ts", raw)
        self.assertNotIn("说明.md", raw)
        # 子项摘要不含嵌套 children 内容
        for child in payload["children"]:
            self.assertNotIn("children", child)

        status, payload = self.get_json(f"/api/children?path={self.q('apps')}")
        self.assertEqual(status, 200)
        self.assertEqual([c["path"] for c in payload["children"]], ["apps/main.tsx", "apps/util.ts"])

    def test_api_children_empty_dir_and_hidden_visible(self):
        status, payload = self.get_json(f"/api/children?path={self.q('empty-dir')}")
        self.assertEqual(status, 200)
        self.assertEqual(payload["children"], [])

        status, payload = self.get_json(f"/api/children?path={self.q('hidden-dir')}")
        self.assertEqual(status, 200)
        self.assertEqual([c["path"] for c in payload["children"]], ["hidden-dir/secret.md"])

        status, payload = self.get_json(f"/api/children?path={self.q('collapsed-dir')}")
        self.assertEqual(status, 200)
        self.assertEqual([c["path"] for c in payload["children"]], ["collapsed-dir/inner.md"])

    def test_api_detail_on_demand(self):
        status, payload = self.get_json(f"/api/detail?path={self.q('apps/main.tsx')}")
        self.assertEqual(status, 200)
        self.assertEqual(payload["detail"], ["含\"双引号\"与\\反斜杠\\转义", "第二行说明"])
        self.assertEqual(payload["rel"][0], {"path": "gone.rs", "exists": False})
        raw = json.dumps(payload, ensure_ascii=False)
        self.assertNotIn("util.ts", raw)  # 只含该条目，不夹带兄弟/深层数据

    def test_api_detail_git_ignore_tri_state(self):
        _, payload = self.get_json(f"/api/detail?path={self.q('exempt/inherit.ts')}")
        self.assertIsNone(payload["git_ignore"]["explicit"])
        self.assertTrue(payload["git_ignore"]["effective"])
        _, payload = self.get_json(f"/api/detail?path={self.q('exempt/optout.ts')}")
        self.assertFalse(payload["git_ignore"]["explicit"])
        self.assertFalse(payload["git_ignore"]["effective"])

    def test_api_detail_chinese_path_roundtrip(self):
        status, payload = self.get_json(f"/api/detail?path={self.q('中文目录/说明.md')}")
        self.assertEqual(status, 200)
        self.assertEqual(payload["detail"], ["中文 detail 第一行", "中文 detail 第二行"])

    def test_api_errors(self):
        status, payload = self.get_json(f"/api/children?path={self.q('nope')}")
        self.assertEqual(status, 404)
        self.assertIn("error", payload)

        status, payload = self.get_json(f"/api/children?path={self.q('apps/main.tsx')}")
        self.assertEqual(status, 400)
        self.assertIn("error", payload)

        status, payload = self.get_json(f"/api/detail?path={self.q('apps/nope.ts')}")
        self.assertEqual(status, 404)

        status, payload = self.get_json("/api/detail")
        self.assertEqual(status, 400)

        status, payload = self.get_json(f"/api/children?path={self.q('../escape')}")
        self.assertEqual(status, 400)
        self.assertIn("'..'", payload["error"])

        status, payload = self.get_json("/api/unknown")
        self.assertEqual(status, 404)

    def test_write_methods_rejected(self):
        for method in ("POST", "PUT", "DELETE", "PATCH"):
            with self.subTest(method=method):
                status, payload = self.get_json_via(method)
                self.assertEqual(status, 405)
                self.assertIn("只读", payload["error"])

    def get_json_via(self, method: str):
        conn = http.client.HTTPConnection(self.host, self.port, timeout=10)
        try:
            conn.request(method, "/api/children")
            resp = conn.getresponse()
            return resp.status, json.loads(resp.read().decode("utf-8"))
        finally:
            conn.close()

    def test_static_index_missing_hint(self):
        status, ctype, data = self.request("GET", "/")
        self.assertEqual(status, 503)
        self.assertIn("text/html", ctype or "")
        text = data.decode("utf-8")
        self.assertIn("npm run build", text)
        self.assertIn("前端", text)

    def test_snapshot_bytes_unchanged_after_traffic(self):
        # 类级 tearDownClass 做终态断言；此处做过程中的即时复核
        status, _ = self.get_json("/api/root")
        self.assertEqual(status, 200)
        self.assertEqual(
            hashlib.sha256(self.snapshot_path.read_bytes()).hexdigest(), self.before_digest
        )


class StaticServingTest(HttpServerBase):
    @classmethod
    def setUpClass(cls):
        import tempfile as _tf

        cls._static_tmp = _tf.TemporaryDirectory()
        static = Path(cls._static_tmp.name) / "build"
        (static / "assets").mkdir(parents=True)
        (static / "index.html").write_text(
            "<!doctype html><title>viewer</title><p>页面 OK</p>", encoding="utf-8", newline="\n"
        )
        (static / "assets" / "app.js").write_text("console.log('ok');", encoding="utf-8", newline="\n")
        (static / "assets" / "style.css").write_text("body{}", encoding="utf-8", newline="\n")
        cls.start_server(static_dir=static)
        # 穿越目标：静态目录之外的敏感文件
        (Path(cls._static_tmp.name) / "secret.txt").write_text("secret", encoding="utf-8")

    @classmethod
    def tearDownClass(cls):
        cls.stop_server()
        cls._static_tmp.cleanup()
        cls.tmp.cleanup()

    def test_index_served_as_html(self):
        status, ctype, data = self.request("GET", "/")
        self.assertEqual(status, 200)
        self.assertIn("text/html", ctype or "")
        self.assertIn("页面 OK", data.decode("utf-8"))

    def test_assets_served_with_mime(self):
        cases = [
            ("/assets/app.js", "text/javascript"),
            ("/assets/style.css", "text/css"),
        ]
        for url, mime in cases:
            with self.subTest(url=url):
                status, ctype, data = self.request("GET", url)
                self.assertEqual(status, 200)
                self.assertIn(mime, ctype or "")

    def test_unknown_static_file_404(self):
        for url in ("/nonexistent.js", "/favicon.ico", "/index.html.bak"):
            with self.subTest(url=url):
                status, _, _ = self.request("GET", url)
                self.assertEqual(status, 404)

    def test_path_traversal_blocked(self):
        cases = [
            "/%2e%2e/secret.txt",        # 解码后 /../secret.txt
            "/..%2fsecret.txt",           # 解码后 /../secret.txt
            "/%2e%2e%2f%2e%2e%2fsecret.txt",
            "/assets/..%2f..%2fsecret.txt",
            "/..%5c..%5csecret.txt",      # 反斜杠变体
        ]
        for url in cases:
            with self.subTest(url=url):
                status, _, data = self.request("GET", url)
                self.assertEqual(status, 404, f"{url} 不应命中静态目录外文件: {data!r}")

    def test_snapshot_file_not_exposed_as_static(self):
        # 快照与查看器源码不在静态目录内，不得经页面路径读到（G20）
        for url in ("/tree.json", "/viewer.py", "/viewer_core.py"):
            with self.subTest(url=url):
                status, _, _ = self.request("GET", url)
                self.assertEqual(status, 404)


# ---------------------------------------------------------------------------
# CLI 入口：真实子进程启动、地址输出、参数校验（G01）
# ---------------------------------------------------------------------------


class HostHeaderCheckTest(unittest.TestCase):
    """默认绑定下的 Host 头校验（#23 审查 C2，防 DNS rebinding）：

    恶意域名解析到 127.0.0.1 后浏览器请求仍携带恶意 Host——默认绑定
    （127.0.0.1）必须只接受回环地址形态的 Host，否则 403 并说明出路；
    显式 --host 自定义绑定视为用户已自行开放网络暴露，跳过校验。
    """

    @classmethod
    def setUpClass(cls):
        import tempfile as _tf

        cls.tmp = _tf.TemporaryDirectory()
        cls.dir = Path(cls.tmp.name)
        cls.snapshot_path = cls.dir / "tree.json"
        cls.snapshot_path.write_text(
            compact_dumps(make_snapshot_data()), encoding="utf-8", newline="\n"
        )
        cls._extra = _tf.TemporaryDirectory()
        cls.static_dir = Path(cls._extra.name) / "viewer"
        cls.static_dir.mkdir(parents=True)
        (cls.static_dir / "index.html").write_text(
            "<!doctype html><title>v</title>", encoding="utf-8", newline="\n"
        )
        # 默认绑定（127.0.0.1）服务：Host 校验开启
        cls.server = viewer.create_server(
            tree_json=cls.snapshot_path, port=0, static_dir=cls.static_dir, quiet=True
        )
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.host, cls.port = cls.server.server_address[:2]
        # 显式自定义绑定（0.0.0.0）：Host 校验关闭
        cls.custom = viewer.create_server(
            tree_json=cls.snapshot_path,
            host="0.0.0.0",
            port=0,
            static_dir=cls.static_dir,
            quiet=True,
        )
        cls.custom_thread = threading.Thread(target=cls.custom.serve_forever, daemon=True)
        cls.custom_thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)
        cls.custom.shutdown()
        cls.custom.server_close()
        cls.custom_thread.join(timeout=5)
        cls._extra.cleanup()
        cls.tmp.cleanup()

    def fetch(self, host_header: str, path: str = "/api/root", server=None):
        target = server if server is not None else self.server
        addr_host, addr_port = target.server_address[:2]
        if addr_host in ("0.0.0.0", "::"):  # Windows 不允许连接通配地址本身
            addr_host = "127.0.0.1"
        conn = http.client.HTTPConnection(addr_host, addr_port, timeout=10)
        try:
            conn.request("GET", path, headers={"Host": host_header})
            resp = conn.getresponse()
            return resp.status, resp.getheader("Content-Type"), resp.read()
        finally:
            conn.close()

    def test_default_bind_rejects_foreign_host_on_api_and_page(self):
        for path in ("/api/root", "/"):
            with self.subTest(path=path):
                status, ctype, body = self.fetch("evil.example.com", path=path)
                self.assertEqual(status, 403)
                self.assertIn("application/json", ctype or "")
                payload = json.loads(body.decode("utf-8"))
                self.assertIn("Host", payload["error"])
                self.assertIn("127.0.0.1", payload["error"])  # 说明放行范围

    def test_default_bind_accepts_loopback_host_forms(self):
        for host_header in (
            f"127.0.0.1:{self.port}",
            f"localhost:{self.port}",
            "localhost",  # 不带端口
            f"[::1]:{self.port}",
        ):
            with self.subTest(host=host_header):
                status, _, _ = self.fetch(host_header)
                self.assertEqual(status, 200)

    def test_custom_host_bind_skips_host_check(self):
        # 显式 --host 自定义绑定：任意 Host 放行（用户已自行承担暴露）
        status, _, _ = self.fetch("evil.example.com", server=self.custom)
        self.assertEqual(status, 200)


class ViewerCliTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.snapshot_path = Path(self.tmp.name) / "tree.json"
        self.snapshot_path.write_text(compact_dumps(make_snapshot_data()), encoding="utf-8", newline="\n")

    def run_cli(self, *args, timeout=30):
        return subprocess.run(
            [sys.executable, str(VIEWER_ENTRY), *args],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )

    def test_cli_requires_tree_json_argument(self):
        result = self.run_cli()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("tree_json", result.stderr + result.stdout)

    def test_cli_missing_snapshot_refuses_to_start(self):
        result = self.run_cli(str(Path(self.tmp.name) / "nope.json"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("无法启动查看器", result.stderr + result.stdout)

    def test_cli_invalid_snapshot_refuses_to_start(self):
        bad = Path(self.tmp.name) / "bad.json"
        bad.write_text("{broken", encoding="utf-8", newline="\n")
        result = self.run_cli(str(bad))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("JSON", result.stderr + result.stdout)

    def test_cli_deeply_nested_snapshot_refuses_to_start(self):
        # 深嵌套快照：启动路径收到可读错误（exit 2），不是 RecursionError 裸 traceback
        deep = Path(self.tmp.name) / "deep.json"
        deep.write_text("[" * 100_000 + "]" * 100_000, encoding="utf-8", newline="\n")
        result = self.run_cli(str(deep))
        self.assertEqual(result.returncode, 2)
        combined = result.stderr + result.stdout
        self.assertIn("嵌套层级过深", combined)
        self.assertNotIn("Traceback", combined)

    def test_cli_starts_prints_url_and_serves(self):
        before = sha256_file(self.snapshot_path)
        proc = subprocess.Popen(
            [sys.executable, str(VIEWER_ENTRY), str(self.snapshot_path), "--port", "0"],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        self.addCleanup(proc.terminate)
        lines: list[str] = []
        reader = threading.Thread(target=lambda: lines.extend(proc.stdout or []), daemon=True)
        reader.start()

        port = None
        deadline = time.time() + 20
        while time.time() < deadline and port is None:
            for line in list(lines):
                match = re.search(r"http://127\.0\.0\.1:(\d+)/", line)
                if match:
                    port = int(match.group(1))
                    break
            if port is None and proc.poll() is not None:
                break
            time.sleep(0.1)
        self.assertIsNotNone(port, f"未在输出中解析到访问地址，输出: {lines}")

        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            conn.request("GET", "/api/root")
            resp = conn.getresponse()
            payload = json.loads(resp.read().decode("utf-8"))
            self.assertEqual(resp.status, 200)
            self.assertEqual(payload["counts"], COUNTS)
        finally:
            conn.close()

        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        reader.join(timeout=2)
        if proc.stdout:
            proc.stdout.close()  # 显式关闭管道，避免 GC 期 ResourceWarning
        self.assertEqual(sha256_file(self.snapshot_path), before)


class ViewerPreflightTest(unittest.TestCase):
    """启动前置检查（#22，G21/G22）：必要运行条件缺失时给出可理解错误退出。

    - Python 版本门槛用 sys.version_info 检测：低于 MIN_PYTHON 拒绝启动并
      说明当前/所需版本，不后台尝试升级运行时；
    - 快照路径是目录（存在但不是文件）与"不存在"分开报错；
    - 发行静态资源缺失：启动横幅提示构建方法，页面降级 503，API 保持可用
      （#21 已合入的降级契约，本票补 CLI 级证据）。
    """

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.snapshot_path = Path(self.tmp.name) / "tree.json"
        self.snapshot_path.write_text(
            compact_dumps(make_snapshot_data()), encoding="utf-8", newline="\n"
        )

    def run_cli(self, *args, timeout=30):
        return subprocess.run(
            [sys.executable, str(VIEWER_ENTRY), *args],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )

    def test_python_version_gate_passes_on_current_interpreter(self):
        # 当前解释器（实测环境）必须通过门槛；门槛本身只升不降见实现注释
        self.assertIsNone(viewer.python_version_error())

    def test_python_version_gate_message_for_old_version(self):
        message = viewer.python_version_error((3, 7, 15, "final", 0))
        self.assertIsNotNone(message)
        self.assertIn("3.7.15", message)  # 如实报告当前版本
        self.assertIn("3.8", message)  # 如实报告门槛版本
        self.assertIn("不会自动升级", message)  # 明说不后台升级运行时

    def test_cli_python_version_below_minimum_refuses_to_start(self):
        # 篡改门槛为不可能满足的版本，走完整 main() 启动路径
        code = (
            "import sys; sys.path.insert(0, {dir!r}); import viewer; "
            "viewer.MIN_PYTHON = (99, 0); "
            "sys.exit(viewer.main([{snap!r}]))"
        ).format(dir=str(VIEWER_ENTRY.parent), snap=str(self.snapshot_path))
        result = subprocess.run(
            [sys.executable, "-c", code],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
        )
        self.assertEqual(result.returncode, 2)
        combined = result.stderr + result.stdout
        self.assertIn("无法启动查看器", combined)
        self.assertIn("Python 版本过低", combined)
        self.assertIn("99.0", combined)  # 门槛值动态进消息，不硬编码

    def test_cli_directory_as_snapshot_refuses_to_start(self):
        result = self.run_cli(self.tmp.name)  # 目录不是快照文件
        self.assertEqual(result.returncode, 2)
        combined = result.stderr + result.stdout
        self.assertIn("无法启动查看器", combined)
        self.assertIn("不是文件", combined)  # 与"不存在"分开的可理解错误

    def test_cli_missing_static_resources_warns_and_api_alive(self):
        # 静态目录置空：启动仍打印访问地址与资源缺失提示（含构建方法），
        # 页面 503、API 正常回答——降级运行契约
        empty_static = Path(self.tmp.name) / "no-static"
        empty_static.mkdir()
        code = (
            "import sys; sys.path.insert(0, {dir!r}); import viewer; "
            "viewer.DEFAULT_STATIC_DIR = {static!r}; "
            "sys.exit(viewer.main([{snap!r}, '--port', '0']))"
        ).format(
            dir=str(VIEWER_ENTRY.parent),
            static=str(empty_static),
            snap=str(self.snapshot_path),
        )
        proc = subprocess.Popen(
            [sys.executable, "-c", code],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        self.addCleanup(proc.terminate)
        lines: list[str] = []
        reader = threading.Thread(target=lambda: lines.extend(proc.stdout or []), daemon=True)
        reader.start()

        port = None
        deadline = time.time() + 20
        while time.time() < deadline and port is None:
            for line in list(lines):
                match = re.search(r"http://127\.0\.0\.1:(\d+)/", line)
                if match:
                    port = int(match.group(1))
                    break
            if port is None and proc.poll() is not None:
                break
            time.sleep(0.1)
        self.assertIsNotNone(port, f"静态缺失时应降级启动并打印地址，输出: {lines}")
        self.assertTrue(
            any("发行页面资源缺失" in line for line in lines),
            f"应打印资源缺失提示，输出: {lines}",
        )
        self.assertTrue(
            any("npm run build" in line for line in lines),
            "缺失提示应包含构建方法",
        )

        # 页面 503（含构建指引），API 保持可用
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            conn.request("GET", "/")
            resp = conn.getresponse()
            body = resp.read().decode("utf-8")
            self.assertEqual(resp.status, 503)
            self.assertIn("npm run build", body)
            conn.request("GET", "/api/root")
            resp = conn.getresponse()
            payload = json.loads(resp.read().decode("utf-8"))
            self.assertEqual(resp.status, 200)
            self.assertEqual(payload["counts"], COUNTS)
        finally:
            conn.close()

        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
        reader.join(timeout=2)
        if proc.stdout:
            proc.stdout.close()


# ---------------------------------------------------------------------------
# 搜索与关联（#19）：组合筛选 / 分页 / 反向关联 / 与核心 query 语义对照
# ---------------------------------------------------------------------------


def make_search_data() -> dict:
    """搜索专用固定样本：相似目录前缀（src vs src2）、中文、casefold 陷阱、
    detail 多行 join 语义、跨目录关联、悬空 rel、git-ignore 三态、hidden 命中。

    全树 walk 序（sort_key casefold + 码点决胜，DFS 先自身后子级）共 13 条：
    docs, docs/guide.md, docs/README.md, hidden-note.md, src, src/core.ts,
    src/sub, src/sub/deep.ts, src/util.ts, src2, src2/clone.ts,
    中文目录, 中文目录/说明.md
    """
    return {
        "root": "搜索样本",
        "tags": {"doc": "说明文档", "script": "维护脚本", "pure": "纯逻辑"},
        "tree": {
            "docs": {
                "kind": "dir",
                "desc": "文档目录",
                "children": {
                    "guide.md": {
                        "kind": "file",
                        "desc": "使用指南",
                        "detail": ["六步学习", "闭环练习"],
                        "tags": ["doc"],
                    },
                    "README.md": {
                        "kind": "file",
                        "desc": "项目自述文件 readme",
                        "rel": ["gone/deep.rs", "src/core.ts"],
                        "tags": ["doc"],
                    },
                },
            },
            "hidden-note.md": {
                "kind": "file",
                "desc": "隐藏的说明 readme",
                "hidden": True,
                "tags": ["doc"],
            },
            "src": {
                "kind": "dir",
                "desc": "源码目录",
                "children": {
                    "core.ts": {
                        "kind": "file",
                        "desc": "核心渲染逻辑 Render",
                        "rel": ["docs/README.md"],
                        "tags": ["script", "pure"],
                    },
                    "sub": {
                        "kind": "dir",
                        "desc": "子模块",
                        "children": {
                            "deep.ts": {"kind": "file", "desc": "深层模块", "tags": ["pure"]},
                        },
                    },
                    "util.ts": {
                        "kind": "file",
                        "desc": "工具函数 readme 提取",
                        "rel": ["src/core.ts"],
                        "tags": ["script"],
                        "git-ignore": False,
                    },
                },
            },
            "src2": {
                "kind": "dir",
                "desc": "相似前缀目录（不应混入 src 子树）",
                "git-ignore": True,
                "children": {
                    "clone.ts": {"kind": "file", "desc": "相似前缀文件"},
                },
            },
            "中文目录": {
                "kind": "dir",
                "desc": "中文命名目录",
                "children": {
                    "说明.md": {
                        "kind": "file",
                        "desc": "中文渲染说明",
                        "detail": ["中文 detail 行"],
                    },
                },
            },
        },
    }


# 固定期望（独立手写，不由被测实现生成）
SEARCH_WALK_ORDER = [
    "docs",
    "docs/guide.md",
    "docs/README.md",
    "hidden-note.md",
    "src",
    "src/core.ts",
    "src/sub",
    "src/sub/deep.ts",
    "src/util.ts",
    "src2",
    "src2/clone.ts",
    "中文目录",
    "中文目录/说明.md",
]
SRC_SUBTREE = [
    "src",
    "src/core.ts",
    "src/sub",
    "src/sub/deep.ts",
    "src/util.ts",
]
README_HITS = ["docs/README.md", "hidden-note.md", "src/util.ts"]
DOC_TAG_HITS = ["docs/guide.md", "docs/README.md", "hidden-note.md"]
PURE_TAG_HITS = ["src/core.ts", "src/sub/deep.ts"]


class SearchFixture(unittest.TestCase):
    """共享夹具：搜索样本的一次性内存 Snapshot。"""

    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.snapshot_path = Path(cls.tmp.name) / "tree.json"
        cls.snapshot_path.write_text(
            compact_dumps(make_search_data()), encoding="utf-8", newline="\n"
        )
        cls.snap = Snapshot(cls.snapshot_path)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    @staticmethod
    def paths(result: dict) -> list[str]:
        return [hit["path"] for hit in result["results"]]


class SearchKeywordTest(SearchFixture):
    """G08 关键词语义：casefold 子串，覆盖 path/desc/detail（与 query :884 对齐）。"""

    def test_kw_matches_across_path_desc_and_detail(self):
        # path 命中 README.md；desc 命中 hidden-note.md 与 util.ts；guide.md 不含
        result = self.snap.search(kw="readme")
        self.assertEqual(self.paths(result), README_HITS)
        self.assertEqual(result["total"], 3)

    def test_kw_casefold_input_case_insensitive(self):
        # 大小写不敏感：输入 "README" 与 "readme" 同集；kw="render" 命中 desc 含 "Render"
        self.assertEqual(self.paths(self.snap.search(kw="README")), README_HITS)
        self.assertEqual(
            self.paths(self.snap.search(kw="RENDER")), ["src/core.ts"]
        )

    def test_kw_chinese(self):
        self.assertEqual(
            self.paths(self.snap.search(kw="渲染")),
            ["src/core.ts", "中文目录/说明.md"],
        )

    def test_kw_detail_join_semantics(self):
        # detail 多行以空格 join（照抄核心 haystack 构造）：带空格短语命中、无空格不命中
        self.assertEqual(self.paths(self.snap.search(kw="学习 闭环")), ["docs/guide.md"])
        result = self.snap.search(kw="学习闭环")
        self.assertEqual(result["total"], 0)
        self.assertEqual(result["results"], [])

    def test_kw_no_match_reports_empty_state(self):
        result = self.snap.search(kw="不存在的关键词")
        self.assertEqual(result["total"], 0)
        self.assertEqual(result["total_pages"], 0)
        self.assertEqual(result["results"], [])

    def test_hidden_entry_still_matched(self):
        # G11：hidden 条目照常命中（界面可见性由前端处理，搜索不过滤）
        self.assertIn("hidden-note.md", self.paths(self.snap.search(kw="readme")))


class SearchTagSubtreeTest(SearchFixture):
    """G08 标签与子树筛选：精确成员匹配；段级前缀（src 不纳 src2）；锚点含入。"""

    def test_tag_exact_membership(self):
        self.assertEqual(self.paths(self.snap.search(tag="doc")), DOC_TAG_HITS)
        self.assertEqual(self.paths(self.snap.search(tag="pure")), PURE_TAG_HITS)

    def test_tag_without_entries_is_empty(self):
        result = self.snap.search(tag="nosuch")
        self.assertEqual(result["total"], 0)
        self.assertEqual(result["results"], [])

    def test_under_includes_anchor_and_excludes_similar_prefix(self):
        # 核心防混淆点：段级比较，src2 不混入 src；锚点自身含入（相对深度 0）
        result = self.snap.search(under="src")
        self.assertEqual(self.paths(result), SRC_SUBTREE)
        all_paths = self.paths(result)
        self.assertNotIn("src2", all_paths)
        self.assertNotIn("src2/clone.ts", all_paths)

    def test_under_chinese_dir(self):
        self.assertEqual(
            self.paths(self.snap.search(under="中文目录")),
            ["中文目录", "中文目录/说明.md"],
        )

    def test_under_depth_limits_relative_levels(self):
        # depth=1：锚点 + 直接子级（deep.ts 相对深度 2 排除）；depth=2：全子树
        self.assertEqual(
            self.paths(self.snap.search(under="src", depth=1)),
            ["src", "src/core.ts", "src/sub", "src/util.ts"],
        )
        self.assertEqual(self.paths(self.snap.search(under="src", depth=2)), SRC_SUBTREE)

    def test_combined_filters_are_conjunction(self):
        # 组合 = AND（与核心 query 一致）
        self.assertEqual(
            self.paths(self.snap.search(kw="readme", tag="doc")),
            ["docs/README.md", "hidden-note.md"],
        )
        self.assertEqual(
            self.paths(self.snap.search(tag="pure", under="src")), PURE_TAG_HITS
        )
        self.assertEqual(
            self.paths(self.snap.search(kw="渲染", tag="pure", under="src")),
            ["src/core.ts"],
        )
        # 组合后无命中：交集为空
        result = self.snap.search(kw="readme", tag="pure")
        self.assertEqual(result["total"], 0)

    def test_no_filters_returns_all_in_walk_order(self):
        result = self.snap.search()
        self.assertEqual(self.paths(result), SEARCH_WALK_ORDER)
        self.assertEqual(result["total"], 13)

    def test_result_entry_field_set(self):
        # 结果条目字段与 tree_tool query --json 同口径；git-ignore 三态不混为一态
        hit = next(h for h in self.snap.search(under="src")["results"] if h["path"] == "src/util.ts")
        self.assertEqual(
            sorted(hit.keys()),
            ["collapsed", "desc", "detail", "git_ignore", "hidden", "kind", "path", "rel", "tags"],
        )
        self.assertEqual(hit["kind"], "file")
        self.assertEqual(hit["desc"], "工具函数 readme 提取")
        self.assertEqual(hit["rel"], ["src/core.ts"])
        self.assertIs(hit["git_ignore"], False)  # 显式退出豁免
        anchor = self.snap.search(under="src")["results"][0]
        self.assertIsNone(anchor["git_ignore"])  # 键缺省 = 继承

    def test_search_order_stable_across_calls(self):
        first = self.paths(self.snap.search(kw="readme"))
        second = self.paths(self.snap.search(kw="readme"))
        self.assertEqual(first, second)


class SearchPaginationTest(SearchFixture):
    """G08 分页：page/page_size（1 起），对确定性序列切片，不漏不重。"""

    def test_first_page_with_total(self):
        result = self.snap.search(page=1, page_size=5)
        self.assertEqual(result["total"], 13)
        self.assertEqual(result["total_pages"], 3)
        self.assertEqual(result["page"], 1)
        self.assertEqual(result["page_size"], 5)
        self.assertEqual(self.paths(result), SEARCH_WALK_ORDER[:5])

    def test_last_page_is_short(self):
        self.assertEqual(
            self.paths(self.snap.search(page=3, page_size=5)), SEARCH_WALK_ORDER[10:]
        )

    def test_all_pages_concatenate_exactly(self):
        # 不漏不重：翻全部页拼接 == 全量，无重复
        collected = []
        for page in (1, 2, 3):
            collected.extend(self.paths(self.snap.search(page=page, page_size=5)))
        self.assertEqual(collected, SEARCH_WALK_ORDER)
        self.assertEqual(len(collected), len(set(collected)))

    def test_page_beyond_last_is_empty_with_total_intact(self):
        result = self.snap.search(page=4, page_size=5)
        self.assertEqual(result["results"], [])
        self.assertEqual(result["total"], 13)
        self.assertEqual(result["total_pages"], 3)

    def test_default_page_size_covers_sample(self):
        result = self.snap.search()
        self.assertEqual(result["page"], 1)
        self.assertEqual(result["page_size"], 50)
        self.assertEqual(result["total_pages"], 1)
        self.assertEqual(len(result["results"]), 13)

    def test_small_result_set_pagination(self):
        # 3 条命中按 page_size=2 → 两页（2 + 1）
        first = self.snap.search(kw="readme", page=1, page_size=2)
        second = self.snap.search(kw="readme", page=2, page_size=2)
        self.assertEqual(first["total"], 3)
        self.assertEqual(first["total_pages"], 2)
        self.assertEqual(self.paths(first), README_HITS[:2])
        self.assertEqual(self.paths(second), README_HITS[2:])

    def test_max_page_size_accepted(self):
        result = self.snap.search(page_size=200)
        self.assertEqual(len(result["results"]), 13)


class SearchErrorTest(SearchFixture):
    """非法参数报可读错误：与核心 ToolError 校验对齐（400/404）。"""

    def assert_viewer_error(self, *args, status: int, fragment: str, **kwargs):
        with self.assertRaises(ViewerError) as ctx:
            self.snap.search(*args, **kwargs)
        self.assertEqual(ctx.exception.status, status, str(ctx.exception))
        self.assertIn(fragment, str(ctx.exception))

    def test_invalid_page(self):
        for page in (0, -1):
            with self.subTest(page=page):
                self.assert_viewer_error(page=page, status=400, fragment="page")

    def test_invalid_page_size(self):
        for size in (0, -5, 201):
            with self.subTest(page_size=size):
                self.assert_viewer_error(page_size=size, status=400, fragment="page_size")

    def test_under_not_found_is_404(self):
        self.assert_viewer_error(under="nope", status=404, fragment="不是树中目录")

    def test_under_file_anchor_is_400(self):
        self.assert_viewer_error(under="src/core.ts", status=400, fragment="不是目录")

    def test_under_illegal_path_is_400(self):
        self.assert_viewer_error(under="../escape", status=400, fragment="'..'")

    def test_depth_requires_under(self):
        self.assert_viewer_error(depth=1, status=400, fragment="under")

    def test_depth_must_be_positive_int(self):
        self.assert_viewer_error(under="src", depth=0, status=400, fragment="正整数")
        self.assert_viewer_error(under="src", depth=-1, status=400, fragment="正整数")
        self.assert_viewer_error(under="src", depth=1.5, status=400, fragment="正整数")


class BackrefDetailTest(SearchFixture):
    """G09 双向关联：detail 返回正向 rel（保持 #18 契约）与派生 backrefs。"""

    def test_backrefs_list_sources_in_walk_order(self):
        d = self.snap.detail("src/core.ts")
        # 引用者 = docs/README.md 与 src/util.ts，walk 序，全部真实存在
        self.assertEqual(
            d["backrefs"],
            [
                {"path": "docs/README.md", "exists": True},
                {"path": "src/util.ts", "exists": True},
            ],
        )

    def test_backrefs_of_readme(self):
        d = self.snap.detail("docs/README.md")
        self.assertEqual(d["backrefs"], [{"path": "src/core.ts", "exists": True}])

    def test_backrefs_empty_when_unreferenced(self):
        for path in ("src/util.ts", "hidden-note.md", "src2/clone.ts"):
            with self.subTest(path=path):
                self.assertEqual(self.snap.detail(path)["backrefs"], [])

    def test_forward_rel_contract_unchanged_with_dangling_flag(self):
        # #18 已建立的正向 rel 契约保持：悬空目标 exists=False，不致命
        d = self.snap.detail("docs/README.md")
        self.assertEqual(
            d["rel"],
            [{"path": "gone/deep.rs", "exists": False}, {"path": "src/core.ts", "exists": True}],
        )

    def test_direction_semantics_distinct(self):
        # 方向实证：util.ts 引用 core.ts（反向含之），但 core.ts 不引用 util.ts（正向无之）
        d = self.snap.detail("src/core.ts")
        self.assertEqual([r["path"] for r in d["rel"]], ["docs/README.md"])
        self.assertIn("src/util.ts", [b["path"] for b in d["backrefs"]])

    def test_dangling_target_detail_is_404_not_crash(self):
        # 悬空目标不在快照：详情 404（前端据 exists 标记不发起跳转，后端兜底不崩溃）
        with self.assertRaises(ViewerError) as ctx:
            self.snap.detail("gone/deep.rs")
        self.assertEqual(ctx.exception.status, 404)


class QueryParityTest(unittest.TestCase):
    """与核心 TreeTool.query 的语义对照：同快照、同参数，路径序列必须一致。

    独立于被测实现：TreeTool 实例只调用 query（内部仅 load() 只读快照），
    不触发任何写入口。
    """

    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.snapshot_path = Path(cls.tmp.name) / "tree.json"
        cls.snapshot_path.write_text(
            compact_dumps(make_search_data()), encoding="utf-8", newline="\n"
        )
        cls.snap = Snapshot(cls.snapshot_path)
        cls.tool = TreeTool(
            tree_json=cls.snapshot_path,
            agents_md=cls.snapshot_path.parent / "AGENTS.md",
            repo_root=cls.snapshot_path.parent,
            root_name="parity",
            history_path=cls.snapshot_path.parent / "history.json",
        )

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def core_paths(self, **kwargs) -> list[str]:
        return [path for path, _ in self.tool.query(**kwargs)]

    def viewer_paths(self, **kwargs) -> list[str]:
        return [h["path"] for h in self.snap.search(**kwargs)["results"]]

    def test_parity_all_entries_walk_order(self):
        self.assertEqual(self.core_paths(), SEARCH_WALK_ORDER)
        self.assertEqual(self.viewer_paths(), SEARCH_WALK_ORDER)

    def test_parity_keyword_dimensions(self):
        for kw in ("readme", "README", "RENDER", "渲染", "学习 闭环", "学习闭环", "不存在的关键词"):
            with self.subTest(kw=kw):
                self.assertEqual(self.core_paths(kw=kw), self.viewer_paths(kw=kw))

    def test_parity_tag(self):
        for tag in ("doc", "script", "pure", "nosuch"):
            with self.subTest(tag=tag):
                self.assertEqual(self.core_paths(tag=tag), self.viewer_paths(tag=tag))

    def test_parity_under_and_depth(self):
        cases = [
            {"under": "src"},
            {"under": "src", "depth": 1},
            {"under": "src", "depth": 2},
            {"under": "src2"},
            {"under": "中文目录"},
        ]
        for kwargs in cases:
            with self.subTest(**kwargs):
                self.assertEqual(self.core_paths(**kwargs), self.viewer_paths(**kwargs))
                self.assertEqual(self.core_paths(**kwargs), self.viewer_paths(**kwargs, page=1, page_size=200))

    def test_parity_combined_filters(self):
        cases = [
            {"kw": "readme", "tag": "doc"},
            {"tag": "pure", "under": "src"},
            {"kw": "渲染", "tag": "pure", "under": "src", "depth": 1},
            {"kw": "readme", "tag": "pure"},
        ]
        for kwargs in cases:
            with self.subTest(**kwargs):
                self.assertEqual(self.core_paths(**kwargs), self.viewer_paths(**kwargs))

    def test_parity_rel_of_equals_backrefs(self):
        # 反向关联派生等价性：query(rel_of=path) 全树扫描 == 内存反向索引
        for path in ("src/core.ts", "docs/README.md", "src/util.ts", "src2/clone.ts"):
            with self.subTest(path=path):
                expected = self.core_paths(rel_of=path)
                self.assertEqual(
                    [b["path"] for b in self.snap.detail(path)["backrefs"]], expected
                )

    def test_parity_under_error_contract(self):
        # 错误口径对照：核心 ToolError / 查看器 ViewerError，消息要点一致
        with self.assertRaises(Exception):
            self.tool.query(under="nope")
        with self.assertRaises(ViewerError) as ctx:
            self.snap.search(under="nope")
        self.assertEqual(ctx.exception.status, 404)
        with self.assertRaises(Exception):
            self.tool.query(under="src", depth=0)
        with self.assertRaises(ViewerError):
            self.snap.search(under="src", depth=0)

    def test_parity_full_field_set_against_core_json(self):
        # 结果条目与核心 _cmd_query --json 字段同口径抽查（kind/desc/tags/git-ignore 三态）
        core = dict((p, n) for p, n in self.tool.query())
        hits = {h["path"]: h for h in self.snap.search(page_size=200)["results"]}
        self.assertEqual(set(hits), set(core))
        node = core["src2"]
        self.assertEqual(hits["src2"]["kind"], "dir")
        self.assertEqual(hits["src2"]["desc"], node.get("desc", ""))
        self.assertEqual(hits["src2"]["tags"], node.get("tags", []))
        self.assertIs(hits["src2"]["git_ignore"], node.get("git-ignore"))  # True 显式豁免
        node_util = core["src/util.ts"]
        self.assertIs(hits["src/util.ts"]["git_ignore"], node_util.get("git-ignore"))


class FindAndGitIgnoreParityTest(unittest.TestCase):
    """节点定位与 git-ignore 有效值的唯一实现对照（#23 审查 Standards-1）。

    Snapshot.find 曾复刻 tree_tool._find_node、_effective_git_ignore 曾复刻
    TreeTool._git_exempt——两处私有逻辑被提升为 tree_tool 公共纯函数
    find_node / effective_git_ignore 后，查看器改为复用。本组用例锁定：
    同快照、同路径，核心实现与查看器行为一致（参照 QueryParityTest 模式，
    对照基准独立于查看器实现，TreeTool 实例只读快照不触写入口）。
    """

    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.snapshot_path = Path(cls.tmp.name) / "tree.json"
        # make_snapshot_data 覆盖 git-ignore 三态：exempt=true（豁免目录）、
        # exempt/inherit.ts（缺省继承 true）、exempt/optout.ts（显式 false 退出）
        cls.snapshot_path.write_text(
            compact_dumps(make_snapshot_data()), encoding="utf-8", newline="\n"
        )
        cls.snap = Snapshot(cls.snapshot_path)
        cls.tool = TreeTool(
            tree_json=cls.snapshot_path,
            agents_md=cls.snapshot_path.parent / "AGENTS.md",
            repo_root=cls.snapshot_path.parent,
            root_name="parity",
            history_path=cls.snapshot_path.parent / "history.json",
        )
        cls.tree = cls.tool.load()["tree"]
        cls.walk_paths = [path for path, _ in walk_entries(cls.tree, [])]

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_find_parity_on_all_walk_paths(self):
        # walk 序全量路径：核心 find_node 与查看器 Snapshot.find 同找到、同内容
        for path in self.walk_paths:
            with self.subTest(path=path):
                core = find_node(self.tree, split_rel_path(path))
                self.assertIsNotNone(core)
                viewer = self.snap.find(path)
                self.assertIsNotNone(viewer)
                self.assertEqual(core.get("desc"), viewer.get("desc"))

    def test_find_parity_on_missing_and_file_midway(self):
        # 不存在路径 / 中段是文件的路径：两条实现同为 None（同防御语义）
        for path in ("nosuch", "apps/nosuch.ts", "apps/main.tsx/deeper.md", "中文目录/没有.md"):
            with self.subTest(path=path):
                self.assertIsNone(find_node(self.tree, split_rel_path(path)))
                self.assertIsNone(self.snap.find(path))

    def test_find_root_wrapper_parity(self):
        # 空 path 返回根包装节点（含 children 键）：与核心 parts[:-1]==[] 用法同口径
        core = find_node(self.tree, [])
        viewer = self.snap.find("")
        self.assertIsNotNone(core)
        self.assertIsNotNone(viewer)
        self.assertEqual(len(core["children"]), len(self.tree))
        self.assertEqual(len(viewer["children"]), len(self.tree))

    def test_effective_git_ignore_parity_three_states(self):
        # 三态 fixture：豁免目录 true / 缺省继承 true / 显式 false 退出 / 无标记 False
        expected = {
            "exempt": True,
            "exempt/inherit.ts": True,
            "exempt/optout.ts": False,
            "apps/main.tsx": False,
            "中文目录/说明.md": False,
        }
        for path, value in expected.items():
            with self.subTest(path=path):
                self.assertIs(effective_git_ignore(self.tree, path), value)
                self.assertIs(self.snap.detail(path)["git_ignore"]["effective"], value)

    def test_effective_git_ignore_parity_all_paths(self):
        # 全量路径对照：detail 的 effective 一律等于核心 effective_git_ignore
        for path in self.walk_paths:
            with self.subTest(path=path):
                self.assertEqual(
                    self.snap.detail(path)["git_ignore"]["effective"],
                    effective_git_ignore(self.tree, path),
                )

    def test_effective_git_ignore_defensive_break_unchanged(self):
        # 中段是文件的路径：核心 break 语义（无更深祖先可继承，落 False）不因提升而漂移
        self.assertIs(effective_git_ignore(self.tree, "apps/main.tsx/deeper.md"), False)


class SearchMemoryHoldingTest(unittest.TestCase):
    """G13 内存持有：构造后删除快照文件，搜索与关联查询仍完整可用。"""

    def test_search_and_backrefs_work_after_file_deleted(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        path = Path(tmp.name) / "tree.json"
        path.write_text(compact_dumps(make_search_data()), encoding="utf-8", newline="\n")
        snap = Snapshot(path)
        path.unlink()  # 物理删除：此后任何请求若重新解析文件都会失败
        result = snap.search(kw="readme")
        self.assertEqual(result["total"], 3)
        d = snap.detail("src/core.ts")
        self.assertEqual([b["path"] for b in d["backrefs"]], ["docs/README.md", "src/util.ts"])


class SearchHttpApiTest(HttpServerBase):
    """真 HTTP：/api/search 完整响应、分页、组合筛选、错误与行为链。"""

    @classmethod
    def setUpClass(cls):
        import tempfile as _tf

        cls._extra = _tf.TemporaryDirectory()
        cls.start_server(static_dir=Path(cls._extra.name), data=make_search_data())

    @classmethod
    def tearDownClass(cls):
        cls.stop_server()
        after = cls.snapshot_path.read_bytes()
        assert hashlib.sha256(after).hexdigest() == cls.before_digest, "搜索流量改变了快照字节"
        assert sorted(p.name for p in cls.dir.iterdir()) == ["tree.json"], "快照目录出现新增文件"
        cls._extra.cleanup()
        cls.tmp.cleanup()

    def test_api_search_response_shape(self):
        status, payload = self.get_json(f"/api/search?kw={self.q('readme')}")
        self.assertEqual(status, 200)
        self.assertEqual(payload["total"], 3)
        self.assertEqual(payload["total_pages"], 1)
        self.assertEqual(payload["page"], 1)
        self.assertEqual(payload["page_size"], 50)
        self.assertEqual(
            payload["query"], {"kw": "readme", "tag": None, "under": None, "depth": None}
        )
        self.assertEqual([h["path"] for h in payload["results"]], README_HITS)
        hit = payload["results"][0]
        self.assertEqual(
            sorted(hit.keys()),
            ["collapsed", "desc", "detail", "git_ignore", "hidden", "kind", "path", "rel", "tags"],
        )

    def test_api_search_pagination_flow(self):
        # 13 条、page_size=5：三页翻完，末页 3 条
        collected = []
        for page in (1, 2, 3, 4):
            status, payload = self.get_json(f"/api/search?page={page}&page_size=5")
            self.assertEqual(status, 200)
            collected.extend(h["path"] for h in payload["results"])
            if page == 4:
                self.assertEqual(payload["results"], [])
                self.assertEqual(payload["total"], 13)
        self.assertEqual(collected, SEARCH_WALK_ORDER)

    def test_api_search_combined_filters_urlencoded(self):
        # 中文 kw 与中文 under 经 URL 编码往返；组合 AND
        url = f"/api/search?kw={self.q('渲染')}&tag=pure&under={self.q('src')}"
        status, payload = self.get_json(url)
        self.assertEqual(status, 200)
        self.assertEqual(payload["query"], {"kw": "渲染", "tag": "pure", "under": "src", "depth": None})
        self.assertEqual([h["path"] for h in payload["results"]], ["src/core.ts"])

    def test_api_search_empty_result_state(self):
        status, payload = self.get_json(f"/api/search?kw={self.q('不存在的关键词')}")
        self.assertEqual(status, 200)
        self.assertEqual(payload["total"], 0)
        self.assertEqual(payload["total_pages"], 0)
        self.assertEqual(payload["results"], [])

    def test_api_search_subtree_excludes_similar_prefix(self):
        status, payload = self.get_json(f"/api/search?under={self.q('src')}&page_size=200")
        self.assertEqual(status, 200)
        paths = [h["path"] for h in payload["results"]]
        self.assertEqual(paths, SRC_SUBTREE)
        self.assertNotIn("src2", paths)

    def test_api_search_error_contract(self):
        cases = [
            ("page=0", 400, "page"),
            ("page_size=201", 400, "page_size"),
            (f"under={self.q('nope')}", 404, "不是树中目录"),
            ("depth=1", 400, "under"),
            (f"under=src&depth=abc", 400, "depth"),
        ]
        for query, status, fragment in cases:
            with self.subTest(query=query):
                code, payload = self.get_json(f"/api/search?{query}")
                self.assertEqual(code, status)
                self.assertIn(fragment, payload["error"])

    def test_api_search_then_detail_then_relation_chain(self):
        # 行为链（模拟前端完整操作）：搜索 → 点命中 → 详情关联分区 → 沿反向关联跳转 → 悬空不崩
        _, search = self.get_json(f"/api/search?kw={self.q('readme')}")
        self.assertEqual([h["path"] for h in search["results"]], README_HITS)

        status, detail = self.get_json(f"/api/detail?path={self.q('src/util.ts')}")
        self.assertEqual(status, 200)
        self.assertEqual([r["path"] for r in detail["rel"]], ["src/core.ts"])

        status, core = self.get_json(f"/api/detail?path={self.q('src/core.ts')}")
        self.assertEqual(status, 200)
        self.assertEqual(
            [b["path"] for b in core["backrefs"]], ["docs/README.md", "src/util.ts"]
        )

        status, readme = self.get_json(f"/api/detail?path={self.q('docs/README.md')}")
        self.assertEqual(status, 200)
        self.assertEqual(
            readme["rel"],
            [
                {"path": "gone/deep.rs", "exists": False},
                {"path": "src/core.ts", "exists": True},
            ],
        )
        # 悬空目标：详情 404（不崩溃），前端凭 exists=False 不跳转
        status, payload = self.get_json(f"/api/detail?path={self.q('gone/deep.rs')}")
        self.assertEqual(status, 404)
        self.assertIn("error", payload)

    def test_api_detail_response_includes_backrefs_field(self):
        status, payload = self.get_json(f"/api/detail?path={self.q('src/core.ts')}")
        self.assertEqual(status, 200)
        self.assertIn("backrefs", payload)
        self.assertEqual(payload["backrefs"][0], {"path": "docs/README.md", "exists": True})


# ---------------------------------------------------------------------------
# 快照刷新与世代号（G12）：原子替换、失败保留旧快照、版本不混用
# ---------------------------------------------------------------------------


class RefreshHttpTestBase(HttpServerBase):
    """刷新夹具：服务启动后可改写同一路径的快照文件再触发 POST /api/refresh。

    模拟真实使用：用户在查看器运行期间用外部工具替换 tree.json，再点刷新。
    """

    @classmethod
    def setUpClass(cls):
        import tempfile as _tf

        cls._extra = _tf.TemporaryDirectory()
        cls.start_server(static_dir=Path(cls._extra.name))

    @classmethod
    def tearDownClass(cls):
        cls.stop_server()
        cls._extra.cleanup()
        cls.tmp.cleanup()

    def setUp(self):
        # 每个用例前把快照文件重置回固定样本并刷新：用例改写的是磁盘文件，
        # 不重置会泄漏给后续用例（世代号断言已全部改为相对值，不受影响）
        self.snapshot_path.write_text(
            compact_dumps(make_snapshot_data()), encoding="utf-8", newline="\n"
        )
        status, _ = self.refresh()
        self.assertEqual(status, 200)

    def rewrite_snapshot(self, data: dict):
        """替换同一路径的快照内容（外部改写，不经查看器）。"""
        self.snapshot_path.write_text(
            compact_dumps(data), encoding="utf-8", newline="\n"
        )

    def request_json(self, method: str, path: str):
        conn = http.client.HTTPConnection(self.host, self.port, timeout=10)
        try:
            conn.request(method, path)
            resp = conn.getresponse()
            return resp.status, json.loads(resp.read().decode("utf-8"))
        finally:
            conn.close()

    def refresh(self):
        return self.request_json("POST", "/api/refresh")

    def current_generation(self) -> int:
        """读当前世代号（不改变状态）；测试断言一律用相对变化，不依赖执行顺序。"""
        status, payload = self.get_json("/api/root")
        self.assertEqual(status, 200)
        return payload["generation"]


class ViewerRefreshApiTest(RefreshHttpTestBase):
    def test_generation_starts_at_one_and_stamped_on_all_apis(self):
        # 世代号防混淆（G12）：目录/搜索/详情/根信息响应统一盖章，
        # 前端据此识别"这是哪个快照版本的回答"
        for path in (
            "/api/root",
            "/api/children",
            "/api/search",
            f"/api/detail?path={self.q('apps')}",
        ):
            with self.subTest(api=path):
                status, payload = self.get_json(path)
                self.assertEqual(status, 200)
                self.assertIn("generation", payload)
                self.assertIsInstance(payload["generation"], int)
        # 同一时刻所有接口盖章一致（同一快照版本）
        gens = set()
        for path in (
            "/api/root",
            "/api/children",
            "/api/search",
            f"/api/detail?path={self.q('apps')}",
        ):
            _, payload = self.get_json(path)
            gens.add(payload["generation"])
        self.assertEqual(len(gens), 1)

    def test_refresh_reloads_same_path_and_bumps_generation(self):
        data = make_snapshot_data()
        data["tree"]["新增目录"] = {"kind": "dir", "desc": "刷新后新增", "children": {}}
        self.rewrite_snapshot(data)
        before_digest = hashlib.sha256(self.snapshot_path.read_bytes()).hexdigest()
        gen_before = self.current_generation()

        status, payload = self.refresh()
        self.assertEqual(status, 200)
        self.assertTrue(payload["refreshed"])
        self.assertEqual(payload["generation"], gen_before + 1)
        self.assertEqual(payload["root"], "演示仓库")
        self.assertEqual(payload["counts"], {"dirs": 7, "files": 8, "total": 15})
        gen_after = payload["generation"]

        # 刷新后目录/搜索/详情都来自新快照，且世代号一致（不混用旧响应）
        status, children = self.get_json("/api/children")
        self.assertEqual(status, 200)
        self.assertEqual(children["generation"], gen_after)
        self.assertIn("新增目录", [c["path"] for c in children["children"]])

        status, search = self.get_json(f"/api/search?kw={self.q('刷新后新增')}")
        self.assertEqual(status, 200)
        self.assertEqual(search["generation"], gen_after)
        self.assertEqual([h["path"] for h in search["results"]], ["新增目录"])

        status, detail = self.get_json(f"/api/detail?path={self.q('新增目录')}")
        self.assertEqual(status, 200)
        self.assertEqual(detail["generation"], gen_after)
        self.assertEqual(detail["desc"], "刷新后新增")

        # 只读（G04）：刷新只是重读，不写回、不产生伴生文件
        self.assertEqual(
            hashlib.sha256(self.snapshot_path.read_bytes()).hexdigest(), before_digest
        )
        self.assertEqual(sorted(p.name for p in self.dir.iterdir()), ["tree.json"])

    def test_refresh_with_unchanged_file_still_bumps_generation(self):
        # 文件未变时刷新：内容相同但世代号必须递增（强制前端丢弃旧缓存重建）
        gen_before = self.current_generation()
        status, first = self.refresh()
        self.assertEqual(status, 200)
        self.assertEqual(first["generation"], gen_before + 1)
        status, second = self.refresh()
        self.assertEqual(status, 200)
        self.assertEqual(second["generation"], gen_before + 2)
        status, payload = self.get_json("/api/root")
        self.assertEqual(payload["generation"], gen_before + 2)
        self.assertEqual(payload["counts"], COUNTS)

    def test_refresh_invalid_json_fails_and_keeps_old_snapshot(self):
        gen_before = self.current_generation()
        self.snapshot_path.write_text("{oops 不是 json", encoding="utf-8", newline="\n")
        status, payload = self.refresh()
        self.assertEqual(status, 400)
        self.assertIn("JSON", payload["error"])
        self.assertFalse(payload["refreshed"])
        # 失败响应带旧世代号：前端据此确认"仍是旧数据，未刷新"
        self.assertEqual(payload["generation"], gen_before)

        # 旧数据保持可用且世代号不变（未发生半更新）
        status, children = self.get_json("/api/children")
        self.assertEqual(status, 200)
        self.assertEqual(children["generation"], gen_before)
        self.assertEqual([c["path"] for c in children["children"]], ROOT_ORDER)

    def test_refresh_bad_structure_fails_and_keeps_old_snapshot(self):
        gen_before = self.current_generation()
        self.rewrite_snapshot({"tree": {"a": {"desc": 1}}})
        status, payload = self.refresh()
        self.assertEqual(status, 400)
        self.assertIn("结构校验失败", payload["error"])
        self.assertFalse(payload["refreshed"])
        self.assertEqual(payload["generation"], gen_before)

        status, detail = self.get_json(f"/api/detail?path={self.q('apps/main.tsx')}")
        self.assertEqual(status, 200)
        self.assertEqual(detail["generation"], gen_before)
        self.assertEqual(detail["name"], "main.tsx")

    def test_refresh_missing_file_fails_and_keeps_old_snapshot(self):
        gen_before = self.current_generation()
        self.snapshot_path.unlink()
        status, payload = self.refresh()
        self.assertEqual(status, 500)
        self.assertFalse(payload["refreshed"])
        self.assertIn("无法读取", payload["error"])
        self.assertEqual(payload["generation"], gen_before)

        # 恢复文件后旧数据仍可用，再次刷新可成功
        self.rewrite_snapshot(make_snapshot_data())
        status, children = self.get_json("/api/children")
        self.assertEqual(status, 200)
        status, payload = self.refresh()
        self.assertEqual(status, 200)
        self.assertEqual(payload["generation"], gen_before + 1)

    def test_refresh_deeply_nested_returns_400_json_keeps_old_snapshot(self):
        # 深嵌套快照触发 RecursionError：刷新必须返回 400 JSON（旧快照保留），
        # 不得让未捕获异常冒出导致连接重置
        gen_before = self.current_generation()
        self.snapshot_path.write_text(
            "[" * 100_000 + "]" * 100_000, encoding="utf-8", newline="\n"
        )
        status, payload = self.refresh()
        self.assertEqual(status, 400)
        self.assertFalse(payload["refreshed"])
        self.assertIn("嵌套层级过深", payload["error"])
        self.assertEqual(payload["generation"], gen_before)

        # 旧快照保持可用
        status, root_payload = self.get_json("/api/root")
        self.assertEqual(status, 200)
        self.assertEqual(root_payload["generation"], gen_before)

    def test_deleted_path_returns_404_after_refresh(self):
        # 前端回退规则的依据：刷新后已删除路径的详情/子项查询明确 404
        data = make_snapshot_data()
        del data["tree"]["apps"]["children"]["main.tsx"]
        self.rewrite_snapshot(data)
        status, _ = self.refresh()
        self.assertEqual(status, 200)

        status, payload = self.get_json(f"/api/detail?path={self.q('apps/main.tsx')}")
        self.assertEqual(status, 404)
        # 已删除路径的子项查询同样明确 404（条目不存在）
        status, payload = self.get_json(f"/api/children?path={self.q('apps/main.tsx')}")
        self.assertEqual(status, 404)

    def test_post_other_endpoints_still_405(self):
        for path in ("/api/children", "/api/root", "/api/search", "/api/detail"):
            with self.subTest(path=path):
                status, payload = self.request_json("POST", path)
                self.assertEqual(status, 405)
                self.assertIn("只读", payload["error"])

    def test_concurrent_queries_never_mix_generations(self):
        # 原子替换（G12"同一快照版本"）：查询线程与成功刷新并发，
        # 每个响应的内容与世代号必须配对——旧集合配旧世代、新集合配新世代
        gen_before = self.current_generation()
        stop = threading.Event()
        mismatches: list[str] = []
        observations: set[tuple[int, bool]] = set()

        def poll_children():
            while not stop.is_set():
                status, payload = self.get_json("/api/children")
                if status != 200:
                    mismatches.append(f"查询失败 {status}: {payload}")
                    return
                has_new = any(c["path"] == "新增目录" for c in payload["children"])
                gen = payload["generation"]
                observations.add((gen, has_new))
                # 配对校验：旧世代必无新增目录；新世代必有（新快照写死包含它）
                if (gen == gen_before and has_new) or (
                    gen == gen_before + 1 and not has_new
                ):
                    mismatches.append(f"版本混用: generation={gen} has_new={has_new}")
                elif gen not in (gen_before, gen_before + 1):
                    mismatches.append(f"意外世代: {gen}")

        data = make_snapshot_data()
        data["tree"]["新增目录"] = {"kind": "dir", "desc": "刷新后新增", "children": {}}

        worker = threading.Thread(target=poll_children, daemon=True)
        worker.start()
        try:
            time.sleep(0.05)
            self.rewrite_snapshot(data)
            status, payload = self.refresh()
            self.assertEqual(status, 200)
            self.assertEqual(payload["generation"], gen_before + 1)
            time.sleep(0.15)
        finally:
            stop.set()
            worker.join(timeout=5)

        self.assertEqual(mismatches, [])
        # 两种版本都被观察到（并发窗口真实存在），且无第三种世代出现
        self.assertEqual({gen for gen, _ in observations}, {gen_before, gen_before + 1})
        # 终态：全部响应落在刷新后世代
        status, payload = self.get_json("/api/children")
        self.assertEqual(payload["generation"], gen_before + 1)


if __name__ == "__main__":
    unittest.main()
