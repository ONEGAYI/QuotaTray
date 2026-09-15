"""file-tree 技能脚本契约测试。

运行：python .agents/skills/file-tree/scripts/tree_tool_test.py
沙箱模式：所有用例在临时目录中构造 tree.json / SKILL.md / AGENTS.md，不触仓库。
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from tree_tool import (  # noqa: E402
    ToolError,
    TreeTool,
    _cmd_add,
    _cmd_add_batch,
    _cmd_get,
    _cmd_mark,
    _cmd_mv,
    _cmd_mv_batch,
    _cmd_query,
    _cmd_rm_batch,
    _cmd_root,
    _cmd_view_add,
    _cmd_view_doc,
    _cmd_view_list,
    _cmd_view_rm,
    append_view_block,
    block_content,
    canonical_form,
    default_history_path,
    dumps_canonical,
    dumps_canonical_legacy,
    insert_block_at_line,
    is_canonical_text,
    normalize_data,
    remove_view_block,
    render_silhouette,
    replace_block,
    resolve_git_dir,
    sort_key,
    split_rel_path,
    view_tree_markers,
)

AGENTS_TEMPLATE = "# AGENTS\n"


def make_data() -> dict:
    return {
        "tags": {"pure": "纯函数", "test": "测试"},
        "tree": {
            "apps": {
                "desc": "应用层",
                "children": {
                    "main.tsx": {"desc": "入口", "detail": ["分派主窗", "双面板"]},
                    "util.ts": {"desc": "工具", "detail": ["纯函数工具集"], "tags": ["pure"]},
                },
            },
            "Cargo.toml": {"desc": "根配置", "detail": ["workspace 根：成员与依赖版本、release 配置"]},
        },
    }


def legacy_dumps(data: dict) -> str:
    """独立旧编码规则（两空格缩进 + 末尾 LF）：标准库直调，不经被测写入路径。

    专用于构造"结构规范、仅排版旧"的历史样本。
    """
    return json.dumps(data, ensure_ascii=False, indent=2) + "\n"


def compact_dumps(data: dict) -> str:
    """独立新编码规则（紧凑单行 + 末尾 LF）：标准库直调，与被测实现无转发关系。

    作为参数化用例的独立期望（锁死 separators 组合本身，而非转发实现参数）。
    """
    return json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n"


class SandboxTest(unittest.TestCase):
    """基类：为每个用例搭临时沙箱并返回配置好的 TreeTool。"""

    def make_tool(
        self,
        data: dict | None = None,
        git_files: set[str] | None = None,
        tracked_files: set[str] | None = None,
        history_limit: int = 20,
    ) -> TreeTool:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        skill_dir = root / ".agents" / "skills" / "file-tree"
        (skill_dir / "scripts").mkdir(parents=True)
        tool = TreeTool(
            tree_json=skill_dir / "tree.json",
            agents_md=root / "AGENTS.md",
            repo_root=root,
            root_name="Demo",
            history_path=skill_dir / ".history.json",
            history_limit=history_limit,
        )
        tool.write_data(data if data is not None else make_data())
        tool.agents_md.write_text(AGENTS_TEMPLATE, encoding="utf-8", newline="\n")
        if git_files is not None:
            tool.git_files_override = git_files
        if tracked_files is not None:
            tool.git_tracked_override = tracked_files
        return tool


class SortKeyTest(unittest.TestCase):
    def test_case_insensitive_then_codepoint(self):
        names = ["b.ts", "A.ts", "a.ts", "B.ts", "_x", "Zz"]
        self.assertEqual(sorted(names, key=sort_key), ["_x", "A.ts", "a.ts", "B.ts", "b.ts", "Zz"])


class SplitPathTest(unittest.TestCase):
    def test_valid(self):
        self.assertEqual(split_rel_path("a/b/c.rs"), ["a", "b", "c.rs"])
        self.assertEqual(split_rel_path("a//b/"), ["a", "b"])

    def test_rejects_absolute_and_dotdot(self):
        for bad in ("/a", "a/../b", "..", "C:\\a", "a/./b"):
            with self.assertRaises(ToolError, msg=bad):
                split_rel_path(bad)


class NormalizeTest(unittest.TestCase):
    def test_sorts_and_drops_empty_and_keeps_detail_order(self):
        data = {
            "tags": {"z": "", "a": "说明"},
            "tree": {
                "b.rs": {"desc": "b", "detail": [], "rel": ["x/a.rs", "x/a.rs"], "tags": ["t", "t"]},
                "a.rs": {"desc": "a", "detail": ["二", "一"], "children": {"z.rs": {"desc": "z"}, "y.rs": {"desc": "y"}}},
            },
        }
        out = normalize_data(data)
        self.assertEqual(list(out["tree"]), ["a.rs", "b.rs"])  # 排序
        self.assertEqual(list(out["tree"]["a.rs"]["children"]), ["y.rs", "z.rs"])  # 子级排序
        self.assertEqual(out["tree"]["a.rs"]["detail"], ["二", "一"])  # detail 顺序保留
        self.assertNotIn("detail", out["tree"]["b.rs"])  # 空列表移除
        self.assertEqual(out["tree"]["b.rs"]["rel"], ["x/a.rs"])  # rel 去重排序
        self.assertEqual(out["tree"]["b.rs"]["tags"], ["t"])
        self.assertEqual(list(out["tags"]), ["a"])  # 空说明的标签移除
        # 字段固定顺序：kind, desc, detail, rel, tags, collapsed, hidden, children
        keys = list(out["tree"]["a.rs"])
        self.assertEqual(keys, ["kind", "desc", "detail", "children"])

    def test_field_order_canonical(self):
        node = {"children": {}, "tags": ["t"], "rel": ["a.rs"], "detail": ["d"], "desc": "x"}
        out = normalize_data({"tags": {"t": "说明"}, "tree": {"n": node}})
        self.assertEqual(list(out["tree"]["n"]), ["kind", "desc", "detail", "rel", "tags", "children"])

    def test_render_flags_false_dropped_and_ordered(self):
        node = {"desc": "x", "collapsed": False, "hidden": False}
        out = normalize_data({"tags": {}, "tree": {"n": node}})
        self.assertEqual(list(out["tree"]["n"]), ["kind", "desc"])  # false 默认值不落盘
        node = {"desc": "x", "hidden": True, "collapsed": True, "children": {}}
        out = normalize_data({"tags": {}, "tree": {"n": node}})
        self.assertEqual(list(out["tree"]["n"]), ["kind", "desc", "collapsed", "hidden", "children"])

    def test_render_flags_must_be_bool(self):
        data = {"tags": {}, "tree": {"n": {"desc": "x", "hidden": "yes"}}}
        with self.assertRaises(ToolError):
            normalize_data(data)
        data = {"tags": {}, "tree": {"n": {"desc": "x", "children": {}, "collapsed": 1}}}
        with self.assertRaises(ToolError):
            normalize_data(data)

    def test_collapsed_rejected_on_file_node(self):
        data = {"tags": {}, "tree": {"f.rs": {"desc": "x", "collapsed": True}}}
        with self.assertRaises(ToolError):
            normalize_data(data)

    def test_git_ignore_flag_canonical(self):
        node = {"desc": "x", "git-ignore": False}
        out = normalize_data({"tags": {}, "tree": {"n": node}})
        # git-ignore 三态：键在即显式设置，true/false 均落盘（false 覆写祖先豁免）；缺省不落盘 = 继承
        self.assertEqual(list(out["tree"]["n"]), ["kind", "desc", "git-ignore"])
        self.assertIs(out["tree"]["n"]["git-ignore"], False)
        node = {"desc": "x", "hidden": False, "git-ignore": True}
        out = normalize_data({"tags": {}, "tree": {"n": node}})
        # hidden 维持旧惯例（false 默认值不落盘），仅 git-ignore 特殊
        self.assertEqual(list(out["tree"]["n"]), ["kind", "desc", "git-ignore"])
        self.assertIs(out["tree"]["n"]["git-ignore"], True)
        with self.assertRaises(ToolError):  # 非 bool 拒绝（同 collapsed/hidden）
            normalize_data({"tags": {}, "tree": {"n": {"desc": "x", "git-ignore": "yes"}}})


class AddRmTest(SandboxTest):
    def test_add_creates_parent_chain(self):
        tool = self.make_tool(data={"tags": {}, "tree": {}})
        tool.add("a/b/c.rs", desc="新文件")
        node = tool.get("a/b/c.rs")
        self.assertEqual(node["desc"], "新文件")
        self.assertEqual(tool.get("a")["desc"], "")  # 中间目录待补 desc
        self.assertEqual(tool.get("a/b")["children"]["c.rs"]["desc"], "新文件")

    def test_add_upsert_keeps_unspecified_fields(self):
        tool = self.make_tool()
        tool.add("apps/main.tsx", desc="旧", detail=["旧细节"], rel=["Cargo.toml"], tags=["pure"])
        tool.add("apps/main.tsx", desc="新")
        node = tool.get("apps/main.tsx")
        self.assertEqual(node["desc"], "新")
        self.assertEqual(node["detail"], ["旧细节"])
        self.assertEqual(node["rel"], ["Cargo.toml"])
        self.assertEqual(node["tags"], ["pure"])

    def test_add_dir_entry(self):
        tool = self.make_tool(data={"tags": {}, "tree": {}})
        tool.add("logs", desc="日志", is_dir_entry=True)
        self.assertEqual(tool.get("logs"), {"kind": "dir", "desc": "日志", "children": {}})

    def test_add_rejects_unknown_tag(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add("apps/x.rs", desc="x", tags=["nope"])

    def test_add_rejects_bad_path_and_dangling_rel(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add("../escape.rs", desc="x")
        with self.assertRaises(ToolError):
            tool.add("apps/x.rs", desc="x", rel=["not/in/tree.rs"])


class CmdAddTagsTest(SandboxTest):
    """CLI 层 _cmd_add 的 --tags 解析契约：逗号分隔、去空白、None 直通。"""

    def run_cmd_add(self, tool: TreeTool, tags):
        import types

        args = types.SimpleNamespace(
            path="apps/new.ts", desc="新文件", detail=None, rel=None, tags=tags, dir=False,
            collapsed=None, hidden=None, git_ignore=None,
        )
        _cmd_add(tool, args)

    def test_comma_separated_split_into_list(self):
        tool = self.make_tool()
        self.run_cmd_add(tool, "pure,test")
        self.assertEqual(tool.get("apps/new.ts")["tags"], ["pure", "test"])

    def test_trims_whitespace_and_drops_empty_segments(self):
        tool = self.make_tool()
        self.run_cmd_add(tool, " pure , , test ")
        self.assertEqual(tool.get("apps/new.ts")["tags"], ["pure", "test"])

    def test_none_keeps_field_absent(self):
        tool = self.make_tool()
        self.run_cmd_add(tool, None)
        self.assertNotIn("tags", tool.get("apps/new.ts"))

    def test_single_tag_not_split_into_chars(self):
        tool = self.make_tool()
        self.run_cmd_add(tool, "pure")
        self.assertEqual(tool.get("apps/new.ts")["tags"], ["pure"])

    def test_rm_prunes_empty_parents(self):
        tool = self.make_tool(data={"tags": {}, "tree": {}})
        tool.add("a/b/c.rs", desc="x")
        tool.rm("a/b/c.rs")
        self.assertEqual(tool.load()["tree"], {})

    def test_rm_keeps_siblings(self):
        tool = self.make_tool()
        tool.rm("apps/util.ts")
        self.assertIn("main.tsx", tool.get("apps")["children"])

    def test_rm_missing_raises(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.rm("nope.rs")


class TagVocabTest(SandboxTest):
    def test_add_and_remove(self):
        tool = self.make_tool()
        tool.tag_add("generated", desc="生成物")
        self.assertEqual(tool.load()["tags"]["generated"], "生成物")
        tool.tag_rm("generated")
        self.assertNotIn("generated", tool.load()["tags"])

    def test_remove_in_use_rejected(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.tag_rm("pure")  # util.ts 在用

    def test_duplicate_rejected(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.tag_add("pure", desc="重复")


class RenderTest(SandboxTest):
    def test_brief_snapshot(self):
        tool = self.make_tool()
        self.assertEqual(
            tool.render_brief_tree(),
            "\n".join(
                [
                    "Demo/",
                    "├── apps/      # 应用层",
                    "│   ├── main.tsx # 入口",
                    "│   └── util.ts  # 工具",
                    "└── Cargo.toml # 根配置",
                ]
            ),
        )

    def test_tags_table(self):
        tool = self.make_tool()
        self.assertEqual(
            tool.render_tags_table(),
            "\n".join(
                [
                    "| 标签 | 说明 |",
                    "| --- | --- |",
                    "| `pure` | 纯函数 |",
                    "| `test` | 测试 |",
                ]
            ),
        )

    def test_empty_desc_renders_without_comment(self):
        tool = self.make_tool()
        tool.add("empty_dir", desc="", is_dir_entry=True)
        tool.render()
        text = tool.agents_md.read_text(encoding="utf-8")
        self.assertIn("\n└── empty_dir/\n", text)

    def test_render_replaces_existing_marker(self):
        tool = self.make_tool()
        tool.render()  # 首跑附加两块
        tool.agents_md.write_text(
            tool.agents_md.read_text(encoding="utf-8").replace("# 入口", "# 被手改"),
            encoding="utf-8",
            newline="\n",
        )
        updated = tool.render()  # 再次渲染按标记替换
        self.assertEqual(updated, [tool.agents_md])
        agents = tool.agents_md.read_text(encoding="utf-8")
        self.assertIn("# 入口", agents)
        self.assertNotIn("# 被手改", agents)
        self.assertEqual(tool.render(), [])  # 幂等

    def test_render_appends_missing_blocks_to_tail(self):
        tool = self.make_tool()
        tool.render()
        agents = tool.agents_md.read_text(encoding="utf-8")
        # 简版树与词表块附加到尾部：带小节标题 + code fence 包裹树
        self.assertIn("## 文件树（简版速览）", agents)
        self.assertIn("## 文件树标签词表", agents)
        self.assertIn("# 入口", agents)
        self.assertIn("`pure`", agents)
        # 附加的树块被 code fence 包裹
        tail = agents[agents.index("## 文件树（简版速览）"):]
        self.assertTrue(tail.index("```") < tail.index("# 入口") < tail.index("```", tail.index("# 入口")))

    def test_render_creates_agents_when_missing(self):
        tool = self.make_tool()
        tool.agents_md.unlink()
        tool.render()
        agents = tool.agents_md.read_text(encoding="utf-8")
        self.assertTrue(agents.startswith("# AGENTS"))
        for marker in ("file-tree:tree:begin", "file-tree:tags:begin"):
            self.assertIn(marker, agents)
        errors, _ = tool.check()
        self.assertEqual(errors, [])

    def test_render_rejects_orphan_end_marker(self):
        tool = self.make_tool()
        tool.agents_md.write_text(
            "# AGENTS\n\n<!-- file-tree:tree:end -->\n", encoding="utf-8", newline="\n"
        )
        with self.assertRaises(ToolError):
            tool.render()


class KindFieldTest(SandboxTest):
    """kind 派生字段：由 children 判据推导（file/dir），落盘供机器消费，不参与渲染。"""

    def test_kind_derived_and_persisted(self):
        tool = self.make_tool()
        self.assertEqual(tool.get("apps")["kind"], "dir")
        self.assertEqual(tool.get("Cargo.toml")["kind"], "file")
        self.assertEqual(tool.get("apps/main.tsx")["kind"], "file")

    def test_hand_edited_kind_corrected_on_write(self):
        tool = self.make_tool()
        data = tool.load()
        data["tree"]["Cargo.toml"]["kind"] = "dir"  # 手改成错误值
        tool.write_data(data)  # 规范化写纠正为推导值
        self.assertEqual(tool.get("Cargo.toml")["kind"], "file")

    def test_legacy_data_without_kind_migrated_by_write(self):
        tool = self.make_tool()  # make_tool 经 write_data 已带 kind；手放旧形态数据
        legacy = {"tags": {}, "tree": {"old.rs": {"desc": "旧", "detail": ["旧数据"]}}}
        tool.tree_json.write_text(
            json.dumps(legacy, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8", newline="\n",
        )
        tool.write_data(tool.load())
        self.assertEqual(tool.get("old.rs")["kind"], "file")
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(errors, [])

    def test_kind_in_query_json(self):
        import contextlib
        import io
        import types

        tool = self.make_tool()
        args = types.SimpleNamespace(kw=None, tag=None, rel_of=None, under=None, depth=None, json=True)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            _cmd_query(tool, args)
        payload = json.loads(buf.getvalue())
        by_path = {e["path"]: e for e in payload}
        self.assertEqual(by_path["apps"]["kind"], "dir")
        self.assertEqual(by_path["Cargo.toml"]["kind"], "file")

    def test_query_json_keeps_git_ignore_tri_state(self):
        # --json 三态保真：null=缺省继承、false=显式退出、true=豁免（二态默认值会把 null 拍平成 false）
        import contextlib
        import io
        import types

        tool = self.make_tool()
        tool.add("apps/exit.rs", desc="退出", detail=["x"], git_ignore=False)
        tool.add("apps/kept.rs", desc="豁免", detail=["x"], git_ignore=True)
        args = types.SimpleNamespace(kw=None, tag=None, rel_of=None, under="apps", depth=None, json=True)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            _cmd_query(tool, args)
        by_path = {e["path"]: e for e in json.loads(buf.getvalue())}
        self.assertIsNone(by_path["apps/main.tsx"]["git-ignore"])  # 缺省继承
        self.assertIs(by_path["apps/exit.rs"]["git-ignore"], False)  # 显式退出
        self.assertIs(by_path["apps/kept.rs"]["git-ignore"], True)

    def test_cli_query_wires_under_and_depth(self):
        # 锁定 CLI 层接线：_cmd_query 必须把 under/depth 传给方法层（回归：曾整层漏传、参数被静默丢弃）
        import contextlib
        import io
        import types

        tool = self.make_tool()
        tool.add("apps/ui", desc="UI 层", is_dir_entry=True)
        tool.add("apps/ui/button.tsx", desc="按钮", detail=["x"])
        args = types.SimpleNamespace(kw=None, tag=None, rel_of=None, under="apps", depth=1, json=True)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            _cmd_query(tool, args)
        self.assertEqual(
            [e["path"] for e in json.loads(buf.getvalue())],
            ["apps", "apps/main.tsx", "apps/ui", "apps/util.ts"],
        )

    def test_kind_not_rendered(self):
        tool = self.make_tool()
        self.assertNotIn("kind", tool.render_brief_tree())


class RenderControlTest(SandboxTest):
    """collapsed / hidden 渲染控制字段：只影响 AGENTS.md 简版树渲染，不影响数据与校验。"""

    def test_collapsed_dir_renders_ellipsis_without_children(self):
        tool = self.make_tool()
        tool.add("build", desc="构建产物", is_dir_entry=True)
        tool.add("build/out.exe", desc="产物", detail=["完整描述"])
        tool.add("build/tmp.rs", desc="临时", detail=["完整描述"])
        data = tool.load()
        data["tree"]["build"]["collapsed"] = True
        tool.write_data(data)
        rendered = tool.render_brief_tree()
        self.assertIn("build/…", rendered)  # 目录名后带省略号
        self.assertNotIn("out.exe", rendered)  # children 不展开
        self.assertNotIn("tmp.rs", rendered)

    def test_collapsed_empty_dir_renders_plain(self):
        tool = self.make_tool()
        tool.add("empty", desc="空目录", is_dir_entry=True, collapsed=True)
        rendered = tool.render_brief_tree()
        self.assertIn("empty/", rendered)
        self.assertNotIn("…", rendered)  # 空目录无可折叠内容，不加省略号

    def test_hidden_excludes_entry_and_subtree(self):
        tool = self.make_tool()
        tool.add("secrets", desc="密钥", is_dir_entry=True)
        tool.add("secrets/token.rs", desc="令牌", detail=["完整描述"])
        data = tool.load()
        data["tree"]["secrets"]["hidden"] = True
        data["tree"]["Cargo.toml"]["hidden"] = True
        tool.write_data(data)
        rendered = tool.render_brief_tree()
        self.assertNotIn("secrets", rendered)  # 条目及子树整体消失
        self.assertNotIn("token.rs", rendered)
        self.assertNotIn("Cargo.toml", rendered)
        self.assertIn("apps/", rendered)  # 其余照常渲染

    def test_hidden_entries_survive_in_data_and_check(self):
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml", "build/out.exe"})
        tool.add("build", desc="构建产物", is_dir_entry=True)
        tool.add("build/out.exe", desc="产物", detail=["完整描述"])
        data = tool.load()
        data["tree"]["build"]["collapsed"] = True
        data["tree"]["Cargo.toml"]["hidden"] = True
        tool.write_data(data)
        tool.render()
        # 隐藏/折叠 ≠ 删除：数据完整性、磁盘对照、产物一致性照常
        errors, warnings = tool.check()
        self.assertEqual((errors, warnings), ([], []))
        self.assertEqual([p for p, _ in tool.query(kw="根配置")], ["Cargo.toml"])  # 查询不受 hidden 影响
        agents = tool.agents_md.read_text(encoding="utf-8")
        self.assertNotIn("Cargo.toml", agents)

    def test_add_render_flags_and_upsert(self):
        tool = self.make_tool()
        tool.add("dist", desc="发布", is_dir_entry=True, collapsed=True)
        tool.add("Cargo.toml", desc="根配置", hidden=True)
        self.assertIs(tool.get("dist")["collapsed"], True)
        self.assertIs(tool.get("Cargo.toml")["hidden"], True)
        tool.add("Cargo.toml", desc="新描述")  # 未指定的标志保留
        self.assertIs(tool.get("Cargo.toml")["hidden"], True)
        tool.add("Cargo.toml", desc="新描述", hidden=False)  # 显式 false 撤销
        self.assertNotIn("hidden", tool.get("Cargo.toml"))
        tool.add("dist", desc="发布", is_dir_entry=True, collapsed=False)
        self.assertNotIn("collapsed", tool.get("dist"))

    def test_collapsed_rejected_on_file_entry(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add("apps/x.rs", desc="x", collapsed=True)  # add 层拒绝
        data = tool.load()
        data["tree"]["Cargo.toml"]["collapsed"] = True
        with self.assertRaises(ToolError):
            tool.write_data(data)  # 数据层兜底拒绝


class ReplaceBlockTest(unittest.TestCase):
    def test_replace_middle(self):
        text = "a\n<!-- b:begin -->\nold\n<!-- b:end -->\nz"
        self.assertEqual(
            replace_block(text, "<!-- b:begin -->", "<!-- b:end -->", "new1\nnew2"),
            "a\n<!-- b:begin -->\nnew1\nnew2\n<!-- b:end -->\nz",
        )

    def test_missing_marker_raises(self):
        with self.assertRaises(ToolError):
            replace_block("nothing", "<!-- b:begin -->", "<!-- b:end -->", "x")


class CheckTest(SandboxTest):
    def test_clean_after_render(self):
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml"})
        tool.render()
        errors, warnings = tool.check()
        self.assertEqual((errors, warnings), ([], []))

    def test_unknown_field(self):
        tool = self.make_tool()
        data = tool.load()
        data["tree"]["Cargo.toml"]["foo"] = 1
        tool.write_data(data)
        errors, _ = tool.check()
        self.assertTrue(any("foo" in e for e in errors))

    def test_tag_outside_vocab(self):
        tool = self.make_tool()
        data = tool.load()
        data["tree"]["Cargo.toml"]["tags"] = ["nope"]
        tool.write_data(data)  # 规范化写入保留未知 tag，由 check 语义层报错
        errors, _ = tool.check()
        self.assertTrue(any("nope" in e for e in errors))

    def test_dangling_and_self_rel(self):
        tool = self.make_tool()
        data = tool.load()
        data["tree"]["Cargo.toml"]["rel"] = ["apps/nope.ts"]
        tool.write_data(data)
        errors, _ = tool.check()
        self.assertTrue(any("apps/nope.ts" in e for e in errors))
        data["tree"]["Cargo.toml"]["rel"] = ["Cargo.toml"]
        tool.write_data(data)
        errors, _ = tool.check()
        self.assertTrue(any("自身" in e for e in errors))

    def test_noncanonical_bytes_detected(self):
        tool = self.make_tool()
        tool.render()
        data = tool.load()
        # 手改格式层（4 空格缩进），内容不变 → 规范形态检测应报错
        tool.tree_json.write_text(
            json.dumps(data, ensure_ascii=False, indent=4) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        errors, _ = tool.check()
        self.assertTrue(any("规范" in e for e in errors))

    def test_crlf_tolerated_but_rewritten_on_next_write(self):
        tool = self.make_tool()
        tool.render()
        raw = tool.tree_json.read_text(encoding="utf-8")
        tool.tree_json.write_text(raw.replace("\n", "\r\n"), encoding="utf-8", newline="")
        errors, _ = tool.check()
        self.assertEqual(errors, [])

    def test_stale_render_detected(self):
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml", "apps/x.rs"})
        tool.render()
        tool.add("apps/x.rs", desc="后加的", detail=["完整描述"])  # 只写数据不渲染
        errors, _ = tool.check()
        self.assertTrue(any("产物" in e for e in errors))
        tool.render()
        errors, warnings = tool.check()
        self.assertEqual((errors, warnings), ([], []))

    def test_git_compare_rules(self):
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml", "docs/x.md", "README.md"})
        tool.add("docs", desc="文档", is_dir_entry=True)
        tool.add("gone.rs", desc="已删除")
        tool.render()
        errors, warnings = tool.check()
        # gone.rs 在树不在 git → 错误
        self.assertTrue(any("gone.rs" in e for e in errors))
        joined_warnings = "\n".join(warnings)
        # apps 展开收录 → 漏掉的顶层 README.md 报未收录告警
        self.assertIn("README.md", joined_warnings)
        # docs 整目录收录 → 其下文件不告警
        self.assertNotIn("docs/x.md", joined_warnings)

    def test_desc_warnings_and_strict(self):
        tool = self.make_tool()
        tool.add("apps/x.rs", desc="这是一个超过二十个字符的超长描述用于触发告警", detail=["完整描述"])
        tool.add("apps/parent", desc="", is_dir_entry=True)
        tool.render()
        errors, warnings = tool.check()
        self.assertEqual(errors, [])
        self.assertEqual(len(warnings), 2)
        self.assertTrue(any("超长" in w for w in warnings))
        errors, _ = tool.check(strict=True)
        self.assertEqual(len(errors), 2)

    def test_field_completeness_detail(self):
        tool = self.make_tool()
        tool.add("apps/bare.rs", desc="只有一句话")  # 文件条目缺 detail
        tool.render()
        errors, warnings = tool.check()
        self.assertEqual(errors, [])
        # 仅文件条目报缺 detail；目录（apps/）一句话 desc 即完整，不告警
        self.assertEqual(
            [w for w in warnings if "缺 detail" in w],
            ["W: apps/bare.rs 缺 detail（完整描述待补，详版树将回退 desc）"],
        )

    def test_skill_pycache_exempt_from_git_compare(self):
        # 技能自身测试产生的 __pycache__ 不报"未收录"（运行时缓存，非仓库内容）
        tool = self.make_tool(git_files={
            "apps/main.tsx", "apps/util.ts", "Cargo.toml",
            ".agents/skills/file-tree/scripts/__pycache__/tree_tool.cpython-314.pyc",
        })
        tool.render()
        errors, warnings = tool.check()
        self.assertEqual((errors, warnings), ([], []))

    def test_other_pycache_still_reported(self):
        # 技能目录之外的 __pycache__ 是仓库卫生问题，照常告警
        tool = self.make_tool(git_files={
            "apps/main.tsx", "apps/util.ts", "Cargo.toml",
            "vendor/__pycache__/x.cpython-314.pyc",
        })
        tool.render()
        _, warnings = tool.check()
        self.assertTrue(any("vendor/__pycache__/x.cpython-314.pyc" in w for w in warnings))

    def test_git_compare_reports_disk_dir_as_type_mismatch(self):
        # 目录被录成文件条目：git ls-files 只列文件不列目录，磁盘实况是目录 →
        # 报类型错配并给修正指引，不再误报"磁盘不存在"把人带向根定位歧途
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml"})
        tool.add("testdata", desc="测试数据")  # 磁盘尚无 → 文件条目
        tool.repo_root.joinpath("testdata").mkdir()  # 磁盘后出现目录（模拟存量错配）
        tool.render()
        errors, _ = tool.check()
        self.assertTrue(any("testdata" in e and "目录" in e for e in errors))
        self.assertFalse(any("磁盘不存在" in e for e in errors))

    def test_git_compare_untracked_file_wording(self):
        # 磁盘上存在的文件未被 git 跟踪：只报 git 未跟踪，不叠加"磁盘不存在"的矛盾表述
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml"})
        tool.repo_root.joinpath("ignored.rs").write_text("x", encoding="utf-8")
        tool.add("ignored.rs", desc="未跟踪")  # 磁盘是文件，不影响自动识别
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(
            [e for e in errors if "ignored.rs" in e],
            ["E: 树中条目未被 git 跟踪: ignored.rs"],
        )


class GitIgnoreTest(SandboxTest):
    """git-ignore 校验控制字段：豁免"必须被 git 跟踪"的对照，check 只看磁盘存在与 git 排除态。

    注入口语义：git_files = tracked ∪ untracked-unignored（被 .gitignore 忽略的文件不在其中，
    同 git ls-files --cached --others --exclude-standard）；tracked_files = --cached 集合。
    """

    def write_disk(self, tool: TreeTool, rel: str, content: str = "x") -> None:
        path = tool.repo_root.joinpath(*split_rel_path(rel))
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    def test_add_and_upsert_git_ignore(self):
        tool = self.make_tool()
        tool.add("data.bin", desc="大文件", detail=["完整描述"], git_ignore=True)
        self.assertIs(tool.get("data.bin")["git-ignore"], True)
        tool.add("data.bin", desc="大文件")  # 未指定的标志保留
        self.assertIs(tool.get("data.bin")["git-ignore"], True)
        # 显式 false 落盘（覆写祖先豁免用），与 collapsed/hidden 的"清除"语义不同
        tool.add("data.bin", desc="大文件", git_ignore=False)
        self.assertIs(tool.get("data.bin")["git-ignore"], False)

    def test_add_batch_git_ignore_field(self):
        tool = self.make_tool(data={"tags": {}, "tree": {}})
        entries = [
            {"path": "data.bin", "desc": "大文件", "git-ignore": True},
            {"path": "pkg.zip", "desc": "离线包", "git-ignore": False},
        ]
        tool.add_batch(entries)
        self.assertIs(tool.get("data.bin")["git-ignore"], True)
        self.assertIs(tool.get("pkg.zip")["git-ignore"], False)  # 显式 false 同样落盘

    def test_check_passes_when_ignored_on_disk(self):
        # 磁盘存在 + 不在 git_files（被 .gitignore 忽略）→ 通过，不报"未被 git 跟踪"
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml"})
        self.write_disk(tool, "data.bin")
        tool.add("data.bin", desc="大文件", detail=["完整描述"], git_ignore=True)
        tool.render()
        self.assertEqual(tool.check(), ([], []))

    def test_check_reports_missing_disk(self):
        # 豁免只豁免 git 对照，不豁免磁盘存在性
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml"})
        tool.add("data.bin", desc="大文件", detail=["完整描述"], git_ignore=True)
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(
            [e for e in errors if "data.bin" in e],
            ["E: data.bin git-ignore 条目磁盘不存在"],
        )

    def test_check_reports_tracked_contradiction(self):
        # 标记 git-ignore 但实际被 git 跟踪：矛盾态（tracked ⊆ git_files，差集循环不可见，须单独拦截）
        base = {"apps/main.tsx", "apps/util.ts", "Cargo.toml"}
        tool = self.make_tool(git_files=base | {"data.bin"}, tracked_files=base | {"data.bin"})
        self.write_disk(tool, "data.bin")
        tool.add("data.bin", desc="大文件", detail=["完整描述"], git_ignore=True)
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(
            [e for e in errors if "data.bin" in e],
            ["E: data.bin 标记 git-ignore 但实际被 git 跟踪（git rm --cached 或移除标记恢复对照）"],
        )

    def test_check_reports_unignored_untracked(self):
        # 磁盘存在、未跟踪、但 .gitignore 没覆盖（git status 会持续显示 untracked）→ 错误
        base = {"apps/main.tsx", "apps/util.ts", "Cargo.toml"}
        tool = self.make_tool(git_files=base | {"data.bin"}, tracked_files=base)
        self.write_disk(tool, "data.bin")
        tool.add("data.bin", desc="大文件", detail=["完整描述"], git_ignore=True)
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(
            [e for e in errors if "data.bin" in e],
            ["E: data.bin 标记 git-ignore 但未被 .gitignore 排除（补 ignore 规则或移除标记）"],
        )

    def test_dir_git_ignore_exempts_subtree(self):
        # 目录标记 → 子树文件条目继承豁免；子树外条目照旧对照
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml"})
        self.write_disk(tool, "datasets/a.bin")
        self.write_disk(tool, "apps/gone.tsx")
        tool.add("datasets", desc="数据集", is_dir_entry=True, git_ignore=True)
        tool.add("datasets/a.bin", desc="数据", detail=["完整描述"])
        tool.add("apps/gone.tsx", desc="未豁免", detail=["完整描述"])
        tool.render()
        errors, _ = tool.check()
        self.assertFalse(any("datasets" in e for e in errors), errors)  # 子树豁免生效
        self.assertTrue(any("apps/gone.tsx" in e and "未被 git 跟踪" in e for e in errors), errors)

    def test_explicit_false_overrides_ancestor(self):
        # .gitignore ! 规则场景：目录整体豁免但个别子文件走 git（tracked）。
        # 继承会把 tracked 子文件卷进豁免集合 → 误报矛盾；显式 false 就近覆写后恢复正常对照
        base = {"apps/main.tsx", "apps/util.ts", "Cargo.toml"}
        tool = self.make_tool(git_files=base | {"datasets/README.md"},
                              tracked_files=base | {"datasets/README.md"})
        self.write_disk(tool, "datasets/README.md")
        tool.add("datasets", desc="数据集", is_dir_entry=True, git_ignore=True)
        tool.add("datasets/README.md", desc="说明", detail=["完整描述"])  # 无显式设置 → 继承 true
        tool.render()
        errors, _ = tool.check()
        self.assertTrue(any("README.md" in e and "被 git 跟踪" in e for e in errors), errors)  # 缺口基准：误报
        tool.add("datasets/README.md", desc="说明", git_ignore=False)  # 显式 false 覆写
        tool.render()
        errors, _ = tool.check()
        self.assertFalse(any("README.md" in e for e in errors), errors)  # 误报消除

    def test_false_inherits_down_subtree(self):
        # 就近覆写向下传递：爷 true + 中间目录 false + 孙无显式设置 → 孙不豁免
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml"})
        self.write_disk(tool, "datasets/sub/x.bin")
        tool.add("datasets", desc="数据集", is_dir_entry=True, git_ignore=True)
        tool.add("datasets/sub", desc="例外子集", is_dir_entry=True, git_ignore=False)
        tool.add("datasets/sub/x.bin", desc="数据", detail=["完整描述"])
        tool.render()
        errors, _ = tool.check()
        self.assertTrue(any("x.bin" in e and "未被 git 跟踪" in e for e in errors), errors)

    def test_git_ignore_keeps_render_and_query(self):
        # 校验控制字段不影响渲染：简版树照常显示；get / query --json 可见
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml"})
        self.write_disk(tool, "data.bin")
        tool.add("data.bin", desc="大文件", detail=["完整描述"], git_ignore=True)
        tool.render()
        self.assertIn("data.bin", tool.render_brief_tree())
        self.assertIs(tool.get("data.bin")["git-ignore"], True)
        import contextlib
        import io
        import types

        args = types.SimpleNamespace(kw="data.bin", tag=None, rel_of=None, under=None, depth=None, json=True)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            _cmd_query(tool, args)
        payload = json.loads(buf.getvalue())
        self.assertIs(payload[0]["git-ignore"], True)

    def test_check_exempt_disk_dir_type_mismatch(self):
        # 豁免条目磁盘上是目录：报类型错配并给修正指引，不误诊为"磁盘不存在"（与非豁免分支对称）。
        # 先 add 后建目录（模拟存量错配）：先建目录会被 add 自动识别为目录条目，走不进豁免对照
        tool = self.make_tool(git_files={"apps/main.tsx", "apps/util.ts", "Cargo.toml"})
        tool.add("legacy.bin", desc="遗留大文件", detail=["完整描述"], git_ignore=True)
        tool.repo_root.joinpath("legacy.bin").mkdir()
        tool.render()
        errors, _ = tool.check()
        self.assertTrue(any("legacy.bin" in e and "磁盘上是目录" in e for e in errors), errors)
        self.assertFalse(any("磁盘不存在" in e for e in errors), errors)


class MarkTest(SandboxTest):
    """mark 子树批量标记：tags 追加/覆写、git-ignore 传播（仅文件条目）、depth 限制、单步历史。

    夹具 docs 子树：a.md(深1)、sub/(深1, 目录)、b.md(深2)、deep/(深2, 目录)、c.md(深3)——
    文件 3 / 目录 2 / 共 5 条；docs 自身与 Cargo.toml 在作用域外。
    """

    def make_mark_tool(self, git_files: set[str] | None = None, tracked_files: set[str] | None = None) -> TreeTool:
        return self.make_tool(
            data={
            "tags": {"doc": "文档", "big": "大文件"},
            "tree": {
                "docs": {"desc": "文档", "children": {
                    "a.md": {"desc": "a", "detail": ["x"]},
                    "sub": {"desc": "子目录", "children": {
                        "b.md": {"desc": "b", "detail": ["x"]},
                        "deep": {"desc": "更深层", "children": {
                            "c.md": {"desc": "c", "detail": ["x"]},
                        }},
                    }},
                }},
                "Cargo.toml": {"desc": "根配置", "detail": ["x"]},
            },
        }, git_files=git_files, tracked_files=tracked_files)

    def test_tags_add_union(self):
        tool = self.make_mark_tool()
        n_tags, n_git, _ = tool.mark("docs", tags=["doc"])
        self.assertEqual((n_tags, n_git), (5, 0))  # 子树全部条目（含目录）
        for path in ["docs/a.md", "docs/sub", "docs/sub/b.md", "docs/sub/deep", "docs/sub/deep/c.md"]:
            self.assertEqual(tool.get(path).get("tags"), ["doc"], path)
        n_tags, _, _ = tool.mark("docs", tags=["big"])  # 并集追加
        self.assertEqual(n_tags, 5)
        self.assertEqual(tool.get("docs/a.md")["tags"], ["big", "doc"])  # 规范化排序
        n_tags, _, _ = tool.mark("docs", tags=["doc"])  # 无新值 → 不计受影响
        self.assertEqual(n_tags, 0)

    def test_tags_replace_and_clear(self):
        tool = self.make_mark_tool()
        tool.mark("docs", tags=["doc"])
        n_tags, _, _ = tool.mark("docs", tags=["big"], tags_mode="replace")
        self.assertEqual(n_tags, 5)
        self.assertEqual(tool.get("docs/a.md")["tags"], ["big"])
        n_tags, _, _ = tool.mark("docs", tags=[], tags_mode="replace")  # 空列表 = 清空
        self.assertEqual(n_tags, 5)
        self.assertNotIn("tags", tool.get("docs/a.md"))
        n_tags, _, _ = tool.mark("docs", tags=[], tags_mode="replace")  # 已清空 → 不计
        self.assertEqual(n_tags, 0)

    def test_scope_excludes_target_and_outside(self):
        tool = self.make_mark_tool()
        tool.mark("docs", tags=["doc"])
        self.assertNotIn("tags", tool.get("docs"))  # 传播不含目标目录自身
        self.assertNotIn("tags", tool.get("Cargo.toml"))  # 子树外不受影响

    def test_depth_limit(self):
        tool = self.make_mark_tool()
        n_tags, n_git, _ = tool.mark("docs", tags=["doc"], git_ignore=True, depth=1)
        self.assertEqual(n_tags, 2)  # 仅深度 1：a.md、sub
        self.assertEqual(n_git, 1)  # 深度 1 的文件只有 a.md（git-ignore 不落目录，防继承穿透 depth）
        self.assertEqual(tool.get("docs/a.md")["tags"], ["doc"])
        self.assertEqual(tool.get("docs/a.md")["git-ignore"], True)
        self.assertNotIn("tags", tool.get("docs/sub/b.md"))
        self.assertNotIn("git-ignore", tool.get("docs/sub/b.md"))
        self.assertNotIn("git-ignore", tool.get("docs/sub"))  # 目录不落标记

    def test_git_ignore_files_only_and_check(self):
        # mark 传播豁免后 check 联动：文件豁免生效零错误、目录条目不落标记
        tool = self.make_mark_tool(git_files={"Cargo.toml"})
        for rel in ["docs/a.md", "docs/sub/b.md", "docs/sub/deep/c.md"]:  # 磁盘就位，不在 git_files = 模拟被忽略
            disk = tool.repo_root.joinpath(*split_rel_path(rel))
            disk.parent.mkdir(parents=True, exist_ok=True)
            disk.write_text("x", encoding="utf-8")
        _, n_git, n_skip = tool.mark("docs", git_ignore=True)
        self.assertEqual((n_git, n_skip), (3, 0))
        for path in ["docs/sub", "docs/sub/deep"]:
            self.assertNotIn("git-ignore", tool.get(path))  # 目录条目不落标记
        tool.render()
        self.assertEqual(tool.check(), ([], []))

    def test_git_ignore_false_overwrite(self):
        # false 传播作用于缺省态文件（批量退出豁免）；显式设置不被批量覆写
        tool = self.make_mark_tool()
        _, n_git, _ = tool.mark("docs/sub", git_ignore=False)
        self.assertEqual(n_git, 2)  # b.md、c.md 落显式 false（就近覆写三态）
        self.assertIs(tool.get("docs/sub/b.md")["git-ignore"], False)
        # 个体表态优先：显式 false 的条目不被后续 true 传播覆写
        _, n_git, _ = tool.mark("docs", git_ignore=True)
        self.assertEqual(n_git, 1)  # 仅缺省态的 a.md 落 true
        self.assertIs(tool.get("docs/a.md")["git-ignore"], True)
        self.assertIs(tool.get("docs/sub/b.md")["git-ignore"], False)

    def test_git_ignore_true_skips_tracked(self):
        # true 传播跳过 git 已跟踪文件（落 true 即矛盾标记，check 必报错）
        tool = self.make_mark_tool(git_files={"Cargo.toml", "docs/a.md"},
                                   tracked_files={"Cargo.toml", "docs/a.md"})
        _, n_git, n_skip = tool.mark("docs", git_ignore=True)
        self.assertEqual((n_git, n_skip), (2, 1))  # b.md、c.md 落 true；a.md 跳过
        self.assertNotIn("git-ignore", tool.get("docs/a.md"))

    def test_git_ignore_false_covers_tracked(self):
        # false 传播不跳 tracked：tracked 文件落显式 false 恰是"退出祖先豁免"的修复动作
        tool = self.make_mark_tool(git_files={"Cargo.toml", "docs/a.md"},
                                   tracked_files={"Cargo.toml", "docs/a.md"})
        _, n_git, n_skip = tool.mark("docs", git_ignore=False)
        self.assertEqual((n_git, n_skip), (3, 0))
        self.assertIs(tool.get("docs/a.md")["git-ignore"], False)

    def test_errors(self):
        tool = self.make_mark_tool()
        with self.assertRaises(ToolError):  # 目录条目不存在
            tool.mark("nope", tags=["doc"])
        with self.assertRaises(ToolError):  # 文件条目不能作为锚点
            tool.mark("Cargo.toml", tags=["doc"])
        with self.assertRaises(ToolError):  # 粗粒度目录无 children，无可传播条目
            tool.add("assets", desc="图标集", is_dir_entry=True)
            tool.mark("assets", tags=["doc"])
        with self.assertRaises(ToolError):  # 无动作参数
            tool.mark("docs")
        with self.assertRaises(ToolError):  # 未知标签
            tool.mark("docs", tags=["nope"])
        with self.assertRaises(ToolError):  # depth 正整数
            tool.mark("docs", tags=["doc"], depth=0)
        # 前置校验失败保持原子：无半落盘（含 assets 建链后的基线）
        before = tool.tree_json.read_bytes()
        with self.assertRaises(ToolError):
            tool.mark("docs", tags=["nope"])
        self.assertEqual(tool.tree_json.read_bytes(), before)

    def test_undo_single_step(self):
        tool = self.make_mark_tool()
        tool.mark("docs", tags=["doc"])
        tool.undo()
        for path in ["docs/a.md", "docs/sub", "docs/sub/b.md", "docs/sub/deep", "docs/sub/deep/c.md"]:
            self.assertNotIn("tags", tool.get(path), path)  # 一次 mark = 一步历史，undo 整体回滚

    def test_redo_roundtrip(self):
        tool = self.make_mark_tool()
        tool.mark("docs", tags=["doc"])
        tool.undo()
        tool.redo()
        self.assertEqual(tool.get("docs/a.md")["tags"], ["doc"])  # redo 完整恢复子树标记


class CmdMarkTest(SandboxTest):
    """CLI 层 _cmd_mark 的参数解析契约：--tags 空串/缺省区分、拆分去空白、输出文案。"""

    def run_cmd_mark(self, tool, path="apps", tags=None, tags_mode="add", git_ignore=None, depth=None):
        import types

        args = types.SimpleNamespace(path=path, tags=tags, tags_mode=tags_mode, git_ignore=git_ignore, depth=depth)
        _cmd_mark(tool, args)

    def test_tags_empty_string_means_clear(self):
        # --tags "" 是显式空列表（配 replace 清空），不折算为 None——CLI 解析独立于方法层，须单独锁定
        tool = self.make_tool()
        tool.mark("apps", tags=["pure"])
        self.run_cmd_mark(tool, tags="", tags_mode="replace")
        self.assertNotIn("tags", tool.get("apps/main.tsx"))

    def test_tags_none_keeps_field(self):
        tool = self.make_tool()
        tool.mark("apps", tags=["pure"])
        self.run_cmd_mark(tool, tags=None, git_ignore=False)
        self.assertEqual(tool.get("apps/main.tsx")["tags"], ["pure"])  # 缺省不动 tags
        self.assertIs(tool.get("apps/main.tsx")["git-ignore"], False)  # false 传播照常

    def test_tags_split_and_trim(self):
        tool = self.make_tool()
        self.run_cmd_mark(tool, tags=" pure , test ")
        self.assertEqual(tool.get("apps/main.tsx")["tags"], ["pure", "test"])

    def test_output_skip_note_by_direction(self):
        import contextlib
        import io

        tool = self.make_tool()
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            self.run_cmd_mark(tool, tags="pure")
        self.assertNotIn("跳过", buf.getvalue())  # 无跳过不显示
        tool.add("apps/main.tsx", desc="入口", git_ignore=False)  # 制造一条显式设置
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            self.run_cmd_mark(tool, git_ignore=True)
        self.assertIn("跳过 1 条（显式设置/git 已跟踪不覆写）", buf.getvalue())  # true 方向文案
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            self.run_cmd_mark(tool, git_ignore=False)
        self.assertIn("跳过 2 条（显式设置不覆写）", buf.getvalue())  # false 方向：两条显式设置都跳过


class GitDirTest(unittest.TestCase):
    """git 私有区识别：以 <gitdir>/HEAD 为准，绝不创建 .git。"""

    def test_none_without_dotgit(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.assertIsNone(resolve_git_dir(root))

    def test_rejects_empty_dotgit_dir(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".git").mkdir()  # 无效仓库：空 .git
            self.assertIsNone(resolve_git_dir(root))
            self.assertEqual(default_history_path(root, root / "skill"), root / "skill" / ".history.json")

    def test_accepts_dir_with_head(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".git").mkdir()
            (root / ".git" / "HEAD").write_text("ref: refs/heads/main\n", encoding="utf-8")
            self.assertEqual(resolve_git_dir(root), root / ".git")
            self.assertEqual(
                default_history_path(root, root / "skill"),
                root / ".git" / "file-tree" / "history.json",
            )

    def test_accepts_worktree_pointer(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            real = root / "realgit"
            real.mkdir()
            (real / "HEAD").write_text("ref: refs/heads/feat\n", encoding="utf-8")
            (root / ".git").write_text(f"gitdir: {real.as_posix()}\n", encoding="utf-8")
            self.assertEqual(resolve_git_dir(root), real)

    def test_history_writing_never_creates_dotgit(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            skill = root / "skill"
            skill.mkdir()
            tool = TreeTool(
                tree_json=skill / "tree.json",
                agents_md=skill / "AGENTS.md",
                repo_root=root,
                root_name="Demo",
                history_path=default_history_path(root, skill),
            )
            tool.write_data({"tags": {}, "tree": {"a.rs": {"desc": "a", "detail": ["a"]}}})
            tool.agents_md.write_text("# AGENTS\n", encoding="utf-8", newline="\n")
            tool.add("b.rs", desc="b", detail=["b"])
            self.assertFalse((root / ".git").exists())  # 不凭空创建 .git
            self.assertTrue((skill / ".history.json").exists())  # 退化路径生效


class UndoRedoTest(SandboxTest):
    def test_undo_restores_previous_state(self):
        tool = self.make_tool()
        tool.add("apps/new.rs", desc="新增", detail=["描述"])
        self.assertIn("new.rs", tool.get("apps")["children"])
        op = tool.undo()
        self.assertEqual(op, "add apps/new.rs")
        self.assertNotIn("new.rs", tool.load()["tree"]["apps"]["children"])
        # 恢复后产物同步、check 干净
        self.assertEqual(tool.check()[0], [])
        self.assertIn("# 入口", tool.agents_md.read_text(encoding="utf-8"))

    def test_redo_roundtrip(self):
        tool = self.make_tool()
        tool.rm("apps/util.ts")
        self.assertNotIn("util.ts", tool.get("apps")["children"])
        tool.undo()
        op = tool.redo()
        self.assertEqual(op, "rm apps/util.ts")
        self.assertNotIn("util.ts", tool.get("apps")["children"])

    def test_undo_empty_raises(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.undo()
        with self.assertRaises(ToolError):
            tool.redo()

    def test_new_op_truncates_redo_branch(self):
        tool = self.make_tool()
        tool.add("apps/a.rs", desc="a", detail=["a"])
        tool.undo()
        tool.add("apps/b.rs", desc="b", detail=["b"])  # 新操作截断 redo 分支
        with self.assertRaises(ToolError):
            tool.redo()

    def test_history_limit_drops_oldest(self):
        tool = self.make_tool(history_limit=2)
        for name in ("a.rs", "b.rs", "c.rs"):
            tool.add(name, desc=name, detail=[name])
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, ["add a.rs", "add b.rs", "add c.rs"][-2:])
        tool.undo()  # 撤销 add c.rs
        tool.undo()  # 撤销 add b.rs
        with self.assertRaises(ToolError):  # a.rs 的快照已被丢弃
            tool.undo()

    def test_validation_failure_leaves_no_history(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add("bad/path/../x.rs", desc="x")  # 校验失败
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, [])


class HistoryMigrationTest(SandboxTest):
    """git 初始化晚于技能使用：旧位置历史自动收敛进 git 私有区，undo 栈不断裂。"""

    def _simulate_git_init(self, tool: TreeTool) -> Path:
        legacy = tool.history_path
        canonical = tool.repo_root / ".git" / "file-tree" / "history.json"
        tool.history_path = canonical
        tool.legacy_history_paths = (legacy,)
        return legacy

    def test_migrates_legacy_history_into_gitdir(self):
        tool = self.make_tool()
        legacy = tool.history_path
        tool.add("apps/first.rs", desc="一", detail=["一"])  # git init 前：历史落在技能目录
        self.assertTrue(legacy.exists())
        self._simulate_git_init(tool)
        # 旧历史仍可读（含迁移前的撤销栈）
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, ["add apps/first.rs"])
        # 下一次写操作完成收敛：栈延续、旧文件删除
        tool.add("apps/second.rs", desc="二", detail=["二"])
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, ["add apps/first.rs", "add apps/second.rs"])
        canonical = tool.history_path
        self.assertTrue(canonical.exists())
        self.assertFalse(legacy.exists())

    def test_undo_reads_legacy_and_converges(self):
        tool = self.make_tool()
        tool.add("apps/old.rs", desc="旧", detail=["旧"])
        self._simulate_git_init(tool)
        op = tool.undo()  # 直接 undo：读旧位置历史，恢复后写 canonical
        self.assertEqual(op, "add apps/old.rs")
        self.assertNotIn("old.rs", tool.load()["tree"]["apps"]["children"])
        self.assertFalse(self.legacy_exists(tool))
        op = tool.redo()  # redo 栈同样延续
        self.assertEqual(op, "add apps/old.rs")

    def legacy_exists(self, tool: TreeTool) -> bool:
        return any(p.exists() for p in tool.legacy_history_paths if p != tool.history_path)

    def test_check_warns_on_pending_migration(self):
        tool = self.make_tool()
        tool.add("apps/x.rs", desc="x", detail=["x"])
        tool.render()
        self._simulate_git_init(tool)
        errors, warnings = tool.check()
        self.assertEqual(errors, [])
        self.assertTrue(any("旧位置" in w for w in warnings))


class QueryTest(SandboxTest):
    def test_filters(self):
        tool = self.make_tool()
        tool.add("apps/render.rs", desc="渲染纯函数", tags=["pure"])
        paths = [p for p, _ in tool.query(kw="渲染")]
        self.assertEqual(paths, ["apps/render.rs"])
        paths = [p for p, _ in tool.query(tag="pure")]
        self.assertEqual(paths, ["apps/render.rs", "apps/util.ts"])
        # 反查：谁关联到 Cargo.toml
        tool.add("apps/main.tsx", rel=["Cargo.toml"])
        paths = [p for p, _ in tool.query(rel_of="Cargo.toml")]
        self.assertEqual(paths, ["apps/main.tsx"])

    def test_under_filters_subtree_and_includes_self(self):
        # --under 锚点自身也算"这块"的条目；子树全量、子树外排除
        tool = self.make_tool()
        tool.add("apps/render.rs", desc="渲染", detail=["x"])
        paths = [p for p, _ in tool.query(under="apps")]
        self.assertEqual(paths, ["apps", "apps/main.tsx", "apps/render.rs", "apps/util.ts"])

    def test_under_depth(self):
        tool = self.make_tool()
        tool.add("apps/ui", desc="UI 层", is_dir_entry=True)
        tool.add("apps/ui/button.tsx", desc="按钮", detail=["x"])
        # depth=1：锚点自身（相对深度 0）+ 直接子级
        paths = [p for p, _ in tool.query(under="apps", depth=1)]
        self.assertEqual(paths, ["apps", "apps/main.tsx", "apps/ui", "apps/util.ts"])

    def test_under_combines_with_other_filters(self):
        tool = self.make_tool()
        paths = [p for p, _ in tool.query(under="apps", tag="pure")]
        self.assertEqual(paths, ["apps/util.ts"])

    def test_under_and_depth_errors(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):  # --under 必须是树中目录条目
            tool.query(under="apps/main.tsx")
        with self.assertRaises(ToolError):
            tool.query(under="nope")
        with self.assertRaises(ToolError):  # depth 须与 under 同用
            tool.query(depth=1)
        with self.assertRaises(ToolError):  # depth 正整数
            tool.query(under="apps", depth=0)

    def test_get_missing_raises(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.get("nope.rs")

    def test_get_multiple_paths(self):
        # get 多路径批量查看：逐条输出、条间空行分隔；单路径输出与旧格式一致
        import contextlib
        import io
        import types

        tool = self.make_tool()
        args = types.SimpleNamespace(path=["Cargo.toml", "apps/util.ts"])
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            _cmd_get(tool, args)
        out = buf.getvalue()
        self.assertIn("Cargo.toml", out)
        self.assertIn("apps/util.ts", out)
        self.assertIn("类型: 文件", out)
        self.assertTrue(out.index("Cargo.toml") < out.index("apps/util.ts"))
        args = types.SimpleNamespace(path=["Cargo.toml"])
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            _cmd_get(tool, args)
        self.assertFalse(buf.getvalue().startswith("\n"))  # 单路径无前导空行


class AddBatchTest(SandboxTest):
    """add-batch：批量 upsert 一次变更一步历史，任一条非法整批拒绝，不变量校验照常。"""

    def entries_basic(self) -> list[dict]:
        return [
            {"path": "apps/new.ts", "desc": "新页面", "detail": ["路由与视图"], "tags": ["pure"]},
            {"path": "docs/guide.md", "desc": "指南"},
            {"path": "lib.rs", "desc": "库根", "detail": ["公共 API"]},
        ]

    def test_writes_all_entries_with_auto_parents(self):
        tool = self.make_tool()
        n = tool.add_batch(self.entries_basic())
        self.assertEqual(n, 3)
        node = tool.get("apps/new.ts")
        self.assertEqual(node["desc"], "新页面")
        self.assertEqual(node["tags"], ["pure"])
        self.assertEqual(tool.get("docs/guide.md")["desc"], "指南")
        self.assertEqual(tool.get("lib.rs")["detail"], ["公共 API"])
        self.assertIn("guide.md", tool.load()["tree"]["docs"]["children"])

    def test_single_history_step_undo_rolls_back_all(self):
        tool = self.make_tool()
        tool.add_batch(self.entries_basic())
        undo, redo = tool.history_summary()
        self.assertEqual(len(undo), 1)
        self.assertIn("add-batch", undo[0])
        self.assertEqual(redo, [])
        tool.undo()
        data = tool.load()
        self.assertNotIn("lib.rs", data["tree"])
        self.assertNotIn("docs", data["tree"])
        self.assertNotIn("new.ts", data["tree"]["apps"]["children"])

    def test_upsert_keeps_untouched_fields(self):
        tool = self.make_tool()
        tool.add_batch([{"path": "apps/util.ts", "desc": "工具集"}])
        node = tool.get("apps/util.ts")
        self.assertEqual(node["desc"], "工具集")
        self.assertEqual(node["detail"], ["纯函数工具集"])
        self.assertEqual(node["tags"], ["pure"])

    def test_internal_rel_between_batch_entries(self):
        tool = self.make_tool()
        tool.add_batch([
            {"path": "apps/a.ts", "desc": "甲", "rel": ["apps/b.ts"]},
            {"path": "apps/b.ts", "desc": "乙", "rel": ["apps/a.ts"]},
        ])
        self.assertEqual(tool.get("apps/a.ts")["rel"], ["apps/b.ts"])
        self.assertEqual(tool.get("apps/b.ts")["rel"], ["apps/a.ts"])

    def test_atomic_reject_unknown_tag(self):
        tool = self.make_tool()
        before = tool.tree_json.read_text(encoding="utf-8")
        with self.assertRaises(ToolError):
            tool.add_batch([
                {"path": "apps/ok.ts", "desc": "没问题"},
                {"path": "apps/bad.ts", "desc": "坏标签", "tags": ["ghost"]},
            ])
        self.assertEqual(tool.tree_json.read_text(encoding="utf-8"), before)
        undo, _ = tool.history_summary()
        self.assertEqual(undo, [])

    def test_atomic_reject_mid_path_conflict(self):
        tool = self.make_tool()
        before = tool.tree_json.read_text(encoding="utf-8")
        with self.assertRaises(ToolError):
            tool.add_batch([
                {"path": "apps/x.ts", "desc": "文件"},
                {"path": "apps/x.ts/child.rs", "desc": "路径中段冲突"},
            ])
        self.assertEqual(tool.tree_json.read_text(encoding="utf-8"), before)

    def test_atomic_reject_rel_missing_target(self):
        tool = self.make_tool()
        before = tool.tree_json.read_text(encoding="utf-8")
        with self.assertRaises(ToolError):
            tool.add_batch([{"path": "apps/c.ts", "desc": "丙", "rel": ["not/in/tree.rs"]}])
        self.assertEqual(tool.tree_json.read_text(encoding="utf-8"), before)

    def test_reject_rel_self_reference(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add_batch([{"path": "apps/d.ts", "desc": "丁", "rel": ["apps/d.ts"]}])

    def test_reject_duplicate_paths_in_batch(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add_batch([
                {"path": "apps/e.ts", "desc": "一"},
                {"path": "apps/e.ts", "desc": "二"},
            ])

    def test_reject_unknown_entry_field(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add_batch([{"path": "a.ts", "desc": "x", "typo_field": 1}])

    def test_reject_bad_field_types(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add_batch([{"path": 123, "desc": "x"}])
        with self.assertRaises(ToolError):
            tool.add_batch([{"path": "a.ts", "desc": "x", "detail": "不是数组"}])
        with self.assertRaises(ToolError):
            tool.add_batch([{"path": "a.ts", "desc": "x", "tags": ["pure", 1]}])
        with self.assertRaises(ToolError):
            tool.add_batch([{"path": "a.ts", "desc": "x", "collapsed": "yes"}])

    def test_dir_entry_with_collapsed(self):
        tool = self.make_tool()
        tool.add_batch([{"path": "assets/icons", "desc": "图标集", "dir": True, "collapsed": True}])
        node = tool.get("assets/icons")
        self.assertEqual(node["children"], {})
        self.assertTrue(node["collapsed"])

    def test_collapsed_on_file_rejected(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add_batch([{"path": "apps/f.ts", "desc": "x", "collapsed": True}])

    def test_empty_entries_rejected(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add_batch([])

    def test_reject_path_variant_duplicates(self):
        """反斜杠/双斜杠变体与正斜杠形式是同一路径，批内同现必须拒绝（判重按归一化路径）。"""
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add_batch([
                {"path": "src/w.ts", "desc": "一"},
                {"path": "src\\w.ts", "desc": "二"},
            ])

    def test_rel_normalized_to_forward_slashes_and_check_clean(self):
        """rel 非规范分隔符形式应规范为正斜杠落盘，check 的精确比较不再报 E。"""
        tool = self.make_tool()
        tool.add_batch([{"path": "apps/ref.ts", "desc": "引用", "rel": ["apps\\main.tsx"]}])
        self.assertEqual(tool.get("apps/ref.ts")["rel"], ["apps/main.tsx"])
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(errors, [])

    def test_empty_rel_rejected(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.add_batch([{"path": "a.ts", "desc": "x", "rel": [""]}])

    def test_redo_restores_whole_batch(self):
        tool = self.make_tool()
        tool.add_batch(self.entries_basic())
        tool.undo()
        op = tool.redo()
        self.assertIn("add-batch", op)
        data = tool.load()
        self.assertIn("lib.rs", data["tree"])
        self.assertIn("new.ts", data["tree"]["apps"]["children"])
        self.assertIn("guide.md", data["tree"]["docs"]["children"])

    def test_rel_to_auto_created_intermediate_dir(self):
        """rel 指向批内自动创建的中间目录：最终树中存在该节点即合法。"""
        tool = self.make_tool()
        tool.add_batch([{"path": "x/y/z.ts", "desc": "深层", "rel": ["x"]}])
        self.assertEqual(tool.get("x/y/z.ts")["rel"], ["x"])

    def test_null_switches_treated_as_absent(self):
        """dir/collapsed/hidden 显式 null 与缺省同义，不报类型错。"""
        tool = self.make_tool()
        tool.add_batch([{"path": "apps/n.ts", "desc": "x", "dir": None, "collapsed": None, "hidden": None}])
        self.assertNotIn("children", tool.get("apps/n.ts"))


class DiskDirAutoDetectTest(SandboxTest):
    """写入防呆：磁盘上是目录的路径未声明 dir 时自动收录为目录条目。

    目录路径录成文件条目没有任何合法存续场景（git ls-files 不列目录，check 必报错），
    自动识别消除"清单漏标 dir → check 报磁盘不存在 → 误诊根定位"的整条摩擦链。
    """

    def test_add_disk_dir_auto_recorded_as_dir(self):
        tool = self.make_tool()
        tool.repo_root.joinpath("testdata", "input").mkdir(parents=True)
        tool.add("testdata/input", desc="测试数据")  # 未声明 dir，磁盘是目录 → 自动识别
        node = tool.get("testdata/input")
        self.assertIn("children", node)  # 目录条目（children 空 = 整目录粗粒度收录）
        self.assertEqual(node["children"], {})

    def test_add_disk_file_stays_file(self):
        tool = self.make_tool()
        tool.repo_root.joinpath("real.rs").write_text("x", encoding="utf-8")
        tool.add("real.rs", desc="真实文件")
        self.assertNotIn("children", tool.get("real.rs"))

    def test_declared_dir_without_disk_still_dir(self):
        tool = self.make_tool()  # 磁盘无该路径（虚拟目录是合法特性）
        tool.add("virtual/group", desc="聚合分类", is_dir_entry=True)
        self.assertIn("children", tool.get("virtual/group"))

    def test_add_existing_file_entry_not_flipped(self):
        tool = self.make_tool()
        tool.add("legacy", desc="旧条目")  # 先录文件条目（磁盘尚无）
        tool.repo_root.joinpath("legacy").mkdir()  # 磁盘后变成目录
        tool.add("legacy", desc="更新")  # upsert 不隐式翻转既有类型，存量错配由 check 报
        self.assertNotIn("children", tool.get("legacy"))

    def test_add_batch_disk_dir_auto(self):
        tool = self.make_tool()
        tool.repo_root.joinpath("assets", "icons").mkdir(parents=True)
        n = tool.add_batch([
            {"path": "assets/icons", "desc": "图标集"},  # 清单未标 dir，磁盘是目录
            {"path": "docs/guide.md", "desc": "指南"},
        ])
        self.assertEqual(n, 2)
        self.assertIn("children", tool.get("assets/icons"))
        self.assertNotIn("children", tool.get("docs/guide.md"))


class RmBatchTest(SandboxTest):
    """rm-batch：批量删除一次变更一步历史，原子生效，修剪变空父目录语义保留。"""

    def test_removes_all_and_prunes_emptied_roots(self):
        tool = self.make_tool()
        tool.add_batch([
            {"path": "tmp/a.rs", "desc": "临时"},
            {"path": "tmp/b.rs", "desc": "临时"},
            {"path": "tmp/sub/c.rs", "desc": "临时"},
        ])
        n = tool.rm_batch(["tmp/a.rs", "tmp/sub/c.rs", "tmp/b.rs"])
        self.assertEqual(n, 3)
        self.assertNotIn("tmp", tool.load()["tree"])

    def test_single_history_step_undo_restores_all(self):
        tool = self.make_tool()
        tool.rm_batch(["apps/main.tsx", "Cargo.toml"])
        undo, _ = tool.history_summary()
        self.assertEqual(len(undo), 1)
        self.assertIn("rm-batch", undo[0])
        tool.undo()
        data = tool.load()
        self.assertIn("main.tsx", data["tree"]["apps"]["children"])
        self.assertIn("Cargo.toml", data["tree"])

    def test_parent_kept_when_sibling_remains(self):
        tool = self.make_tool()
        tool.rm_batch(["apps/util.ts"])
        self.assertIn("main.tsx", tool.load()["tree"]["apps"]["children"])

    def test_atomic_reject_missing_entry(self):
        tool = self.make_tool()
        before = tool.tree_json.read_text(encoding="utf-8")
        with self.assertRaises(ToolError):
            tool.rm_batch(["apps/main.tsx", "no/such.rs"])
        self.assertEqual(tool.tree_json.read_text(encoding="utf-8"), before)
        undo, _ = tool.history_summary()
        self.assertEqual(undo, [])

    def test_reject_duplicate_paths(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.rm_batch(["apps/util.ts", "apps/util.ts"])

    def test_reject_ancestor_descendant_mix(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.rm_batch(["apps", "apps/main.tsx"])

    def test_sibling_prefix_not_misjudged(self):
        """a/b 与 a/bc 是兄弟而非祖先-后代，前缀判断不得误伤。"""
        tool = self.make_tool()
        tool.add_batch([
            {"path": "tmp/a/b.rs", "desc": "临时"},
            {"path": "tmp/a/bc.rs", "desc": "临时"},
        ])
        tool.rm_batch(["tmp/a/b.rs", "tmp/a/bc.rs"])
        self.assertNotIn("tmp", tool.load()["tree"])


class MvTest(SandboxTest):
    """mv：条目带信息迁移（含子树）——数据层操作不碰磁盘，全树自动重写指向旧路径的 rel 边。"""

    def assert_mv_rejected(self, tool: TreeTool, src: str, dst: str) -> None:
        """拒绝即原子：tree.json 字节不变，撤销栈与重做栈均空（调用前须无历史）。"""
        before = tool.tree_json.read_text(encoding="utf-8")
        with self.assertRaises(ToolError):
            tool.mv(src, dst)
        self.assertEqual(tool.tree_json.read_text(encoding="utf-8"), before)
        undo, redo = tool.history_summary()
        self.assertEqual((undo, redo), ([], []))

    def test_moves_file_with_all_fields(self):
        tool = self.make_tool()
        tool.add("apps/util.ts", rel=["Cargo.toml"])
        tool.mv("apps/util.ts", "lib/util.ts")
        node = tool.get("lib/util.ts")
        self.assertEqual(node["desc"], "工具")
        self.assertEqual(node["detail"], ["纯函数工具集"])
        self.assertEqual(node["tags"], ["pure"])
        self.assertEqual(node["rel"], ["Cargo.toml"])  # 指向未移动目标的边不动
        with self.assertRaises(ToolError):
            tool.get("apps/util.ts")
        self.assertIn("main.tsx", tool.get("apps")["children"])  # 有兄弟则源父目录保留

    def test_moves_dir_subtree_intact(self):
        tool = self.make_tool()
        tool.mv("apps", "src/apps")
        apps = tool.get("src/apps")
        self.assertEqual(apps["desc"], "应用层")
        self.assertEqual(apps["children"]["main.tsx"]["desc"], "入口")
        self.assertEqual(apps["children"]["util.ts"]["tags"], ["pure"])
        self.assertNotIn("apps", tool.load()["tree"])

    def test_rewrites_rel_edge_pointing_to_old_path(self):
        tool = self.make_tool()
        tool.add("docs/guide.md", desc="指南", detail=["文档"], rel=["apps/util.ts"])
        n = tool.mv("apps/util.ts", "lib/util.ts")
        self.assertEqual(n, 1)
        self.assertEqual(tool.get("docs/guide.md")["rel"], ["lib/util.ts"])

    def test_rewrites_rel_edges_pointing_into_subtree(self):
        tool = self.make_tool()
        tool.add("docs/guide.md", desc="指南", detail=["文档"], rel=["apps/main.tsx", "apps/util.ts"])
        tool.mv("apps", "src")
        self.assertEqual(tool.get("docs/guide.md")["rel"], ["src/main.tsx", "src/util.ts"])

    def test_rewrites_rel_edges_inside_moved_subtree(self):
        """子树内部条目的 rel 存全路径，目录迁移后若不前缀重写即悬空。"""
        tool = self.make_tool()
        tool.add("apps/main.tsx", rel=["apps/util.ts"])
        tool.mv("apps", "src")
        tool.render()
        self.assertEqual(tool.get("src/main.tsx")["rel"], ["src/util.ts"])
        errors, _ = tool.check()
        self.assertEqual(errors, [])

    def test_rel_of_query_hits_new_path(self):
        tool = self.make_tool()
        tool.add("apps/main.tsx", rel=["apps/util.ts"])
        tool.mv("apps/util.ts", "lib/util.ts")
        self.assertEqual([p for p, _ in tool.query(rel_of="lib/util.ts")], ["apps/main.tsx"])
        self.assertEqual(tool.query(rel_of="apps/util.ts"), [])

    def test_single_history_step_undo_restores(self):
        tool = self.make_tool()
        tool.add("docs/guide.md", desc="指南", detail=["文档"], rel=["apps/util.ts"])
        tool.mv("apps/util.ts", "lib/util.ts")
        undo, _ = tool.history_summary()
        self.assertEqual(undo, ["add docs/guide.md", "mv apps/util.ts -> lib/util.ts"])
        tool.undo()
        self.assertEqual(tool.get("docs/guide.md")["rel"], ["apps/util.ts"])
        self.assertEqual(tool.get("apps/util.ts")["desc"], "工具")
        with self.assertRaises(ToolError):
            tool.get("lib/util.ts")

    def test_reject_missing_src_leaves_untouched(self):
        tool = self.make_tool()
        self.assert_mv_rejected(tool, "nope.rs", "lib/nope.rs")

    def test_reject_existing_dst(self):
        tool = self.make_tool()
        self.assert_mv_rejected(tool, "apps/util.ts", "Cargo.toml")
        self.assert_mv_rejected(tool, "apps\\util.ts", "apps\\main.tsx")  # 反斜杠变体归一化后同判

    def test_reject_same_src_dst(self):
        tool = self.make_tool()
        self.assert_mv_rejected(tool, "apps/util.ts", "apps/util.ts")
        self.assert_mv_rejected(tool, "apps\\util.ts", "apps//util.ts")  # 分隔符变体归一化后同判

    def test_reject_dst_inside_src_subtree(self):
        tool = self.make_tool()
        self.assert_mv_rejected(tool, "apps", "apps/sub")
        # 粗粒度收录（无 children）时目标在"虚拟子树"下同样拒绝
        coarse = self.make_tool(data={"tags": {}, "tree": {"assets": {"desc": "图标集"}}})
        self.assert_mv_rejected(coarse, "assets", "assets/icons")

    def test_rename_in_place_keeps_parent_info(self):
        """时序回归：同父重命名且源是父目录唯一孩子，父目录不得被修剪后以空骨架重建。"""
        data = {"tags": {}, "tree": {"solo": {
            "desc": "独子目录", "detail": ["不该丢"],
            "children": {"only.rs": {"desc": "唯一", "detail": ["x"]}},
        }}}
        tool = self.make_tool(data=data)
        tool.mv("solo/only.rs", "solo/renamed.rs")
        parent = tool.get("solo")
        self.assertEqual(parent["desc"], "独子目录")
        self.assertEqual(parent["detail"], ["不该丢"])
        self.assertEqual(parent["children"]["renamed.rs"]["desc"], "唯一")
        self.assertNotIn("only.rs", parent["children"])

    def test_prunes_emptied_source_parents(self):
        data = {"tags": {}, "tree": {"a": {"desc": "", "children": {"b.rs": {"desc": "x", "detail": ["d"]}}}}}
        tool = self.make_tool(data=data)
        tool.mv("a/b.rs", "b.rs")
        self.assertEqual(set(tool.load()["tree"]), {"b.rs"})

    def test_auto_creates_dst_parents(self):
        tool = self.make_tool()
        tool.mv("apps/util.ts", "lib/core/util.ts")
        self.assertIn("util.ts", tool.get("lib/core")["children"])
        self.assertEqual(tool.get("lib")["desc"], "")  # 自动建的父链 desc 待补
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(errors, [])

    def test_reject_dst_mid_path_is_file(self):
        tool = self.make_tool()
        self.assert_mv_rejected(tool, "apps/util.ts", "Cargo.toml/util.ts")

    def test_moves_dir_keeps_collapsed_flag(self):
        data = {"tags": {}, "tree": {"legacy": {
            "desc": "旧模块", "collapsed": True,
            "children": {"old.rs": {"desc": "旧", "detail": ["x"]}},
        }}}
        tool = self.make_tool(data=data)
        tool.mv("legacy", "archived/legacy")
        node = tool.get("archived/legacy")
        self.assertIs(node["collapsed"], True)
        self.assertIn("old.rs", node["children"])

    def test_moves_file_keeps_hidden_flag(self):
        data = {"tags": {}, "tree": {"apps": {"desc": "应用层", "children": {
            "util.ts": {"desc": "工具", "detail": ["纯函数"], "hidden": True}}}}}
        tool = self.make_tool(data=data)
        tool.mv("apps/util.ts", "lib/util.ts")
        self.assertIs(tool.get("lib/util.ts")["hidden"], True)
        tool.render()
        self.assertNotIn("工具", tool.agents_md.read_text(encoding="utf-8"))  # 隐藏渲染仍生效

    def test_no_rewrite_on_sibling_prefix(self):
        """指向兄弟前缀路径（apps2/x）的边不得被裸前缀匹配误伤。"""
        tool = self.make_tool()
        tool.add("apps2/x.rs", desc="x", detail=["x"])
        tool.add("docs/guide.md", desc="指南", detail=["d"], rel=["apps2/x.rs"])
        tool.mv("apps", "src")
        self.assertEqual(tool.get("docs/guide.md")["rel"], ["apps2/x.rs"])

    def test_mixed_rel_keeps_misses(self):
        """命中与未命中混合的 rel 列表：只改命中项，未命中项原样保留。"""
        tool = self.make_tool()
        tool.add("docs/guide.md", desc="指南", detail=["d"], rel=["Cargo.toml", "apps/util.ts"])
        n = tool.mv("apps/util.ts", "lib/util.ts")
        self.assertEqual(n, 1)
        self.assertEqual(tool.get("docs/guide.md")["rel"], ["Cargo.toml", "lib/util.ts"])

    def test_returns_edge_count_not_entry_count(self):
        """n 按重写的边数计（非发生重写的条目数）：单节点两条命中边计 2。"""
        tool = self.make_tool()
        tool.add("docs/guide.md", desc="指南", detail=["d"],
                 rel=["Cargo.toml", "apps/util.ts", "apps/main.tsx"])
        n = tool.mv("apps", "src")
        self.assertEqual(n, 2)
        self.assertEqual(tool.get("docs/guide.md")["rel"], ["Cargo.toml", "src/main.tsx", "src/util.ts"])

    def test_mv_not_blocked_by_preexisting_dangling_rel(self):
        """既有悬空 rel（rm 的合法产物）不阻塞无关 mv——mv 正是修复悬空的手段。"""
        tool = self.make_tool()
        tool.add("apps/tmp.rs", desc="t", detail=["t"])
        tool.add("docs/guide.md", desc="指南", detail=["d"], rel=["apps/tmp.rs"])
        tool.rm("apps/tmp.rs")  # rm 不重写 rel，guide.md 的边悬空
        tool.mv("Cargo.toml", "Cargo.lock")
        self.assertEqual(tool.get("docs/guide.md")["rel"], ["apps/tmp.rs"])  # 悬空边原样留给 check 报告

    def test_pruned_ancestor_rel_left_dangling_for_check(self):
        """源端父链修剪可使指向被修剪祖先的 rel 边悬空——同 rm 口径，由 check 报 E 兜底。"""
        data = {"tags": {}, "tree": {
            "apps": {"desc": "应用层", "children": {"util.ts": {"desc": "工具", "detail": ["x"]}}},
            "docs.md": {"desc": "文档", "detail": ["d"], "rel": ["apps"]},
        }}
        tool = self.make_tool(data=data)
        n = tool.mv("apps/util.ts", "lib/util.ts")  # apps 变空被修剪
        self.assertEqual(n, 0)  # 指向祖先 apps 的边不在前缀改写范围
        self.assertEqual(tool.get("docs.md")["rel"], ["apps"])  # 悬空边原样保留
        with self.assertRaises(ToolError):
            tool.get("apps")
        tool.render()
        errors, _ = tool.check()
        self.assertTrue(any("rel 目标不在树中" in e for e in errors))

    def test_mv_leaves_disk_files_alone(self):
        """数据层迁移不碰磁盘：真实文件留在原位，新路径不产生文件。"""
        tool = self.make_tool()
        src_file = tool.repo_root / "apps" / "util.ts"
        src_file.parent.mkdir(parents=True, exist_ok=True)
        src_file.write_text("x", encoding="utf-8")
        tool.mv("apps/util.ts", "lib/util.ts")
        self.assertTrue(src_file.exists())
        self.assertFalse((tool.repo_root / "lib" / "util.ts").exists())

    def test_redo_restores_move(self):
        tool = self.make_tool()
        tool.mv("apps/util.ts", "lib/util.ts")
        tool.undo()
        op = tool.redo()
        self.assertEqual(op, "mv apps/util.ts -> lib/util.ts")
        self.assertEqual(tool.get("lib/util.ts")["desc"], "工具")
        with self.assertRaises(ToolError):
            tool.get("apps/util.ts")

    def test_top_level_rename(self):
        tool = self.make_tool()
        tool.mv("Cargo.toml", "Cargo.lock")
        self.assertEqual(tool.get("Cargo.lock")["desc"], "根配置")
        self.assertNotIn("Cargo.toml", tool.load()["tree"])


class CmdMvTest(SandboxTest):
    """CLI 层 mv：参数直通 + 写后自动重渲染 AGENTS.md。"""

    def test_cmd_mv_passes_args_and_renders(self):
        import types

        tool = self.make_tool()
        _cmd_mv(tool, types.SimpleNamespace(src="apps/util.ts", dst="lib/util.ts"))
        self.assertEqual(tool.get("lib/util.ts")["desc"], "工具")
        text = tool.agents_md.read_text(encoding="utf-8")
        self.assertIn("lib/", text)  # 简版树为多行树形，目录与文件名分行渲染
        self.assertIn("工具", text)

    def test_cmd_mv_reports_rewrite_count(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_tool()
        tool.add("docs/guide.md", desc="指南", detail=["d"], rel=["apps/util.ts"])
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_mv(tool, types.SimpleNamespace(src="apps/util.ts", dst="lib/util.ts"))
        self.assertIn("已迁移并重渲染: apps/util.ts -> lib/util.ts（重写 1 条 rel 边）", buf.getvalue())


class MvBatchTest(SandboxTest):
    """mv-batch：一份清单 = 一次变更 = 一步历史；批内互斥预校验，任一非法整批拒绝。"""

    def moves_basic(self) -> list[dict]:
        return [
            {"src": "apps/util.ts", "dst": "lib/util.ts"},
            {"src": "Cargo.toml", "dst": "conf/Cargo.toml"},
        ]

    def assert_batch_rejected(self, tool: TreeTool, moves, msg: str | None = None) -> None:
        """拒绝即原子：tree.json 字节不变，撤销栈与重做栈均空（调用前须无历史）。msg 标注子场景。"""
        before = tool.tree_json.read_text(encoding="utf-8")
        with self.assertRaises(ToolError, msg=msg):
            tool.mv_batch(moves)
        self.assertEqual(tool.tree_json.read_text(encoding="utf-8"), before)
        undo, redo = tool.history_summary()
        self.assertEqual((undo, redo), ([], []))

    def test_moves_all_entries_with_fields(self):
        tool = self.make_tool()
        n, edges = tool.mv_batch(self.moves_basic())
        self.assertEqual((n, edges), (2, 0))
        util = tool.get("lib/util.ts")
        self.assertEqual(util["desc"], "工具")
        self.assertEqual(util["detail"], ["纯函数工具集"])
        self.assertEqual(util["tags"], ["pure"])
        self.assertEqual(tool.get("conf/Cargo.toml")["detail"][0][:9], "workspace")
        with self.assertRaises(ToolError):
            tool.get("apps/util.ts")
        self.assertIn("main.tsx", tool.get("apps")["children"])  # 有兄弟则源父目录保留

    def test_shared_dst_parent_auto_created(self):
        tool = self.make_tool()
        tool.mv_batch([
            {"src": "apps/util.ts", "dst": "lib/core/util.ts"},
            {"src": "Cargo.toml", "dst": "lib/conf.toml"},
        ])
        lib_children = set(tool.get("lib")["children"])
        self.assertEqual(lib_children, {"core", "conf.toml"})
        self.assertIn("util.ts", tool.get("lib/core")["children"])

    def test_rel_rewrite_stacking_batch_internal(self):
        """批内互指：两者都移动，rel 边最终指向对方新路径（与清单顺序无关）。"""
        tool = self.make_tool()
        tool.add("apps/main.tsx", rel=["apps/util.ts"])
        tool.mv_batch([
            {"src": "apps/main.tsx", "dst": "src/main.tsx"},
            {"src": "apps/util.ts", "dst": "lib/util.ts"},
        ])
        self.assertEqual(tool.get("src/main.tsx")["rel"], ["lib/util.ts"])
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(errors, [])

    def test_rel_rewrite_stacking_order_independent(self):
        """叠加顺序无关的另一半：反序清单结果一致，且 edges 按重写动作累计。"""
        tool = self.make_tool()
        tool.add("apps/main.tsx", rel=["apps/util.ts"])
        n, edges = tool.mv_batch([
            {"src": "apps/util.ts", "dst": "lib/util.ts"},
            {"src": "apps/main.tsx", "dst": "src/main.tsx"},
        ])
        self.assertEqual((n, edges), (2, 1))
        self.assertEqual(tool.get("src/main.tsx")["rel"], ["lib/util.ts"])
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(errors, [])

    def test_prunes_dir_when_all_children_moved(self):
        """批量移光目录全部孩子：最后一条触发父目录修剪，undo 完整恢复子树。"""
        tool = self.make_tool()
        tool.mv_batch([
            {"src": "apps/main.tsx", "dst": "src/main.tsx"},
            {"src": "apps/util.ts", "dst": "lib/util.ts"},
        ])
        self.assertNotIn("apps", tool.load()["tree"])
        self.assertEqual(tool.get("lib/util.ts")["tags"], ["pure"])
        tool.undo()
        apps = tool.get("apps")
        self.assertEqual(set(apps["children"]), {"main.tsx", "util.ts"})
        self.assertEqual(apps["desc"], "应用层")

    def test_promote_out_of_dir_in_batch(self):
        """同条目的 dst 与自身 src 祖先关系不进交叉检查（i != j）：批内提升合法。"""
        tool = self.make_tool()
        tool.mv_batch([
            {"src": "apps/util.ts", "dst": "util.ts"},
            {"src": "Cargo.toml", "dst": "conf/Cargo.toml"},
        ])
        self.assertEqual(tool.get("util.ts")["tags"], ["pure"])
        self.assertIn("main.tsx", tool.get("apps")["children"])  # 有兄弟则源父保留

    def test_rel_rewrite_stacking_external(self):
        tool = self.make_tool()
        tool.add("docs/guide.md", desc="指南", detail=["d"],
                 rel=["Cargo.toml", "apps/util.ts"])
        _, edges = tool.mv_batch(self.moves_basic())
        self.assertEqual(edges, 2)
        self.assertEqual(tool.get("docs/guide.md")["rel"], ["conf/Cargo.toml", "lib/util.ts"])

    def test_moves_dir_with_subtree(self):
        tool = self.make_tool()
        tool.mv_batch([{"src": "apps", "dst": "src/apps"}])
        self.assertEqual(tool.get("src/apps")["desc"], "应用层")
        self.assertIn("main.tsx", tool.get("src/apps")["children"])
        self.assertNotIn("apps", tool.load()["tree"])

    def test_single_history_step_undo_restores_all(self):
        tool = self.make_tool()
        tool.add("docs/guide.md", desc="指南", detail=["d"], rel=["apps/util.ts"])
        tool.mv_batch(self.moves_basic())
        undo, redo = tool.history_summary()
        self.assertEqual(undo, ["add docs/guide.md", "mv-batch 2 条"])
        self.assertEqual(redo, [])
        tool.undo()
        self.assertEqual(tool.get("apps/util.ts")["desc"], "工具")
        self.assertEqual(tool.get("docs/guide.md")["rel"], ["apps/util.ts"])
        with self.assertRaises(ToolError):
            tool.get("lib/util.ts")
        with self.assertRaises(ToolError):
            tool.get("conf/Cargo.toml")

    def test_redo_restores_whole_batch(self):
        tool = self.make_tool()
        tool.mv_batch(self.moves_basic())
        tool.undo()
        op = tool.redo()
        self.assertEqual(op, "mv-batch 2 条")
        self.assertEqual(tool.get("lib/util.ts")["desc"], "工具")

    def test_reject_bad_manifest_structure(self):
        tool = self.make_tool()
        for bad in ([], {}, {"no_moves": []}, {"moves": "x"}, {"moves": []}):
            with self.assertRaises(ToolError, msg=repr(bad)):
                tool.mv_batch(bad)

    def test_reject_bad_entry(self):
        tool = self.make_tool()
        for bad in (
            ["not-object"],
            [{"src": "apps/util.ts"}],                       # 缺 dst
            [{"dst": "lib/util.ts"}],                        # 缺 src
            [{"src": "apps/util.ts", "dst": ""}],            # 空 dst
            [{"src": 1, "dst": "lib/util.ts"}],              # 非字符串
            [{"src": "apps/util.ts", "dst": "lib/x", "why": "x"}],  # 未知字段
        ):
            with self.assertRaises(ToolError, msg=repr(bad)):
                tool.mv_batch(bad)

    def test_reject_path_variant_duplicates(self):
        """反斜杠/双斜杠变体归一化后同判批内重复。"""
        tool = self.make_tool()
        self.assert_batch_rejected(tool, [
            {"src": "apps/util.ts", "dst": "a.ts"},
            {"src": "apps\\util.ts", "dst": "b.rs"},
        ])
        self.assert_batch_rejected(tool, [
            {"src": "apps/util.ts", "dst": "lib/util.ts"},
            {"src": "Cargo.toml", "dst": "lib//util.ts"},
        ])

    def test_reject_single_entry_violations(self):
        """单条四关（src==dst / src 缺失 / dst 已存在 / 自嵌套）任一失败整批拒绝。"""
        tool = self.make_tool()
        self.assert_batch_rejected(tool, [
            {"src": "apps/util.ts", "dst": "apps/util.ts"},
            {"src": "Cargo.toml", "dst": "conf/Cargo.toml"},
        ])
        self.assert_batch_rejected(tool, [
            {"src": "apps/util.ts", "dst": "lib/util.ts"},
            {"src": "nope.rs", "dst": "lib/nope.rs"},
        ])
        self.assert_batch_rejected(tool, [
            {"src": "apps/util.ts", "dst": "lib/util.ts"},
            {"src": "Cargo.toml", "dst": "apps/main.tsx"},
        ])
        self.assert_batch_rejected(tool, [
            {"src": "apps", "dst": "apps/sub"},
        ])

    def test_reject_src_ancestor_descendant(self):
        tool = self.make_tool()
        self.assert_batch_rejected(tool, [
            {"src": "apps", "dst": "src/apps"},
            {"src": "apps/main.tsx", "dst": "src/main.tsx"},
        ])

    def test_reject_dst_ancestor_descendant(self):
        tool = self.make_tool()
        self.assert_batch_rejected(tool, [
            {"src": "apps/util.ts", "dst": "t/u"},
            {"src": "Cargo.toml", "dst": "t/u/v"},
        ])

    def test_reject_move_chain(self):
        """不支持批内移动链：第一条的 dst 恰是第二条的 src——静态交叉检查（目的地落在他人源路径上）拦截。"""
        data = {"tags": {}, "tree": {
            "apps": {"desc": "应用层", "children": {"util.ts": {"desc": "工具", "detail": ["x"]}}},
            "mid": {"desc": "中转", "children": {"x.rs": {"desc": "x", "detail": ["x"]}}},
        }}
        tool = self.make_tool(data=data)
        self.assert_batch_rejected(tool, [
            {"src": "apps/util.ts", "dst": "mid/x.rs"},
            {"src": "mid/x.rs", "dst": "end/x.rs"},
        ])

    def test_reject_src_inside_other_dst_subtree(self):
        """对称交叉：源路径落在其他移动的目的地上——后续条会"看见"前序结果，破坏初始树语义。"""
        # 形态一：第二条 src 在第一条 dst 子树内（初始树不存在，逐条应用会因前序挂载而存在）
        data = {"tags": {}, "tree": {
            "a": {"desc": "A目录", "children": {"x.rs": {"desc": "x", "detail": ["x"]}}},
        }}
        tool = self.make_tool(data=data)
        self.assert_batch_rejected(tool, [
            {"src": "a", "dst": "b"},
            {"src": "b/x.rs", "dst": "d"},
        ], msg="src 在他人 dst 子树内")
        # 形态二：第二条 dst 是第一条 src 修剪后的变空祖先（初始树存在应拒，应用期被修剪后静默重建）
        data_b = {"tags": {}, "tree": {
            "a": {"desc": "A目录"},
            "d": {"desc": "D目录", "detail": ["不该丢"], "children": {"x.rs": {"desc": "x", "detail": ["x"]}}},
        }}
        tool_b = self.make_tool(data=data_b)
        self.assert_batch_rejected(tool_b, [
            {"src": "d/x.rs", "dst": "e"},
            {"src": "a", "dst": "d"},
        ], msg="dst 是他人 src 修剪后的变空祖先")

    def test_reject_dst_inside_other_src_subtree(self):
        """目的地不得落在批内其他移动的源子树内（否则随源整体被搬走）。清单顺序两种都拒。"""
        tool = self.make_tool()
        moves = [
            {"src": "apps", "dst": "src/apps"},
            {"src": "Cargo.toml", "dst": "apps/renamed.toml"},
        ]
        self.assert_batch_rejected(tool, moves)
        self.assert_batch_rejected(tool, list(reversed(moves)))


class CmdMvBatchTest(SandboxTest):
    """CLI 层 mv-batch：清单读取/解析契约与写后自动渲染。"""

    def write_manifest(self, tool: TreeTool, obj) -> str:
        path = tool.tree_json.parent / "moves.json"
        path.write_text(json.dumps(obj, ensure_ascii=False), encoding="utf-8")
        return str(path)

    def test_cmd_reads_manifest_and_renders(self):
        import types

        tool = self.make_tool()
        manifest = self.write_manifest(tool, {"moves": [{"src": "apps/util.ts", "dst": "lib/util.ts"}]})
        _cmd_mv_batch(tool, types.SimpleNamespace(manifest=manifest))
        self.assertEqual(tool.get("lib/util.ts")["desc"], "工具")
        self.assertIn("lib/", tool.agents_md.read_text(encoding="utf-8"))

    def test_cmd_reports_count_and_edges(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_tool()
        tool.add("docs/guide.md", desc="指南", detail=["d"], rel=["apps/util.ts", "Cargo.toml"])
        manifest = self.write_manifest(tool, {"moves": [
            {"src": "apps/util.ts", "dst": "lib/util.ts"},
            {"src": "Cargo.toml", "dst": "conf/Cargo.toml"},
        ]})
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_mv_batch(tool, types.SimpleNamespace(manifest=manifest))
        self.assertIn("已批量迁移并重渲染: 2 条（重写 2 条 rel 边；一次变更，单步历史）", buf.getvalue())

    def test_cmd_output_omits_edges_when_zero(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_tool()
        manifest = self.write_manifest(tool, {"moves": [{"src": "apps/util.ts", "dst": "lib/util.ts"}]})
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_mv_batch(tool, types.SimpleNamespace(manifest=manifest))
        self.assertIn("已批量迁移并重渲染: 1 条（一次变更，单步历史）", buf.getvalue())
        self.assertNotIn("重写", buf.getvalue())

    def test_cmd_rejects_missing_file_and_bad_json(self):
        import types

        tool = self.make_tool()
        with self.assertRaises(ToolError):
            _cmd_mv_batch(tool, types.SimpleNamespace(manifest=str(tool.tree_json.parent / "nope.json")))
        path = tool.tree_json.parent / "moves.json"
        path.write_text("{不是JSON", encoding="utf-8")
        with self.assertRaises(ToolError):
            _cmd_mv_batch(tool, types.SimpleNamespace(manifest=str(path)))

    def test_cmd_rejects_non_moves_structure(self):
        import types

        tool = self.make_tool()
        for obj in ([], {}, {"no_moves": []}, {"moves": "x"}, {"moves": []}):
            manifest = self.write_manifest(tool, obj)
            with self.assertRaises(ToolError, msg=repr(obj)):
                _cmd_mv_batch(tool, types.SimpleNamespace(manifest=manifest))


class CmdBatchTest(SandboxTest):
    """CLI 层：add-batch 清单读取/解析契约，rm-batch 参数直通。"""

    def write_manifest(self, tool: TreeTool, obj) -> str:
        path = tool.tree_json.parent / "batch.json"
        path.write_text(json.dumps(obj, ensure_ascii=False), encoding="utf-8")
        return str(path)

    def test_cmd_add_batch_reads_manifest_and_renders(self):
        import types

        tool = self.make_tool()
        manifest = self.write_manifest(tool, {"entries": [{"path": "apps/cli.ts", "desc": "CLI"}]})
        _cmd_add_batch(tool, types.SimpleNamespace(manifest=manifest))
        self.assertEqual(tool.get("apps/cli.ts")["desc"], "CLI")
        self.assertIn("cli.ts", tool.agents_md.read_text(encoding="utf-8"))

    def test_cmd_add_batch_missing_file(self):
        import types

        tool = self.make_tool()
        args = types.SimpleNamespace(manifest=str(tool.tree_json.parent / "nope.json"))
        with self.assertRaises(ToolError):
            _cmd_add_batch(tool, args)

    def test_cmd_add_batch_rejects_non_object_and_bad_entries(self):
        import types

        tool = self.make_tool()
        for obj in ([], {}, {"no_entries": []}, {"entries": "x"}):
            manifest = self.write_manifest(tool, obj)
            with self.assertRaises(ToolError):
                _cmd_add_batch(tool, types.SimpleNamespace(manifest=manifest))

    def test_cmd_add_batch_rejects_bad_json(self):
        import types

        tool = self.make_tool()
        path = tool.tree_json.parent / "batch.json"
        path.write_text("{不是JSON", encoding="utf-8")
        with self.assertRaises(ToolError):
            _cmd_add_batch(tool, types.SimpleNamespace(manifest=str(path)))

    def test_cmd_rm_batch_passes_paths(self):
        import types

        tool = self.make_tool()
        _cmd_rm_batch(tool, types.SimpleNamespace(paths=["apps/main.tsx", "Cargo.toml"]))
        data = tool.load()
        self.assertEqual(data["tree"]["apps"]["children"], {"util.ts": data["tree"]["apps"]["children"]["util.ts"]})
        self.assertNotIn("Cargo.toml", data["tree"])


class RootTest(SandboxTest):
    """root：固定/清除渲染根名——防 worktree 检出目录名漂移；未设置时自动取仓库根目录名。"""

    def brief_first_line(self, tool: TreeTool) -> str:
        return tool.render_brief_tree().split("\n", 1)[0]

    def test_default_uses_repo_root_name(self):
        tool = self.make_tool()
        self.assertEqual(self.brief_first_line(tool), "Demo/")

    def test_set_root_persists_and_renders(self):
        tool = self.make_tool()
        tool.set_root("Fixed")
        self.assertEqual(tool.load()["root"], "Fixed")
        self.assertEqual(self.brief_first_line(tool), "Fixed/")
        tool.render()
        errors, _ = tool.check()
        self.assertEqual(errors, [])

    def test_canonical_puts_root_first(self):
        tool = self.make_tool()
        tool.set_root("Fixed")
        text = tool.tree_json.read_text(encoding="utf-8")
        self.assertLess(text.index('"root"'), text.index('"tags"'))

    def test_clear_root_restores_auto(self):
        tool = self.make_tool()
        tool.set_root("Fixed")
        tool.clear_root()
        self.assertNotIn("root", tool.load())
        self.assertEqual(self.brief_first_line(tool), "Demo/")

    def test_clear_without_custom_rejected(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.clear_root()

    def test_set_root_rejects_empty(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.set_root("")
        with self.assertRaises(ToolError):
            tool.set_root(None)

    def test_root_change_is_single_undo_step(self):
        tool = self.make_tool()
        tool.set_root("Fixed")
        undo, _ = tool.history_summary()
        self.assertEqual(len(undo), 1)
        tool.undo()
        self.assertNotIn("root", tool.load())

    def test_worktree_dir_rename_does_not_drift(self):
        """同一 tree.json 在不同检出目录名下：未固定根名随目录漂移，设置后渲染稳定。"""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        skill_dir = root / ".agents" / "skills" / "file-tree"
        (skill_dir / "scripts").mkdir(parents=True)
        common = dict(
            tree_json=skill_dir / "tree.json", agents_md=root / "AGENTS.md",
            repo_root=root, history_path=skill_dir / ".history.json",
        )
        tool_a = TreeTool(root_name="QuotaTray", **common)
        tool_a.write_data(make_data())
        tool_b = TreeTool(root_name="QuotaTray-feat", **common)  # 同数据、不同检出目录名
        self.assertNotEqual(self.brief_first_line(tool_a), self.brief_first_line(tool_b))
        tool_a.set_root("QuotaTray")
        self.assertEqual(self.brief_first_line(tool_a), self.brief_first_line(tool_b))

    def test_normalize_rejects_bad_root(self):
        for bad in (123, "", [], {}, None):
            with self.assertRaises(ToolError):
                normalize_data({"root": bad, "tags": {}, "tree": {}})

    def test_check_reports_hand_edited_bad_root(self):
        tool = self.make_tool()
        tool.render()
        original = tool.tree_json.read_text(encoding="utf-8")
        tool.tree_json.write_text(original.replace("{", '{"root": 1,', 1), encoding="utf-8", newline="\n")
        errors, _ = tool.check()
        self.assertTrue(any("结构非法" in e for e in errors))


class CmdRootTest(SandboxTest):
    """CLI 层 root 命令：查看/设置/清除与互斥约束。"""

    def make_args(self, name=None, clear=False):
        import types

        return types.SimpleNamespace(name=name, clear=clear)

    def test_view_without_args_shows_current(self):
        tool = self.make_tool()
        _cmd_root(tool, self.make_args())  # 仅查看，不抛错即通过

    def test_set_then_clear_roundtrip(self):
        tool = self.make_tool()
        _cmd_root(tool, self.make_args(name="Fixed"))
        self.assertEqual(tool.load()["root"], "Fixed")
        self.assertIn("Fixed/", tool.agents_md.read_text(encoding="utf-8"))
        _cmd_root(tool, self.make_args(clear=True))
        self.assertNotIn("root", tool.load())
        self.assertIn("Demo/", tool.agents_md.read_text(encoding="utf-8"))

    def test_clear_with_name_rejected(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            _cmd_root(tool, self.make_args(name="X", clear=True))


class CompactWriteContractTest(SandboxTest):
    """新规范写入编码的独立字节契约（规格 F02/F03）。

    期望全部为手写固定字面量（含中文、字符串内空白/换行/转义、空树、末尾 LF），
    不得用被测 serializer 自己生成唯一期望。
    """

    def test_exact_bytes_handwritten(self):
        # 字符串内部空格保真（desc 内空格）、换行/引号/反斜杠以 JSON 转义保真（detail 行）。
        # 期望为手写转义字面量（\\n 等即 JSON 文本中的两字符转义序列），不经 serializer 生成。
        data = {
            "tags": {"文档": "文档类"},
            "tree": {
                "说明.md": {
                    "desc": "中文 说明",
                    "detail": ["第一行\n第二行 \"引号\" \\ 反斜杠"],
                    "tags": ["文档"],
                },
            },
        }
        tool = self.make_tool(data=data)
        expected = (
            '{"tags":{"文档":"文档类"},'
            '"tree":{"说明.md":{"kind":"file","desc":"中文 说明",'
            '"detail":["第一行\\n第二行 \\"引号\\" \\\\ 反斜杠"],"tags":["文档"]}}}'
            "\n"
        ).encode("utf-8")
        self.assertEqual(tool.tree_json.read_bytes(), expected)

    def test_empty_tree_bytes(self):
        tool = self.make_tool(data={"tags": {}, "tree": {}})  # 空 tags 词表被规范化剔除
        self.assertEqual(tool.tree_json.read_bytes(), b'{"tree":{}}\n')

    def test_single_trailing_lf_no_bom_one_line(self):
        tool = self.make_tool()
        raw = tool.tree_json.read_bytes()
        self.assertFalse(raw.startswith(b"\xef\xbb\xbf"))  # UTF-8 无 BOM
        self.assertTrue(raw.endswith(b"\n"))
        self.assertFalse(raw.endswith(b"\n\n"))  # 末尾恰好一个 LF
        self.assertNotIn(b"\n", raw[:-1])  # 正文单行：紧凑无缩进

    def test_root_key_first_in_compact(self):
        tool = self.make_tool()
        tool.set_root("固定根")
        self.assertTrue(tool.tree_json.read_text(encoding="utf-8").startswith('{"root":"固定根",'))

    def test_repeat_write_identical_bytes(self):
        tool = self.make_tool()
        first = tool.tree_json.read_bytes()
        tool.write_data(tool.load())  # 重复写入同一规范化内容
        self.assertEqual(tool.tree_json.read_bytes(), first)

    def test_dumps_functions_two_forms(self):
        # 共享判定来源的原料：同一规范化数据恰有两种规范序列化形态
        data = normalize_data(make_data())
        self.assertEqual(dumps_canonical(data), compact_dumps(data))
        self.assertEqual(dumps_canonical_legacy(data), legacy_dumps(data))
        self.assertNotEqual(dumps_canonical(data), dumps_canonical_legacy(data))


class LegacyCheckCompatTest(SandboxTest):
    """旧两空格规范格式的读取与检查兼容（规格 F04/F05/F06）。

    旧格式样本一律由独立旧编码规则（legacy_dumps）字面构造，不经新的 write_data。
    """

    def write_legacy(self, tool: TreeTool, data: dict) -> None:
        tool.tree_json.write_text(legacy_dumps(normalize_data(data)), encoding="utf-8", newline="\n")

    def test_check_strict_accepts_both_formats(self):
        tool = self.make_tool()
        tool.render()
        self.assertEqual(tool.check(strict=True), ([], []))  # 新规范
        self.write_legacy(tool, make_data())
        errors, warnings = tool.check(strict=True)  # 旧规范：无格式错误、无 strict 告警
        self.assertEqual((errors, warnings), ([], []))

    def test_crlf_legacy_also_accepted(self):
        # CRLF 兼容口径对新旧两种规范形态同时生效
        tool = self.make_tool()
        tool.render()
        self.write_legacy(tool, make_data())
        raw = tool.tree_json.read_text(encoding="utf-8")
        tool.tree_json.write_text(raw.replace("\n", "\r\n"), encoding="utf-8", newline="")
        self.assertEqual(tool.check()[0], [])

    def test_readonly_commands_preserve_legacy_bytes(self):
        # 查询类命令不因读取触发重写：字节与撤销历史均不动（规格 F04/命令行为矩阵）
        tool = self.make_tool()
        self.write_legacy(tool, make_data())
        tool.render()
        before = tool.tree_json.read_bytes()
        tool.get("Cargo.toml")
        tool.query(kw="入口")
        tool.history_summary()
        tool.current_root_name()
        tool.check()
        tool.render()
        self.assertEqual(tool.tree_json.read_bytes(), before)
        self.assertEqual(tool.history_summary(), ([], []))

    def test_is_canonical_text_two_forms_and_rejections(self):
        # 单一判定来源：序列化文本级双形态比较，对象相等不足以通过
        data = normalize_data(make_data())
        self.assertTrue(is_canonical_text(compact_dumps(data)))
        self.assertTrue(is_canonical_text(legacy_dumps(data)))
        self.assertTrue(is_canonical_text(compact_dumps(data).replace("\n", "\r\n")))
        self.assertTrue(is_canonical_text(legacy_dumps(data).replace("\n", "\r\n")))
        self.assertFalse(is_canonical_text(json.dumps(data, ensure_ascii=False, indent=4) + "\n"))
        self.assertFalse(is_canonical_text(json.dumps(data, ensure_ascii=False, separators=(", ", ": ")) + "\n"))
        self.assertFalse(is_canonical_text(compact_dumps(data).rstrip("\n")))  # 缺末尾 LF
        self.assertFalse(is_canonical_text(compact_dumps(data) + "\n"))  # 冗余末尾 LF
        # 键序错乱（对象相等但 tree 在前）与冗余空字段（detail:[]）继续判否
        reordered = {k: data[k] for k in reversed(list(data))}
        self.assertFalse(is_canonical_text(compact_dumps(reordered)))
        self.assertFalse(
            is_canonical_text(compact_dumps({"tags": {}, "tree": {"a.rs": {"kind": "file", "desc": "x", "detail": []}}}))
        )
        self.assertFalse(is_canonical_text("not json"))
        self.assertFalse(is_canonical_text('{"tree":"不是对象"}\n'))  # 结构非法

    def test_canonical_form_returns_specific_form(self):
        # 判定来源的形态化版本：返回具体形态而非布尔；CRLF 归一口径内聚其中，
        # 两种规范的 CRLF 变体归入各自形态
        data = normalize_data(make_data())
        self.assertEqual(canonical_form(compact_dumps(data)), "compact")
        self.assertEqual(canonical_form(compact_dumps(data).replace("\n", "\r\n")), "compact")
        self.assertEqual(canonical_form(legacy_dumps(data)), "legacy")
        self.assertEqual(canonical_form(legacy_dumps(data).replace("\n", "\r\n")), "legacy")
        # 非规范排版（缩进/缺末尾 LF）与解析失败/结构非法 → None
        self.assertIsNone(canonical_form(json.dumps(data, ensure_ascii=False, indent=4) + "\n"))
        self.assertIsNone(canonical_form(compact_dumps(data).rstrip("\n")))
        self.assertIsNone(canonical_form("not json"))
        self.assertIsNone(canonical_form('{"tree":"不是对象"}\n'))

    def test_is_canonical_text_thin_wrapper_of_canonical_form(self):
        # is_canonical_text 是 canonical_form 的薄封装：两口径逐样本一致
        data = normalize_data(make_data())
        for text in (
            compact_dumps(data),
            legacy_dumps(data),
            compact_dumps(data).replace("\n", "\r\n"),
            legacy_dumps(data).replace("\n", "\r\n"),
            json.dumps(data, ensure_ascii=False, indent=4) + "\n",
            compact_dumps(data).rstrip("\n"),
            "not json",
        ):
            with self.subTest(text=text[:40]):
                self.assertEqual(is_canonical_text(text), canonical_form(text) is not None)


    def test_check_rejects_disallowed_layouts(self):
        # 其余排版一律拒绝：对象相等不放行任意缩进/键序/空字段（规格 F06）
        data = normalize_data(make_data())
        variants = {
            "indent-4": json.dumps(data, ensure_ascii=False, indent=4) + "\n",
            "keys-reordered": compact_dumps({k: data[k] for k in reversed(list(data))}),
            "spaced-separators": json.dumps(data, ensure_ascii=False, separators=(", ", ": ")) + "\n",
            "empty-detail-kept": compact_dumps(
                {"tags": {}, "tree": {"a.rs": {"kind": "file", "desc": "x", "detail": []}}}
            ),
            "missing-trailing-lf": compact_dumps(data).rstrip("\n"),
        }
        for name, text in variants.items():
            with self.subTest(variant=name):
                tool = self.make_tool()
                tool.render()
                tool.tree_json.write_text(text, encoding="utf-8", newline="\n")
                errors, _ = tool.check()
                self.assertTrue(any("规范" in e for e in errors), text[:60])


class WriteMigrationTest(SandboxTest):
    """旧格式延迟转换：真正写入时输出新规范，拒绝操作不迁移，undo/redo 保持新规范（F07/F08/F09）。"""

    def write_legacy(self, tool: TreeTool, data: dict) -> None:
        tool.tree_json.write_text(legacy_dumps(normalize_data(data)), encoding="utf-8", newline="\n")

    def make_legacy_tool(self, data: dict | None = None) -> TreeTool:
        tool = self.make_tool(data=data)
        self.write_legacy(tool, data if data is not None else make_data())
        return tool

    def assert_compact_on_disk(self, tool: TreeTool) -> None:
        text = tool.tree_json.read_text(encoding="utf-8")
        self.assertNotIn("\n", text[:-1])  # 正文单行
        self.assertEqual(text, compact_dumps(json.loads(text)))  # 与独立新编码规则逐字节一致

    def test_every_write_category_converts(self):
        # 参数化覆盖全部写入类别：任一现有写入入口在旧格式样本上落盘均为新规范
        cases = {
            "add": lambda t: t.add("apps/new.rs", desc="新增", detail=["完整"]),
            "rm": lambda t: t.rm("Cargo.toml"),
            "mv": lambda t: t.mv("Cargo.toml", "conf/Cargo.toml"),
            "add-batch": lambda t: t.add_batch(
                [{"path": "a.rs", "desc": "a", "detail": ["x"]}, {"path": "b.rs", "desc": "b", "detail": ["y"]}]
            ),
            "rm-batch": lambda t: t.rm_batch(["apps/main.tsx", "apps/util.ts"]),
            "mv-batch": lambda t: t.mv_batch([{"src": "Cargo.toml", "dst": "x/Cargo.toml"}]),
            "mark": lambda t: t.mark("apps", tags=["test"]),
            "tag-add": lambda t: t.tag_add("新标签", "说明"),
            "tag-rm": lambda t: t.tag_rm("test"),  # test 标签未被条目使用，可删
            "root-set": lambda t: t.set_root("固定名"),
            "root-clear": lambda t: t.clear_root(),
        }
        for name, op in cases.items():
            with self.subTest(op=name):
                data = make_data()
                if name == "root-clear":
                    data["root"] = "旧名"  # clear 需已有自定义根名
                tool = self.make_legacy_tool(data)
                op(tool)
                self.assert_compact_on_disk(tool)
                undo_ops, redo_ops = tool.history_summary()
                self.assertEqual((len(undo_ops), redo_ops), (1, []))  # 批量也只记一步历史

    def test_undo_redo_after_migration_stay_compact(self):
        # 旧格式 → 业务写入 → undo → redo：三次落盘均新规范，迁移不占历史步（规格 F09）
        tool = self.make_legacy_tool()
        original = normalize_data(make_data())
        tool.add("apps/new.rs", desc="新增", detail=["完整"])
        self.assertIn("new.rs", tool.load()["tree"]["apps"]["children"])
        self.assert_compact_on_disk(tool)  # 落盘 1：业务写入转新规范
        op = tool.undo()
        self.assertEqual(op, "add apps/new.rs")
        self.assertEqual(tool.load(), original)  # 撤销恢复业务数据
        self.assert_compact_on_disk(tool)  # 落盘 2：undo 不退回旧排版
        self.assertEqual(tool.history_summary(), ([], ["add apps/new.rs"]))
        tool.redo()
        self.assertIn("new.rs", tool.load()["tree"]["apps"]["children"])
        self.assert_compact_on_disk(tool)  # 落盘 3：redo 新规范
        undo_ops, redo_ops = tool.history_summary()
        self.assertEqual((len(undo_ops), len(redo_ops)), (1, 0))  # 全程仅一步业务历史

    def test_rejected_operations_keep_legacy_bytes(self):
        # 写入前拒绝的操作不触发预先迁移：旧文件字节与历史状态保持原样（规格 F08）
        tool = self.make_legacy_tool()
        cases = {
            "add-bad-path": lambda t: t.add("a/../x.rs", desc="x"),
            "add-unknown-tag": lambda t: t.add("y.rs", desc="y", tags=["nope"]),
            "mv-dst-exists": lambda t: t.mv("Cargo.toml", "apps/main.tsx"),
            "rm-missing": lambda t: t.rm("nope.rs"),
            "add-batch-unknown-field": lambda t: t.add_batch([{"path": "a.rs", "desc": "a", "bogus": 1}]),
            "rm-batch-missing": lambda t: t.rm_batch(["apps/main.tsx", "nope.rs"]),
            "mv-batch-bad-entry": lambda t: t.mv_batch([{"src": "Cargo.toml"}]),
            "mark-file-anchor": lambda t: t.mark("Cargo.toml", tags=["test"]),
            "mark-no-action": lambda t: t.mark("apps"),
            "tag-add-exists": lambda t: t.tag_add("pure", "重复"),
            "tag-rm-in-use": lambda t: t.tag_rm("pure"),
            "root-empty": lambda t: t.set_root(""),
            "root-clear-unset": lambda t: t.clear_root(),
            "undo-empty": lambda t: t.undo(),
        }
        before = tool.tree_json.read_bytes()
        for name, op in cases.items():
            with self.subTest(op=name):
                with self.assertRaises(ToolError):
                    op(tool)
                self.assertEqual(tool.tree_json.read_bytes(), before)
        self.assertEqual(tool.history_summary(), ([], []))


class DisplayStabilityTest(SandboxTest):
    """展示接口稳定（规格 F13）：query --json 与 AGENTS.md 渲染不因数据排版新旧而改变。"""

    def test_query_json_and_render_same_across_formats(self):
        import contextlib
        import io
        import types

        compact_tool = self.make_tool()
        legacy_tool = self.make_tool()
        legacy_tool.tree_json.write_text(
            legacy_dumps(normalize_data(make_data())), encoding="utf-8", newline="\n"
        )
        args = types.SimpleNamespace(kw=None, tag=None, rel_of=None, under=None, depth=None, json=True)
        outputs = []
        for tool in (compact_tool, legacy_tool):
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                _cmd_query(tool, args)
            outputs.append(buf.getvalue())
        self.assertEqual(outputs[0], outputs[1])
        compact_tool.render()
        legacy_tool.render()
        self.assertEqual(
            compact_tool.agents_md.read_text(encoding="utf-8"),
            legacy_tool.agents_md.read_text(encoding="utf-8"),
        )


def make_view_data() -> dict:
    """视图用例数据：apps 含文件与子目录 ui（孙代 button/input），供骨架链/子树渲染验证。"""
    return {
        "tags": {"doc": "文档"},
        "tree": {
            "apps": {
                "desc": "应用层",
                "children": {
                    "main.tsx": {"desc": "入口", "detail": ["x"]},
                    "ui": {
                        "desc": "UI 组件",
                        "children": {
                            "button.tsx": {"desc": "按钮", "detail": ["x"]},
                            "input.tsx": {"desc": "输入框", "detail": ["x"]},
                        },
                    },
                },
            },
            "Cargo.toml": {"desc": "根配置", "detail": ["x"]},
        },
    }


class ViewSandboxTest(SandboxTest):
    """视图用例公共夹具：在仓库根下构造绑定文档并读取。"""

    def write_doc(self, tool: TreeTool, rel: str, text: str) -> Path:
        path = tool.repo_root.joinpath(*split_rel_path(rel))
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8", newline="\n")
        return path

    def doc_text(self, tool: TreeTool, rel: str) -> str:
        return tool.repo_root.joinpath(*split_rel_path(rel)).read_text(encoding="utf-8")

    def make_view_tool(self, docs: dict[str, str] | None = None) -> TreeTool:
        tool = self.make_tool(data=make_view_data())
        for rel, text in (docs or {}).items():
            self.write_doc(tool, rel, text)
        return tool

    def rendered_block(self, tool: TreeTool, view_id: str, rel: str) -> str:
        """读取绑定文档中视图块的渲染内容（标记行之间，不含围栏）。"""
        begin, end = view_tree_markers(view_id)
        text = self.doc_text(tool, rel)
        return text[text.index(begin) + len(begin) + 1 : text.index(end)]


def make_filter_data() -> dict:
    """T2 过滤器用例数据：多目录 + 根级散文件 + 三标签（core/doc/misc），供求值、投影与编译验证。"""
    return {
        "tags": {"core": "核心", "doc": "文档", "misc": "杂项"},
        "tree": {
            "apps": {
                "desc": "应用层",
                "children": {
                    "main.tsx": {"desc": "入口", "detail": ["x"], "tags": ["core"]},
                    "ui": {
                        "desc": "UI 组件",
                        "children": {
                            "button.tsx": {"desc": "按钮", "detail": ["x"], "tags": ["doc"]},
                            "input.tsx": {"desc": "输入框", "detail": ["x"], "tags": ["doc"]},
                        },
                    },
                },
            },
            "docs": {
                "desc": "文档目录",
                "children": {
                    "guide.md": {"desc": "指南", "detail": ["x"], "tags": ["doc"]},
                },
            },
            "Cargo.toml": {"desc": "根配置", "detail": ["x"]},
            "README.md": {"desc": "说明", "detail": ["x"], "tags": ["doc"]},
        },
    }


def collect_paths(tree: dict) -> set[str]:
    """测试侧独立收集全量条目路径（黑盒期望构造，不经被测求值引擎）。"""
    out: set[str] = set()

    def walk(children: dict, prefix: list[str]) -> None:
        for name, node in children.items():
            path = "/".join(prefix + [name])
            out.add(path)
            if isinstance(node.get("children"), dict):
                walk(node["children"], prefix + [name])

    walk(tree, [])
    return out


def select_under(tree: dict, anchor: str) -> set[str]:
    """单锚点 under 选中集的测试侧语义：锚点自身含入 + 全部后代。"""
    return {p for p in collect_paths(tree) if p == anchor or p.startswith(anchor + "/")}


class FilterSandboxTest(ViewSandboxTest):
    """T2 过滤器用例公共夹具。"""

    def make_filter_tool(self, docs: dict[str, str] | None = None) -> TreeTool:
        tool = self.make_tool(data=make_filter_data())
        for rel, text in (docs or {}).items():
            self.write_doc(tool, rel, text)
        return tool


class ViewsSchemaTest(SandboxTest):
    """views 顶层键的 schema 契约：id 语法、过滤器表达式树、render_overrides 结构校验、规范化。"""

    def test_normalize_validates_views_structure(self):
        ok_filter = {"op": "under", "path": "apps"}
        bad_views = (
            [],                                                    # 非对象
            {"Ext": {"filter": ok_filter}},                        # id 含大写
            {"default": {"filter": ok_filter}},                    # 保留字
            {"-abc": {"filter": ok_filter}},                       # id 首字符非法
            {"a" * 65: {"filter": ok_filter}},                     # id 超长
            {"v x": {"filter": ok_filter}},                        # id 含空格
            {"v": []},                                             # 实体非对象
            {"v": {"docs": ["a.md"]}},                             # 缺 filter
            {"v": {"filter": ok_filter, "docs": "a.md"}},          # docs 非数组
            {"v": {"filter": ok_filter, "docs": [""]}},            # docs 元素空串
            {"v": {"filter": ok_filter, "docs": ["../e.md"]}},     # docs 元素非法路径
            {"v": {"filter": ok_filter, "extra": 1}},              # 实体未知字段
        )
        for views in bad_views:
            with self.subTest(views=views):
                with self.assertRaises(ToolError):
                    normalize_data({**make_view_data(), "views": views})

    def test_normalize_accepts_all_filter_node_forms(self):
        # 五种表达式树节点均为合法 schema（渲染消费范围是另一回事）
        filters = [
            {"op": "under", "path": "apps"},
            {"op": "tag", "tag": "doc"},
            {"op": "and", "children": [{"op": "under", "path": "apps"}, {"op": "tag", "tag": "doc"}]},
            {"op": "or", "children": [{"op": "under", "path": "apps"}]},
            {"op": "not", "child": {"op": "tag", "tag": "doc"}},
        ]
        for filt in filters:
            with self.subTest(filt=filt):
                out = normalize_data({**make_view_data(), "views": {"v": {"filter": filt, "docs": ["a.md"]}}})
                self.assertEqual(out["views"]["v"]["filter"], filt)

    def test_normalize_rejects_bad_filter_nodes(self):
        bad_filters = (
            "x", [], {}, {"op": "regex", "path": "a"},          # 非对象 / 缺 op / 未知 op
            {"op": "under"},                                    # 缺 path
            {"op": "under", "path": "/abs"},                    # 非法路径
            {"op": "under", "path": "apps", "why": 1},          # 节点未知字段
            {"op": "tag"},                                      # 缺 tag
            {"op": "tag", "tag": ""},                           # 空标签
            {"op": "and", "children": []},                      # 空子节点
            {"op": "and", "children": "x"},                     # children 非数组
            {"op": "not"},                                      # 缺 child
            {"op": "not", "child": {}},                         # child 非法
        )
        for filt in bad_filters:
            with self.subTest(filt=filt):
                with self.assertRaises(ToolError):
                    normalize_data({**make_view_data(), "views": {"v": {"filter": filt}}})

    def test_render_overrides_rejected_bad_structure(self):
        # 二期正式消费：合法结构放行（见 RenderOverridesSchemaTest），仅结构非法在此拦截
        ok_filter = {"op": "under", "path": "apps"}
        bad_overrides = (
            [],                                              # 非对象
            {"apps": []},                                    # 覆盖项非对象
            {"apps": {}},                                    # 覆盖项空对象（无字段即无语义，手改信号）
            {"apps": {"collapsed": True, "why": 1}},         # 未知字段
            {"apps": {"collapsed": "yes"}},                  # 非布尔
            {"apps": {"hidden": 1}},                         # 非布尔
            {"/abs": {"hidden": True}},                      # 非法路径键
            {"a/../b": {"hidden": True}},                    # 非法路径键
        )
        for overrides in bad_overrides:
            with self.subTest(overrides=overrides):
                with self.assertRaises(ToolError):
                    normalize_data({**make_view_data(), "views": {
                        "v": {"filter": ok_filter, "docs": ["a.md"], "render_overrides": overrides}
                    }})

    def test_hand_edited_bad_overrides_blocks_view_add(self):
        # 手改 tree.json 注入结构非法的 render_overrides：view-add 在落盘规范化处被拦截，且保持原子
        tool = self.make_tool(data=make_view_data())
        doc_path = tool.repo_root / "docs" / "a.md"
        doc_path.parent.mkdir(parents=True, exist_ok=True)
        doc_path.write_text("# A\n", encoding="utf-8", newline="\n")
        data = tool.load()
        data["views"] = {"v": {"filter": {"op": "under", "path": "apps"}, "render_overrides": {"apps": {"bogus": True}}}}
        tool.tree_json.write_text(
            json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n",
            encoding="utf-8", newline="\n",
        )
        before = tool.tree_json.read_text(encoding="utf-8")
        with self.assertRaises(ToolError):
            tool.view_add("other", unders=["apps"], doc="docs/a.md")
        self.assertEqual(tool.tree_json.read_text(encoding="utf-8"), before)  # 拒绝保持原子
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, [])  # 无半截历史

    def test_views_sorted_and_docs_deduped(self):
        data = {**make_view_data(), "views": {
            "zeta": {"filter": {"op": "under", "path": "apps"}, "docs": ["b.md", "a.md", "b.md"]},
            "alpha": {"filter": {"op": "under", "path": "apps"}},
        }}
        out = normalize_data(data)
        self.assertEqual(list(out["views"]), ["alpha", "zeta"])  # id 确定性排序
        self.assertEqual(out["views"]["zeta"]["docs"], ["a.md", "b.md"])  # docs 去重排序
        self.assertNotIn("docs", out["views"]["alpha"])  # 空 docs 省略键

    def test_under_path_normalized_to_forward_slashes(self):
        data = {**make_view_data(), "views": {"v": {"filter": {"op": "under", "path": "apps\\ui"}}}}
        out = normalize_data(data)
        self.assertEqual(out["views"]["v"]["filter"], {"op": "under", "path": "apps/ui"})

    def test_empty_views_dropped(self):
        out = normalize_data({**make_view_data(), "views": {}})
        self.assertNotIn("views", out)  # 空 views 不落盘：无配置仓库字节与现状一致


class ViewAddTest(ViewSandboxTest):
    """view-add 基础契约：落盘配置、块渲染、id/锚点/文档校验、拒绝原子性。"""

    def test_view_add_persists_config_and_renders_block(self):
        tool = self.make_view_tool(docs={"docs/ext.md": "# 扩展\n\n正文。\n"})
        tool.view_add("ext", unders=["apps"], doc="docs/ext.md")
        self.assertEqual(tool.load()["views"]["ext"], {
            "filter": {"op": "under", "path": "apps"},
            "docs": ["docs/ext.md"],
        })
        text = self.doc_text(tool, "docs/ext.md")
        begin, end = view_tree_markers("ext")
        self.assertEqual(begin, "<!-- file-tree:tree^id=ext:begin 由脚本渲染，禁止手改 -->")
        self.assertEqual(end, "<!-- file-tree:tree^id=ext:end -->")
        self.assertIn(begin, text)
        self.assertIn(end, text)
        lines = text.split("\n")
        b, e = lines.index(begin), lines.index(end)
        self.assertEqual(lines[b - 1], "```")  # 代码围栏包裹（与默认树块同款）
        self.assertEqual(lines[e + 1], "```")
        self.assertIn("main.tsx # 入口", text)  # 锚点子树正常渲染
        self.assertNotIn("Cargo.toml", text)    # 视图外条目不出现
        self.assertNotIn("file-tree:tags", text)  # 子树视图只有树块、无 tags 块

    def test_view_add_id_validation(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        for bad in ("Ext", "default", "-abc", "_abc", "a b", "a.b", "a" * 65, "", "中文"):
            with self.subTest(view_id=bad):
                with self.assertRaises(ToolError):
                    tool.view_add(bad, unders=["apps"], doc="docs/a.md")
        self.assertNotIn("views", tool.load())  # 拒绝保持原子
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, [])
        for ok in ("a", "1", "a" * 64, "ext-2_0"):  # 合法边界：单字符、64 字符、连字符/下划线
            with self.subTest(view_id=ok):
                tool.view_add(ok, unders=["apps"], doc="docs/a.md")

    def test_view_add_anchor_must_be_existing_dir(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        with self.assertRaises(ToolError):
            tool.view_add("v", unders=["nope"], doc="docs/a.md")  # 不存在
        with self.assertRaises(ToolError):
            tool.view_add("v", unders=["Cargo.toml"], doc="docs/a.md")  # 文件条目
        with self.assertRaises(ToolError):
            tool.view_add("v", unders=["apps/main.tsx"], doc="docs/a.md")  # 深层文件条目
        self.assertNotIn("views", tool.load())

    def test_view_add_requires_existing_doc(self):
        tool = self.make_view_tool()
        with self.assertRaises(ToolError):
            tool.view_add("v", unders=["apps"], doc="docs/missing.md")
        self.assertNotIn("views", tool.load())
        self.assertFalse((tool.repo_root / "docs").exists())  # 不凭空创建文档/目录

    def test_line_requires_doc(self):
        tool = self.make_view_tool()
        with self.assertRaises(ToolError):
            tool.view_add("v", unders=["apps"], line=1)  # --line 依附 --doc
        self.assertNotIn("views", tool.load())

    def test_view_add_without_doc_persists_config_only(self):
        tool = self.make_view_tool()
        tool.view_add("v", unders=["apps"])  # 配置先行：无绑定文档、无渲染产物
        self.assertEqual(tool.load()["views"]["v"], {"filter": {"op": "under", "path": "apps"}})

    def test_view_add_does_not_touch_agents_md(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.render()  # 先渲染默认视图产物
        agents_before = tool.agents_md.read_text(encoding="utf-8")
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        self.assertEqual(tool.agents_md.read_text(encoding="utf-8"), agents_before)  # 数据未变，默认产物不动


class ViewBlockPlacementTest(ViewSandboxTest):
    """块放置契约：--line 的 1-based 落位、上下空行保证、越界拒绝、省略追加尾部。"""

    def numbered_doc(self, n: int) -> str:
        return "\n".join(f"第{i}行" for i in range(1, n + 1)) + "\n"

    def test_line_puts_fence_first_at_nth_line(self):
        tool = self.make_view_tool(docs={"docs/a.md": self.numbered_doc(20)})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=8)
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        self.assertEqual(lines[7], "```")  # 围栏首行落第 8 行（1-based）
        self.assertEqual(lines[8], view_tree_markers("v")[0])
        self.assertEqual(lines[0], "第1行")  # 前面内容原样保留
        self.assertEqual(lines[6], "")      # 块外上方空行（原第 7 行非空则补）

    def test_insert_before_current_nth_line(self):
        # 承诺落点语义：原第 7 行（"第7行"）非空 → 带前导空行插在其前，fence 落第 8 行，
        # 原第 7 行内容完整后移到块（含下方空行）之后
        tool = self.make_view_tool(docs={"docs/a.md": self.numbered_doc(20)})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=8)
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        end_idx = lines.index(view_tree_markers("v")[1])
        self.assertEqual(lines[end_idx + 1], "```")
        self.assertEqual(lines[end_idx + 2], "")  # 块外下方空行
        self.assertEqual(lines[end_idx + 3], "第7行")  # 原第 7 行内容完整后移
        self.assertEqual(lines[end_idx + 4], "第8行")

    def test_no_extra_blank_when_neighbors_blank(self):
        # 原第 N-1 行已是空行：直接插在当前第 N 行前，空行不叠加
        doc = "# 标题\n\n正文二\n\n正文四\n"  # 第 4 行已空，第 5 行 = 正文四
        tool = self.make_view_tool(docs={"docs/a.md": doc})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=5)
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        fence_idx = lines.index("```")
        self.assertEqual(lines[fence_idx], "```")
        self.assertEqual(lines[fence_idx - 1], "")     # 上方空行（原有，未叠加）
        self.assertEqual(lines[fence_idx - 2], "正文二")
        end_idx = lines.index(view_tree_markers("v")[1])
        self.assertEqual(lines[end_idx + 1], "```")
        self.assertEqual(lines[end_idx + 2], "")       # 下方补的空行
        self.assertEqual(lines[end_idx + 3], "正文四")

    def test_nonblank_above_adds_leading_blank(self):
        # 原第 N-1 行非空：插入序列带前导空行、插在其内容之前，fence 仍落第 N 行
        doc = "# 标题\n\n正文二\n正文三\n正文四\n"  # 第 3 行 = 正文二（非空），N=4
        tool = self.make_view_tool(docs={"docs/a.md": doc})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=4)
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        self.assertEqual(lines[3], "```")            # fence 落第 4 行（承诺落点）
        self.assertEqual(lines[2], "")               # 补的前导空行占第 3 行
        end_idx = lines.index(view_tree_markers("v")[1])
        self.assertEqual(lines[end_idx + 2], "")     # 块下方空行
        self.assertEqual(lines[end_idx + 3], "正文二")  # 原第 3 行内容后移

    def test_line_at_document_head(self):
        tool = self.make_view_tool(docs={"docs/a.md": self.numbered_doc(5)})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=1)
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        self.assertEqual(lines[0], "```")  # 第 1 行即围栏首行，上方无需空行
        self.assertEqual(lines[2], "Demo/")  # 块内剪影从全局根名开始
        end_idx = lines.index(view_tree_markers("v")[1])
        self.assertEqual(lines[end_idx + 1], "```")
        self.assertEqual(lines[end_idx + 2], "")  # 块下方空行
        self.assertEqual(lines[-2], "第5行")      # 原内容完整保留
        self.assertEqual(lines[-1], "")          # 末尾规范：恰好一个 LF

    def test_line_out_of_range_rejected(self):
        tool = self.make_view_tool(docs={"docs/a.md": self.numbered_doc(10)})
        original = self.doc_text(tool, "docs/a.md")
        for bad in (0, -1, 12):  # 10 行文档 split 后 11 元素（末尾空行），合法上界 11
            with self.subTest(line=bad):
                with self.assertRaises(ToolError):
                    tool.view_add("v", unders=["apps"], doc="docs/a.md", line=bad)
        self.assertNotIn("views", tool.load())
        self.assertEqual(self.doc_text(tool, "docs/a.md"), original)  # 文档原样

    def test_line_at_last_blank_line_allowed(self):
        tool = self.make_view_tool(docs={"docs/a.md": self.numbered_doc(10)})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=11)  # 末尾空行元素位置
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        self.assertEqual(lines[10], "```")  # fence 落第 11 行（承诺落点）
        end_idx = lines.index(view_tree_markers("v")[1])
        self.assertEqual(lines[end_idx + 2], "")  # 第 10 行非空 → 块下补空行
        self.assertEqual(lines[-2], "第10行")
        self.assertEqual(lines[-1], "")

    def test_no_line_appends_to_tail(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n\n正文"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        begin, _end = view_tree_markers("v")
        b = lines.index(begin)
        self.assertEqual(lines[b - 1], "```")       # fence 紧贴 begin 上方
        self.assertEqual(lines[b - 2], "")          # 块外上方空行
        self.assertEqual(lines[b - 3], "正文")      # 原正文保留
        self.assertEqual(lines[-2], "```")
        self.assertEqual(lines[-1], "")  # 追加尾部保持末尾恰好一个 LF


class ViewBlockHelpersTest(unittest.TestCase):
    """块文本操作纯函数：插入/追加/删除的行级语义（不经完整命令）。"""

    def setUp(self):
        self.begin, self.end = view_tree_markers("x")

    def test_insert_at_line_semantics(self):
        # "l1" 非空 → 带前导空行插在 l1 前：空行占第 1 行、fence 落第 2 行（承诺落点）
        out = insert_block_at_line("l1\nl2\nl3", self.begin, self.end, "c1\nc2", 2)
        self.assertEqual(
            out.split("\n"),
            ["", "```", self.begin, "c1", "c2", self.end, "```", "", "l1", "l2", "l3"],
        )

    def test_insert_keeps_existing_blank_lines(self):
        out = insert_block_at_line("l1\n\nl2", self.begin, self.end, "c", 3)
        self.assertEqual(
            out.split("\n"),
            ["l1", "", "```", self.begin, "c", self.end, "```", "", "l2"],
        )

    def test_insert_out_of_range(self):
        for bad in (0, -1, 4):
            with self.subTest(line=bad):
                with self.assertRaises(ToolError):
                    insert_block_at_line("a\nb\nc", self.begin, self.end, "x", bad)

    def test_append_view_block(self):
        out = append_view_block("# A\n\n正文", self.begin, self.end, "c")
        self.assertEqual(
            out.split("\n"),
            ["# A", "", "正文", "", "```", self.begin, "c", self.end, "```", ""],
        )

    def test_remove_view_block_with_fences_and_blank(self):
        text = "a\n\n```\n" + self.begin + "\nc\n" + self.end + "\n```\n\nb"
        out = remove_view_block(text, self.begin, self.end)
        self.assertEqual(out, "a\n\nb")  # 连带围栏与紧邻上方空行一并删除

    def test_remove_view_block_without_blank_above(self):
        # 围栏上方无空行（文档头）时只删块本身
        text = "```\n" + self.begin + "\nc\n" + self.end + "\n```\nb"
        out = remove_view_block(text, self.begin, self.end)
        self.assertEqual(out, "b")

    def test_remove_missing_markers_raises(self):
        with self.assertRaises(ToolError):
            remove_view_block("nothing", self.begin, self.end)

    def test_remove_view_block_collapses_blank_glue(self):
        # 拼接点收敛：上方残留空行与下方保障空行相接时去其一，删除动作自身不产生双空行粘连
        text = "a\n\n\n```\n" + self.begin + "\nc\n" + self.end + "\n```\n\nb"
        out = remove_view_block(text, self.begin, self.end)
        self.assertEqual(out, "a\n\nb")


class ViewSilhouetteTest(ViewSandboxTest):
    """剪影渲染契约：选中条目正常渲染、未选中祖先仅作路径骨架、首行全局 root 名、hidden/collapsed 语义。"""

    def silhouette(self, tool: TreeTool, *parts: str) -> str:
        anchor = "/".join(parts)
        tree = tool.load()["tree"]
        name, _custom = tool.current_root_name()
        return render_silhouette(name, tree, select_under(tree, anchor))

    def test_single_anchor_silhouette(self):
        # under 选中含锚点自身：apps 是选中条目、正常显示简介（T2 统一模型，不再是 T1 的纯骨架链）
        tool = self.make_view_tool()
        self.assertEqual(
            self.silhouette(tool, "apps"),
            "\n".join([
                "Demo/",
                "└── apps/ # 应用层",
                "    ├── main.tsx # 入口",
                "    └── ui/      # UI 组件",
                "        ├── button.tsx # 按钮",
                "        └── input.tsx  # 输入框",
            ]),
        )

    def test_deep_anchor_skeleton_chain(self):
        # 深锚点：锚点自身选中显示简介，路径上的未选中祖先（apps）才是骨架、无简介
        tool = self.make_view_tool()
        self.assertEqual(
            self.silhouette(tool, "apps", "ui"),
            "\n".join([
                "Demo/",
                "└── apps/",
                "    └── ui/ # UI 组件",
                "        ├── button.tsx # 按钮",
                "        └── input.tsx  # 输入框",
            ]),
        )

    def test_selected_entries_show_desc_skeletons_do_not(self):
        # 骨架 = 未选中的祖先目录：只作容器不显示简介；选中条目（含锚点自身）显示简介
        shallow = self.silhouette(self.make_view_tool(), "apps")
        self.assertIn("应用层", shallow)   # 锚点自身在选中集中，显示简介
        self.assertIn("UI 组件", shallow)  # 子树目录也在选中集中
        deep = self.silhouette(self.make_view_tool(), "apps", "ui")
        self.assertNotIn("应用层", deep)   # apps 未选中，是骨架
        self.assertIn("UI 组件", deep)     # ui 锚点选中，显示简介
        self.assertIn("按钮", deep)

    def test_first_line_is_global_root_name(self):
        tool = self.make_view_tool()
        tree = tool.load()["tree"]
        self.assertTrue(render_silhouette("AnyRoot", tree, select_under(tree, "apps")).startswith("AnyRoot/"))
        tool.set_root("Fixed")
        name, _ = tool.current_root_name()
        self.assertTrue(render_silhouette(name, tree, select_under(tree, "apps")).startswith("Fixed/"))

    def test_view_block_uses_global_root_name(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.set_root("Fixed")
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        text = self.doc_text(tool, "docs/a.md")
        begin, end = view_tree_markers("v")
        block = text[text.index(begin):text.index(end)]
        self.assertIn("Fixed/", block)

    def test_hidden_entries_absent_in_view(self):
        data = make_view_data()
        data["tree"]["apps"]["children"]["main.tsx"]["hidden"] = True
        tool = self.make_tool(data=data)
        tree = tool.load()["tree"]
        out = render_silhouette("Demo", tree, select_under(tree, "apps"))
        self.assertNotIn("main.tsx", out)
        self.assertIn("ui/", out)  # 其余照常

    def test_hidden_anchor_or_ancestor_yields_root_only(self):
        data = make_view_data()
        data["tree"]["apps"]["hidden"] = True
        tool = self.make_tool(data=data)
        tree = tool.load()["tree"]
        self.assertEqual(render_silhouette("Demo", tree, select_under(tree, "apps")), "Demo/")
        self.assertEqual(render_silhouette("Demo", tree, select_under(tree, "apps/ui")), "Demo/")

    def test_collapsed_in_subtree_still_folds(self):
        data = make_view_data()
        data["tree"]["apps"]["children"]["ui"]["collapsed"] = True
        tool = self.make_tool(data=data)
        tree = tool.load()["tree"]
        out = render_silhouette("Demo", tree, select_under(tree, "apps"))
        self.assertIn("ui/…", out)
        self.assertNotIn("button.tsx", out)


class FilterShortcutCompileTest(FilterSandboxTest):
    """CLI 快捷参数编译契约：多锚点并集(or)、锚点×标签交集(and)、排除差集(and+not)，落盘为规范表达式树。"""

    def persisted_filter(self, tool: TreeTool, view_id: str) -> dict:
        return tool.load()["views"][view_id]["filter"]

    def test_single_anchor_compiles_to_bare_under(self):
        # T1 兼容：单锚点无其他维度 = 裸 under 节点（与既有落盘形态逐字节一致）
        tool = self.make_filter_tool()
        tool.view_add("v", unders=["apps"])
        self.assertEqual(self.persisted_filter(tool, "v"), {"op": "under", "path": "apps"})

    def test_multiple_anchors_compile_to_or_union(self):
        # 多锚点为并集：or 包裹；组内排序去重（与 CLI 给出顺序无关的确定性产物）
        tool = self.make_filter_tool()
        tool.view_add("v", unders=["docs", "apps"])
        tool.view_add("w", unders=["apps", "docs", "apps"])  # 重复与乱序同样收敛
        self.assertEqual(self.persisted_filter(tool, "v"), {
            "op": "or",
            "children": [
                {"op": "under", "path": "apps"},
                {"op": "under", "path": "docs"},
            ],
        })
        self.assertEqual(self.persisted_filter(tool, "v"), self.persisted_filter(tool, "w"))

    def test_anchor_times_tags_compile_to_and(self):
        tool = self.make_filter_tool()
        tool.view_add("v", unders=["apps"], tags=["doc"])
        self.assertEqual(self.persisted_filter(tool, "v"), {
            "op": "and",
            "children": [
                {"op": "under", "path": "apps"},
                {"op": "tag", "tag": "doc"},
            ],
        })

    def test_tags_only_compile_to_bare_or_and(self):
        # 仅标签（无锚点）：单标签退化为裸 tag 节点，多标签同为交集
        tool = self.make_filter_tool()
        tool.view_add("v", tags=["doc"])
        self.assertEqual(self.persisted_filter(tool, "v"), {"op": "tag", "tag": "doc"})
        tool.view_add("w", tags=["misc", "doc"])
        self.assertEqual(self.persisted_filter(tool, "w"), {
            "op": "and",
            "children": [
                {"op": "tag", "tag": "doc"},
                {"op": "tag", "tag": "misc"},
            ],
        })

    def test_excludes_compile_to_and_not(self):
        # 排除差集：and + not(under)
        tool = self.make_filter_tool()
        tool.view_add("v", unders=["apps"], excludes=["apps/ui"])
        self.assertEqual(self.persisted_filter(tool, "v"), {
            "op": "and",
            "children": [
                {"op": "under", "path": "apps"},
                {"op": "not", "child": {"op": "under", "path": "apps/ui"}},
            ],
        })

    def test_full_combination_canonical_order(self):
        # 规范 children 顺序：锚点（多锚点 or 包裹）→ 标签 → 排除（not 包裹）；各组内按确定性排序
        tool = self.make_filter_tool()
        tool.view_add("v", unders=["docs", "apps"], tags=["misc", "doc"], excludes=["docs", "apps/ui"])
        self.assertEqual(self.persisted_filter(tool, "v"), {
            "op": "and",
            "children": [
                {"op": "or", "children": [
                    {"op": "under", "path": "apps"},
                    {"op": "under", "path": "docs"},
                ]},
                {"op": "tag", "tag": "doc"},
                {"op": "tag", "tag": "misc"},
                {"op": "not", "child": {"op": "under", "path": "apps/ui"}},
                {"op": "not", "child": {"op": "under", "path": "docs"}},
            ],
        })

    def test_handwritten_equivalent_expression_persists_identically(self):
        # 手写等价（规范形态）表达式与快捷参数编译产物：落盘结果逐字节相同
        shortcut = self.make_filter_tool()
        shortcut.view_add("v", unders=["docs", "apps"], tags=["doc"], excludes=["apps/ui"])
        handwritten = {
            "op": "and",
            "children": [
                {"op": "or", "children": [
                    {"op": "under", "path": "apps"},
                    {"op": "under", "path": "docs"},
                ]},
                {"op": "tag", "tag": "doc"},
                {"op": "not", "child": {"op": "under", "path": "apps/ui"}},
            ],
        }
        manual = self.make_filter_tool()
        manual.view_add("v", filt=handwritten)
        self.assertEqual(
            shortcut.tree_json.read_text(encoding="utf-8"),
            manual.tree_json.read_text(encoding="utf-8"),
        )

    def test_filter_source_mutex(self):
        tool = self.make_filter_tool()
        with self.assertRaises(ToolError):
            tool.view_add("v", unders=["apps"], filt={"op": "tag", "tag": "doc"})
        with self.assertRaises(ToolError):
            tool.view_add("v", tags=["doc"], excludes=["docs"], filt={"op": "tag", "tag": "core"})
        self.assertNotIn("views", tool.load())  # 拒绝保持原子

    def test_no_scope_source_rejected(self):
        # 快捷参数至少要一个范围来源（--under 或 --tag）：纯排除/空参数拒绝
        tool = self.make_filter_tool()
        with self.assertRaises(ToolError):
            tool.view_add("v", excludes=["apps"])
        with self.assertRaises(ToolError):
            tool.view_add("v")
        self.assertNotIn("views", tool.load())


class ViewFilterValidationTest(FilterSandboxTest):
    """引用预检契约：under 引用不存在的路径、tag 引用未登记标签 → view-add 拒绝且原子。"""

    def test_under_ref_must_exist_in_tree(self):
        tool = self.make_filter_tool(docs={"docs/a.md": "# A\n"})
        before = tool.tree_json.read_text(encoding="utf-8")
        doc_before = self.doc_text(tool, "docs/a.md")
        for bad in ("nope", "apps/ghost.tsx"):
            with self.subTest(under=bad):
                with self.assertRaises(ToolError):
                    tool.view_add("v", unders=[bad], doc="docs/a.md")
        self.assertNotIn("views", tool.load())                              # 不落盘
        self.assertEqual(tool.tree_json.read_text(encoding="utf-8"), before)
        self.assertEqual(self.doc_text(tool, "docs/a.md"), doc_before)      # 文档不动
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, [])                                      # 不留撤销历史

    def test_under_ref_must_be_dir_entry(self):
        # 文件条目不是合法锚点（under 为目录锚点前缀语义）
        tool = self.make_filter_tool(docs={"docs/a.md": "# A\n"})
        for bad in ("Cargo.toml", "apps/main.tsx"):
            with self.subTest(under=bad):
                with self.assertRaises(ToolError):
                    tool.view_add("v", unders=[bad], doc="docs/a.md")
        self.assertNotIn("views", tool.load())

    def test_under_ref_checked_anywhere_in_expression(self):
        # 嵌套表达式深处的悬空引用同样拒绝（清单承载的任意组合走同一预检）
        tool = self.make_filter_tool()
        expr = {"op": "or", "children": [
            {"op": "under", "path": "apps"},
            {"op": "not", "child": {"op": "under", "path": "ghost"}},
        ]}
        with self.assertRaises(ToolError):
            tool.view_add("v", filt=expr)
        self.assertNotIn("views", tool.load())

    def test_exclude_ref_validated_too(self):
        # 排除项编译为 not(under) 后同样过引用预检
        tool = self.make_filter_tool()
        with self.assertRaises(ToolError):
            tool.view_add("v", unders=["apps"], excludes=["ghost"])
        self.assertNotIn("views", tool.load())

    def test_tag_ref_must_be_registered(self):
        tool = self.make_filter_tool()
        with self.assertRaises(ToolError):
            tool.view_add("v", tags=["unknown-tag"])
        self.assertNotIn("views", tool.load())

    def test_tag_ref_checked_in_nested_expression(self):
        tool = self.make_filter_tool()
        expr = {"op": "and", "children": [
            {"op": "under", "path": "apps"},
            {"op": "not", "child": {"op": "tag", "tag": "ghost"}},
        ]}
        with self.assertRaises(ToolError):
            tool.view_add("v", filt=expr)
        self.assertNotIn("views", tool.load())


class ViewFilterEvalTest(FilterSandboxTest):
    """求值引擎契约（经 view-add 渲染到文档观测）：五种节点语义、not 全量补集、任意嵌套。"""

    def test_tag_selects_disconnected_entries(self):
        # tag doc 选中：根散文件 README + apps/ui 深处两文件 + docs/guide.md——不连通集合
        tool = self.make_filter_tool(docs={"docs/v.md": "# V\n"})
        tool.view_add("v", tags=["doc"], doc="docs/v.md")
        block = self.rendered_block(tool, "v", "docs/v.md")
        self.assertIn("README.md # 说明", block)
        self.assertIn("guide.md # 指南", block)
        self.assertIn("button.tsx # 按钮", block)
        self.assertIn("input.tsx  # 输入框", block)  # 列对齐：与 button.tsx 行按最宽 stem 对齐
        self.assertNotIn("main.tsx", block)   # core 标签不选
        self.assertNotIn("Cargo.toml", block) # 无标签不选

    def test_and_intersects_anchor_and_tag(self):
        tool = self.make_filter_tool(docs={"docs/v.md": "# V\n"})
        tool.view_add("v", filt={"op": "and", "children": [
            {"op": "under", "path": "apps"},
            {"op": "tag", "tag": "doc"},
        ]}, doc="docs/v.md")
        block = self.rendered_block(tool, "v", "docs/v.md")
        self.assertIn("button.tsx", block)
        self.assertIn("input.tsx", block)
        self.assertNotIn("main.tsx", block)  # apps 内但无 doc 标签
        self.assertNotIn("guide.md", block)  # 有 doc 标签但不在 apps 下
        self.assertNotIn("README.md", block)

    def test_or_unions_anchor_and_tag(self):
        tool = self.make_filter_tool(docs={"docs/v.md": "# V\n"})
        tool.view_add("v", filt={"op": "or", "children": [
            {"op": "under", "path": "docs"},
            {"op": "tag", "tag": "core"},
        ]}, doc="docs/v.md")
        block = self.rendered_block(tool, "v", "docs/v.md")
        self.assertIn("guide.md", block)
        self.assertIn("main.tsx", block)
        self.assertNotIn("button.tsx", block)
        self.assertNotIn("README.md", block)

    def test_not_is_complement_over_full_data(self):
        # not(under apps) = 全量条目（含根散文件与目录）减 apps 子树
        tool = self.make_filter_tool(docs={"docs/v.md": "# V\n"})
        tool.view_add("v", filt={"op": "not", "child": {"op": "under", "path": "apps"}}, doc="docs/v.md")
        block = self.rendered_block(tool, "v", "docs/v.md")
        self.assertIn("Cargo.toml # 根配置", block)
        self.assertIn("README.md  # 说明", block)  # 列对齐：Cargo.toml 行更宽，README 补两空格
        self.assertIn("guide.md # 指南", block)
        self.assertIn("# 文档目录", block)  # docs 目录自身在补集中（选中显示简介）
        self.assertNotIn("main.tsx", block)
        self.assertNotIn("button.tsx", block)

    def test_nested_combination(self):
        # and(or(under apps, under docs), not(tag core))：两目录内除 core 条目外的全部
        tool = self.make_filter_tool(docs={"docs/v.md": "# V\n"})
        tool.view_add("v", filt={"op": "and", "children": [
            {"op": "or", "children": [
                {"op": "under", "path": "apps"},
                {"op": "under", "path": "docs"},
            ]},
            {"op": "not", "child": {"op": "tag", "tag": "core"}},
        ]}, doc="docs/v.md")
        block = self.rendered_block(tool, "v", "docs/v.md")
        self.assertIn("button.tsx", block)
        self.assertIn("input.tsx", block)
        self.assertIn("guide.md", block)
        self.assertIn("ui/", block)
        self.assertNotIn("main.tsx", block)   # core 被 not 排除
        self.assertNotIn("README.md", block)  # 两目录之外
        self.assertNotIn("Cargo.toml", block)

    def test_shortcut_exclusion_renders_as_difference(self):
        # 快捷差集端到端：apps 减 apps/ui 只剩 main.tsx（apps 选中显示简介）
        tool = self.make_filter_tool(docs={"docs/v.md": "# V\n"})
        tool.view_add("v", unders=["apps"], excludes=["apps/ui"], doc="docs/v.md")
        block = self.rendered_block(tool, "v", "docs/v.md")
        self.assertIn("apps/ # 应用层", block)
        self.assertIn("main.tsx # 入口", block)
        self.assertNotIn("ui/", block)

    def test_empty_selection_warns_but_persists(self):
        # 选中 0 条：告警放行、配置落盘、渲染仅剩根名行（check 的空选中告警归后续票）
        import io
        from contextlib import redirect_stdout

        tool = self.make_filter_tool(docs={"docs/v.md": "# V\n"})
        expr = {"op": "and", "children": [
            {"op": "under", "path": "apps"},
            {"op": "tag", "tag": "misc"},  # 无条目同时满足两条件
        ]}
        buf = io.StringIO()
        with redirect_stdout(buf):
            tool.view_add("v", filt=expr, doc="docs/v.md")
        self.assertIn("警告", buf.getvalue())
        self.assertIn("0 条", buf.getvalue())
        self.assertEqual(tool.load()["views"]["v"]["filter"], expr)
        self.assertEqual(self.rendered_block(tool, "v", "docs/v.md").strip(), "Demo/")


class ViewSilhouetteProjectionTest(FilterSandboxTest):
    """不连通选中集的剪影投影：选中投回全树结构、骨架祖先只作容器、空集/hidden/collapsed 形态。"""

    def render(self, tool: TreeTool, selected: set[str], root: str = "Demo") -> str:
        return render_silhouette(root, tool.load()["tree"], selected)

    def test_disconnected_mix_of_files_and_dirs(self):
        # 散文件（README）+ 多目录深处文件（ui 下两条）+ 子目录文件（guide.md）混合投影
        tool = self.make_filter_tool()
        selected = {"apps/ui/button.tsx", "apps/ui/input.tsx", "docs/guide.md", "README.md"}
        self.assertEqual(
            self.render(tool, selected),
            "\n".join([
                "Demo/",
                "├── apps/",  # 骨架：无简介
                "│   └── ui/",  # 骨架
                "│       ├── button.tsx # 按钮",
                "│       └── input.tsx  # 输入框",
                "├── docs/",
                "│   └── guide.md # 指南",
                "└── README.md # 说明",
            ]),
        )

    def test_selected_dir_without_selected_children_has_no_subblock(self):
        # 选中目录（如被 tag 选中）但其子条目不在选中集：目录行显示简介、无子块
        tool = self.make_filter_tool()
        selected = {"docs", "apps/ui/button.tsx"}
        out = self.render(tool, selected)
        self.assertIn("docs/ # 文档目录", out)
        self.assertNotIn("# 应用层", out)  # apps 未选中是骨架，无简介
        self.assertNotIn("guide.md", out)  # docs 的子不在选中集
        self.assertNotIn("input.tsx", out)

    def test_hidden_selected_entries_skipped(self):
        # hidden 选中条目不出现（渲染层统一跳过，隐藏覆盖选中）
        data = make_filter_data()
        data["tree"]["README.md"]["hidden"] = True
        data["tree"]["docs"]["children"]["guide.md"]["hidden"] = True
        tool = self.make_tool(data=data)
        tree = tool.load()["tree"]
        out = render_silhouette("Demo", tree, {"README.md", "docs", "docs/guide.md"})
        self.assertNotIn("README.md", out)
        self.assertNotIn("guide.md", out)
        self.assertIn("docs/ # 文档目录", out)  # docs 自身未 hidden 照常渲染

    def test_hidden_skeleton_ancestor_hides_whole_branch(self):
        # T1 语义保持：骨架链上的 hidden 祖先 → 该分支整体不出现
        data = make_filter_data()
        data["tree"]["apps"]["hidden"] = True
        tool = self.make_tool(data=data)
        tree = tool.load()["tree"]
        out = render_silhouette("Demo", tree, {"apps/ui/button.tsx", "docs/guide.md"})
        self.assertNotIn("apps", out)
        self.assertIn("guide.md", out)

    def test_empty_selection_renders_root_only(self):
        tool = self.make_filter_tool()
        self.assertEqual(self.render(tool, set()), "Demo/")

    def test_collapsed_selected_dir_folds(self):
        # 选中目录 collapsed 生效：折叠不展开（与简版树同语义）
        data = make_filter_data()
        data["tree"]["apps"]["children"]["ui"]["collapsed"] = True
        tool = self.make_tool(data=data)
        tree = tool.load()["tree"]
        out = render_silhouette("Demo", tree, select_under(tree, "apps"))
        self.assertIn("ui/…", out)
        self.assertNotIn("button.tsx", out)

    def test_skeleton_dir_never_folds(self):
        # 骨架目录忽略 collapsed：必须展开到选中后代（折叠会让剪影丢内容）
        data = make_filter_data()
        data["tree"]["apps"]["collapsed"] = True
        tool = self.make_tool(data=data)
        tree = tool.load()["tree"]
        out = render_silhouette("Demo", tree, {"apps/ui/button.tsx"})
        self.assertIn("button.tsx", out)
        self.assertNotIn("apps/…", out)


class RenderOverridesSchemaTest(SandboxTest):
    """render_overrides 的 schema 契约（二期）：合法结构规范化、false 语义值落盘、空集剔除。"""

    def test_normalize_accepts_and_normalizes(self):
        data = {**make_view_data(), "views": {
            "v": {
                "filter": {"op": "under", "path": "apps"},
                "docs": ["a.md"],
                "render_overrides": {
                    "apps\\ui": {"hidden": False, "collapsed": True},   # 路径归一 + 字段定序
                    "apps/main.tsx": {"hidden": True},
                    "Cargo.toml": {"collapsed": False},                  # false 是语义值（双向覆盖），保留落盘
                },
            }
        }}
        out = normalize_data(data)
        self.assertEqual(out["views"]["v"]["render_overrides"], {
            "Cargo.toml": {"collapsed": False},
            "apps/main.tsx": {"hidden": True},
            "apps/ui": {"collapsed": True, "hidden": False},
        })
        # 实体键序：filter → docs → render_overrides
        self.assertEqual(list(out["views"]["v"]), ["filter", "docs", "render_overrides"])

    def test_empty_render_overrides_dropped(self):
        data = {**make_view_data(), "views": {
            "v": {"filter": {"op": "under", "path": "apps"}, "render_overrides": {}}
        }}
        out = normalize_data(data)
        self.assertNotIn("render_overrides", out["views"]["v"])  # 空集剔除（与 docs 空省略键一致）

    def test_paths_sorted_deterministically(self):
        data = {**make_view_data(), "views": {
            "v": {
                "filter": {"op": "under", "path": "apps"},
                "render_overrides": {"apps/ui": {"collapsed": True}, "Cargo.toml": {"hidden": True}},
            }
        }}
        out = normalize_data(data)
        self.assertEqual(list(out["views"]["v"]["render_overrides"]), ["apps/ui", "Cargo.toml"])


class RenderOverridesSilhouetteTest(ViewSandboxTest):
    """剪影渲染消费 render_overrides：优先级 视图覆盖 > 条目全局字段 > 默认值，布尔双向。"""

    def silhouette(self, tool: TreeTool, anchor: str, overrides: dict | None) -> str:
        tree = tool.load()["tree"]
        name, _custom = tool.current_root_name()
        return render_silhouette(name, tree, select_under(tree, anchor), overrides=overrides)

    def test_collapse_selected_dir_folds(self):
        # 覆盖折叠选中目录：目录行带 …、选中后代不渲染（与全局 collapsed 同形态）
        tool = self.make_view_tool()
        out = self.silhouette(tool, "apps", {"apps/ui": {"collapsed": True}})
        self.assertIn("ui/…", out)
        self.assertIn("main.tsx", out)
        self.assertNotIn("button.tsx", out)

    def test_collapse_skeleton_dir_ignored(self):
        # 骨架目录强制展开（T2 规则），collapsed 覆盖不破例：折叠会丢选中后代
        tool = self.make_view_tool()
        out = self.silhouette(tool, "apps/ui", {"apps": {"collapsed": True}})
        self.assertIn("apps/", out)
        self.assertNotIn("apps/…", out)
        self.assertIn("button.tsx", out)  # 骨架链下的选中后代照常

    def test_expand_globally_collapsed_dir(self):
        # 反向覆盖：全局 collapsed=true 的选中目录在本视图展开
        data = make_view_data()
        data["tree"]["apps"]["children"]["ui"]["collapsed"] = True
        tool = self.make_tool(data=data)
        out = self.silhouette(tool, "apps", {"apps/ui": {"collapsed": False}})
        self.assertNotIn("ui/…", out)
        self.assertIn("button.tsx", out)

    def test_hide_selected_entry(self):
        # 正向覆盖：选中文件在本视图隐藏
        tool = self.make_view_tool()
        out = self.silhouette(tool, "apps", {"apps/main.tsx": {"hidden": True}})
        self.assertNotIn("main.tsx", out)
        self.assertIn("button.tsx", out)

    def test_hide_selected_dir_prunes_subtree(self):
        # 正向覆盖选中目录：连同子树整体隐藏（与全局 hidden 同语义）
        tool = self.make_view_tool()
        out = self.silhouette(tool, "apps", {"apps/ui": {"hidden": True}})
        self.assertNotIn("ui/", out)
        self.assertNotIn("button.tsx", out)
        self.assertNotIn("input.tsx", out)
        self.assertIn("main.tsx", out)

    def test_show_globally_hidden_entry(self):
        # 反向覆盖：全局 hidden=true 的选中条目在本视图显示（简介照常）
        data = make_view_data()
        data["tree"]["apps"]["children"]["main.tsx"]["hidden"] = True
        tool = self.make_tool(data=data)
        out = self.silhouette(tool, "apps", {"apps/main.tsx": {"hidden": False}})
        self.assertIn("main.tsx # 入口", out)
        self.assertIn("button.tsx", out)

    def test_show_globally_hidden_dir_reveals_selected_descendants(self):
        # 反向覆盖全局 hidden 目录：目录恢复显示，其选中后代（求值不排 hidden）一并可见
        data = make_view_data()
        data["tree"]["apps"]["children"]["ui"]["hidden"] = True
        tool = self.make_tool(data=data)
        out = self.silhouette(tool, "apps", {"apps/ui": {"hidden": False}})
        self.assertIn("ui/", out)
        self.assertIn("UI 组件", out)
        self.assertIn("button.tsx", out)
        self.assertIn("input.tsx", out)

    def test_globally_hidden_ancestor_prunes_before_descendant_show(self):
        # 逐节点有效 hidden + 祖先优先剪枝：祖先全局 hidden 无覆盖，后代覆盖 show 不生效
        data = make_view_data()
        data["tree"]["apps"]["hidden"] = True
        tool = self.make_tool(data=data)
        out = self.silhouette(tool, "apps", {"apps/main.tsx": {"hidden": False}})
        self.assertEqual(out, "Demo/")  # apps 剪枝整棵子树，main.tsx 的反向覆盖无从生效

    def test_overridden_hidden_ancestor_prunes_before_descendant_show(self):
        # 祖先被视图覆盖隐藏（含子树语义）：后代覆盖 show 不生效
        tool = self.make_view_tool()
        out = self.silhouette(tool, "apps", {
            "apps/ui": {"hidden": True},
            "apps/ui/button.tsx": {"hidden": False},
        })
        self.assertNotIn("ui", out)
        self.assertNotIn("button.tsx", out)

    def test_override_absent_from_projection_no_effect(self):
        # 覆盖不改变选中集：路径不在选中集与骨架中的条目不会因覆盖凭空出现
        tool = self.make_view_tool()
        out = self.silhouette(tool, "apps/ui", {"Cargo.toml": {"hidden": False}, "apps": {"collapsed": True}})
        self.assertNotIn("Cargo.toml", out)  # 视图外条目不因 show 出现
        self.assertIn("button.tsx", out)     # 骨架 apps 的 collapsed 覆盖被忽略（见骨架测试）

    def test_no_override_keeps_phase1_output(self):
        # 零回归锚点：无覆盖（None 或空表）与一期渲染逐字节一致
        tool = self.make_view_tool()
        expected = self.silhouette(tool, "apps", None)
        self.assertEqual(self.silhouette(tool, "apps", {}), expected)
        self.assertIn("ui/", expected)
        self.assertIn("button.tsx", expected)


class RenderOverridesViewAddTest(ViewSandboxTest):
    """view-add 的覆盖配置入口：快捷参数编译、清单、写盘预检、upsert 与视图间隔离。"""

    def test_shortcut_params_compile_to_overrides(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", collapse=["apps/ui"], hide=["apps/main.tsx"])
        self.assertEqual(tool.load()["views"]["v"]["render_overrides"], {
            "apps/main.tsx": {"hidden": True},
            "apps/ui": {"collapsed": True},
        })
        block = self.rendered_block(tool, "v", "docs/a.md")
        self.assertIn("ui/…", block)          # 折叠生效
        self.assertNotIn("main.tsx", block)   # 隐藏生效

    def test_show_and_expand_compile_to_false_values(self):
        data = make_view_data()
        data["tree"]["apps"]["children"]["ui"]["collapsed"] = True
        data["tree"]["apps"]["children"]["main.tsx"]["hidden"] = True
        tool = self.make_tool(data=data)
        tool.view_add("v", unders=["apps"], show=["apps/main.tsx"], expand=["apps/ui"])
        self.assertEqual(tool.load()["views"]["v"]["render_overrides"], {
            "apps/main.tsx": {"hidden": False},
            "apps/ui": {"collapsed": False},
        })

    def test_same_path_two_fields_merge(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], collapse=["apps/ui"], hide=["apps/ui"])
        self.assertEqual(tool.load()["views"]["v"]["render_overrides"], {
            "apps/ui": {"collapsed": True, "hidden": True},
        })

    def test_same_path_conflicting_shortcuts_rejected_atomically(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        with self.assertRaises(ToolError) as ctx:
            tool.view_add("v", unders=["apps"], collapse=["apps/ui"], expand=["apps/ui"])
        self.assertIn("冲突", str(ctx.exception))
        self.assertNotIn("views", tool.load())      # 拒绝保持原子
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, [])              # 无半截历史

    def test_overrides_manifest_and_mutex(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        manifest = {
            "apps/ui": {"collapsed": True},
            "apps/main.tsx": {"hidden": True},
        }
        tool.view_add("v", unders=["apps"], overrides=manifest)
        self.assertEqual(tool.load()["views"]["v"]["render_overrides"], manifest)
        with self.assertRaises(ToolError):
            tool.view_add("w", unders=["apps"], overrides=manifest, hide=["Cargo.toml"])
        self.assertNotIn("w", tool.load().get("views", {}))

    def test_write_precheck_rejects_dangling_and_file_collapse(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        before = tool.tree_json.read_text(encoding="utf-8")
        with self.assertRaises(ToolError) as ctx:
            tool.view_add("v", unders=["apps"], hide=["ghost.md"], doc="docs/a.md")
        self.assertIn("不在树中", str(ctx.exception))
        with self.assertRaises(ToolError) as ctx:
            tool.view_add("v", unders=["apps"], collapse=["apps/main.tsx"], doc="docs/a.md")
        self.assertIn("目录条目", str(ctx.exception))
        self.assertEqual(tool.tree_json.read_text(encoding="utf-8"), before)  # 原子
        self.assertNotIn("file-tree:tree", self.doc_text(tool, "docs/a.md"))  # 块未写
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, [])

    def test_overrides_do_not_leak_other_views_or_default(self):
        # 视图间隔离：覆盖只作用于本视图块，其他视图与默认视图（AGENTS.md 简版树）不受影响
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("va", unders=["apps"], doc="docs/a.md", collapse=["apps/ui"])
        tool.view_add("vb", unders=["apps"], doc="docs/b.md")
        self.assertIn("ui/…", self.rendered_block(tool, "va", "docs/a.md"))
        self.assertNotIn("ui/…", self.rendered_block(tool, "vb", "docs/b.md"))
        self.assertIn("button.tsx", self.rendered_block(tool, "vb", "docs/b.md"))
        brief = tool.render_brief_tree()  # 默认视图渲染不受任何视图覆盖影响
        self.assertIn("button.tsx", brief)
        self.assertIn("main.tsx", brief)
        self.assertNotIn("ui/…", brief)

    def test_data_change_rerender_keeps_overrides(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", collapse=["apps/ui"])
        tool.add("apps/new.tsx", desc="新文件")
        tool.render()  # 数据命令的渲染时机在 CLI 层（写后自动重渲染）
        block = self.rendered_block(tool, "v", "docs/a.md")
        self.assertIn("ui/…", block)      # 覆盖仍生效
        self.assertIn("new.tsx", block)   # 新条目照常进入剪影

    def test_upsert_replaces_and_clears_overrides(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", collapse=["apps/ui"], hide=["apps/main.tsx"])
        tool.view_add("v", unders=["apps"], doc="docs/a.md", hide=["Cargo.toml"])  # upsert 整体替换覆盖
        self.assertEqual(tool.load()["views"]["v"]["render_overrides"], {"Cargo.toml": {"hidden": True}})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")  # 不给覆盖参数 = 移除键
        self.assertNotIn("render_overrides", tool.load()["views"]["v"])
        self.assertIn("button.tsx", self.rendered_block(tool, "v", "docs/a.md"))  # 回到无覆盖形态

    def test_overrides_never_touch_entry_fields(self):
        # 覆盖只存于 views 配置：条目全局字段与查询语义不被改写
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], hide=["apps/main.tsx"], show=["Cargo.toml"])
        node = tool.load()["tree"]["apps"]["children"]["main.tsx"]
        self.assertNotIn("hidden", node)      # 视图隐藏不落条目字段
        self.assertNotIn("hidden", tool.load()["tree"]["Cargo.toml"])
        self.assertIn("main.tsx", tool.render_brief_tree())  # 默认视图不受影响

    def test_undo_rolls_back_overrides(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_add("v", unders=["apps"], doc="docs/a.md", collapse=["apps/ui"])
        tool.undo()
        spec = tool.load()["views"]["v"]
        self.assertNotIn("render_overrides", spec)
        self.assertIn("button.tsx", self.rendered_block(tool, "v", "docs/a.md"))  # 块随回滚恢复展开

    def test_dangling_after_rm_renders_on_and_ops_alive(self):
        # rm 后悬空覆盖是合法中间态（T1 裁定）：渲染静默忽略该项、后续数据操作不被卡死
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", hide=["apps/main.tsx"])
        tool.rm("apps/main.tsx")
        block = self.rendered_block(tool, "v", "docs/a.md")
        self.assertIn("button.tsx", block)   # 其余内容照常渲染
        tool.add("Cargo.lock", desc="锁文件")  # 数据操作不因悬空覆盖失败
        errors, _warnings = tool.check()
        self.assertTrue(any("render_overrides" in e and "不在树中" in e for e in errors))


class RenderOverridesCheckTest(SandboxTest):
    """check 对 render_overrides 的诊断：悬空路径与 collapsed 指向文件为错误；合法覆盖干净通过。"""

    def prepare(self, render_to: str | None = None) -> TreeTool:
        tool = self.make_tool(data=make_view_data())
        tool.view_add("v", unders=["apps"], doc=render_to,
                      overrides={"apps/ui": {"collapsed": True}, "apps/main.tsx": {"hidden": True}})
        tool.render()  # 渲染默认视图标记块（check 产物一致性校验的前提）
        return tool

    def test_check_clean_with_valid_overrides(self):
        tool = self.prepare()
        errors, warnings = tool.check()
        self.assertEqual((errors, warnings), ([], []))

    def test_check_reports_dangling_override_path(self):
        tool = self.prepare()
        tool.rm("apps/ui")
        errors, _warnings = tool.check()
        self.assertTrue(any("render_overrides" in e and "apps/ui" in e for e in errors))

    def test_check_reports_collapsed_on_file_entry(self):
        # 手改把 collapsed 覆盖指向文件条目：结构合法（布尔/字段名都对），check 按语义报错
        tool = self.prepare()
        data = tool.load()
        data["views"]["v"]["render_overrides"] = {"apps/main.tsx": {"collapsed": True}}
        tool.tree_json.write_text(
            json.dumps({"root": "Demo", **data}, ensure_ascii=False, separators=(",", ":")) + "\n",
            encoding="utf-8", newline="\n",
        )
        errors, _warnings = tool.check()
        self.assertTrue(any("collapsed" in e and "目录" in e for e in errors))

    def test_check_reports_hand_edited_bad_structure(self):
        tool = self.prepare()
        data = tool.load()
        data["views"]["v"]["render_overrides"] = {"apps": {"bogus": True}}
        tool.tree_json.write_text(
            json.dumps({"root": "Demo", **data}, ensure_ascii=False, separators=(",", ":")) + "\n",
            encoding="utf-8", newline="\n",
        )
        errors, _warnings = tool.check()
        self.assertTrue(any("结构非法" in e for e in errors))


class CmdRenderOverridesTest(ViewSandboxTest):
    """CLI 层覆盖参数接线：快捷参数（--collapse/--expand/--hide/--show）与 --overrides 清单。"""

    def make_args(self, tool: TreeTool, **kw):
        import types

        base = dict(
            view_id="v", under=["apps"], tag=None, exclude=None, filter=None,
            doc="docs/a.md", line=None, overrides=None,
            collapse=None, expand=None, hide=None, show=None,
        )
        base.update(kw)
        return types.SimpleNamespace(**base)

    def test_cmd_wires_shortcut_params(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        _cmd_view_add(tool, self.make_args(tool, collapse=["apps/ui"], hide=["apps/main.tsx"]))
        self.assertEqual(tool.load()["views"]["v"]["render_overrides"], {
            "apps/main.tsx": {"hidden": True},
            "apps/ui": {"collapsed": True},
        })
        self.assertIn("ui/…", self.rendered_block(tool, "v", "docs/a.md"))

    def test_cmd_wires_show_and_expand(self):
        data = make_view_data()
        data["tree"]["apps"]["children"]["ui"]["collapsed"] = True
        data["tree"]["apps"]["children"]["main.tsx"]["hidden"] = True
        tool = self.make_tool(data=data)
        self.write_doc(tool, "docs/a.md", "# A\n")
        _cmd_view_add(tool, self.make_args(tool, show=["apps/main.tsx"], expand=["apps/ui"]))
        self.assertEqual(tool.load()["views"]["v"]["render_overrides"], {
            "apps/main.tsx": {"hidden": False},
            "apps/ui": {"collapsed": False},
        })

    def test_cmd_wires_overrides_manifest(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        manifest = tool.repo_root / "ovr.json"
        manifest.write_text(
            json.dumps({"overrides": {"apps/ui": {"collapsed": True}}}, ensure_ascii=False),
            encoding="utf-8", newline="\n",
        )
        _cmd_view_add(tool, self.make_args(tool, overrides=str(manifest), collapse=None))
        self.assertEqual(tool.load()["views"]["v"]["render_overrides"], {"apps/ui": {"collapsed": True}})

    def test_cmd_rejects_bad_overrides_manifest(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        cases = {
            "no-key.json": '{"filter": 1}',
            "not-object.json": '["overrides"]',
            "broken.json": '{"overrides": ',
        }
        for name, content in cases.items():
            with self.subTest(manifest=name):
                manifest = tool.repo_root / name
                manifest.write_text(content, encoding="utf-8", newline="\n")
                with self.assertRaises(ToolError):
                    _cmd_view_add(tool, self.make_args(tool, overrides=str(manifest)))
        with self.assertRaises(ToolError):
            _cmd_view_add(tool, self.make_args(tool, overrides=str(tool.repo_root / "ghost.json")))
        self.assertNotIn("views", tool.load())

    def test_cmd_overrides_manifest_mutex_with_shortcuts(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        manifest = tool.repo_root / "ovr.json"
        manifest.write_text('{"overrides": {}}', encoding="utf-8", newline="\n")
        with self.assertRaises(ToolError):
            _cmd_view_add(tool, self.make_args(tool, overrides=str(manifest), hide=["Cargo.toml"]))
        self.assertNotIn("views", tool.load())


class ViewUpsertTest(ViewSandboxTest):
    """view-add 重复执行同 id = upsert：改过滤器、重定位块、替换绑定文档。"""

    def test_upsert_changes_filter_and_refreshes_block(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_add("v", unders=["apps/ui"], doc="docs/a.md")
        self.assertEqual(tool.load()["views"]["v"]["filter"], {"op": "under", "path": "apps/ui"})
        text = self.doc_text(tool, "docs/a.md")
        begin, _end = view_tree_markers("v")
        self.assertEqual(text.count(begin), 1)     # 仍恰一块（不重复追加）
        self.assertNotIn("main.tsx", text)          # 新过滤器生效
        self.assertIn("button.tsx", text)

    def test_upsert_relocates_block(self):
        # 重定位语义：以删除旧块后的文档为基准，围栏首行落基准第 N 行
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n\n" + "\n".join(f"L{i}" for i in range(1, 21)) + "\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=2)
        begin, _end = view_tree_markers("v")
        self.assertEqual(self.doc_text(tool, "docs/a.md").split("\n").count(begin), 1)
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=15)
        text = self.doc_text(tool, "docs/a.md")
        self.assertEqual(text.count(begin), 1)
        self.assertEqual(text.split("\n")[14], "```")  # 基准第 15 行 = 围栏首行

    def test_upsert_relocates_without_line_keeps_position(self):
        # 块已存在且未给 --line：原地替换内容，位置不动
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n\nL1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nL10\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=3)
        before_fence = self.doc_text(tool, "docs/a.md").split("\n").index("```")
        tool.view_add("v", unders=["apps/ui"], doc="docs/a.md")
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        self.assertEqual(lines.index("```"), before_fence)  # 位置不变
        self.assertNotIn("main.tsx", "\n".join(lines))       # 内容已刷新

    def test_upsert_replaces_docs_keeps_old_block(self):
        # 换绑定文档：配置清单替换；旧文档的块保留（孤儿块清理归 view-rm --purge / check）
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_add("v", unders=["apps"], doc="docs/b.md")
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/b.md"])
        begin, _end = view_tree_markers("v")
        self.assertIn(begin, self.doc_text(tool, "docs/a.md"))  # 旧块保留不删
        self.assertIn(begin, self.doc_text(tool, "docs/b.md"))  # 新文档已有块


class ViewDocMirrorTest(ViewSandboxTest):
    """一 id 多文档镜像：view-doc --add 扩充绑定清单，全部文档块内容一致且联动刷新。"""

    def block(self, tool: TreeTool, rel: str, view_id: str = "v") -> str:
        begin, end = view_tree_markers(view_id)
        return block_content(self.doc_text(tool, rel), begin, end)

    def bind_two(self, tool: TreeTool) -> None:
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")

    def test_view_doc_add_mirrors_same_block_content(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md", "docs/b.md"])  # 清单扩充且排序
        self.assertEqual(self.block(tool, "docs/a.md"), self.block(tool, "docs/b.md"))   # 镜像内容一致
        self.assertEqual(self.doc_text(tool, "docs/a.md").count(view_tree_markers("v")[0]), 1)
        self.assertEqual(self.doc_text(tool, "docs/b.md").count(view_tree_markers("v")[0]), 1)

    def test_data_change_refreshes_all_mirrors(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        import types

        args = types.SimpleNamespace(
            path="apps/new.tsx", desc="新增", detail=None, rel=None, tags=None,
            dir=False, collapsed=None, hidden=None, git_ignore=None,
        )
        _cmd_add(tool, args)  # CLI 层：写后自动渲染全部视图 × 全部绑定文档
        self.assertIn("new.tsx", self.block(tool, "docs/a.md"))
        self.assertIn("new.tsx", self.block(tool, "docs/b.md"))

    def test_view_doc_add_line_semantics_same_as_view_add(self):
        # --line 语义与 view-add 完全一致：围栏首行承诺落点、上下空行保证
        doc = "# 标题\n\n" + "\n".join(f"L{i}" for i in range(1, 16)) + "\n"
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": doc})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md", line=4)
        lines = self.doc_text(tool, "docs/b.md").split("\n")
        self.assertEqual(lines[3], "```")  # 围栏首行落第 4 行（承诺落点）
        self.assertEqual(lines[2], "")     # 补的前导空行
        begin, end = view_tree_markers("v")
        e = lines.index(end)
        self.assertEqual(lines[e + 1], "```")
        self.assertEqual(lines[e + 2], "")  # 块下方空行
        self.assertEqual(lines[e + 3], "L1")  # 原第 3 行内容（L1）完整后移
        # 既有绑定文档的块不受 --line 影响：原地刷新、不重定位
        a_lines = self.doc_text(tool, "docs/a.md").split("\n")
        self.assertEqual(a_lines[0], "# A")
        self.assertEqual(a_lines[1], "")   # view-add 追加尾部形态保持
        self.assertEqual(a_lines[2], "```")

    def test_view_doc_add_line_out_of_range_rejected(self):
        doc = "# B\n\nL1\nL2\nL3\n"
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": doc})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        with self.assertRaises(ToolError):
            tool.view_doc("v", add="docs/b.md", line=99)
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])  # 拒绝保持原子
        self.assertNotIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/b.md"))

    def test_view_doc_add_reactivates_orphan_block_in_place(self):
        # 解绑后再重新绑定：孤儿块原地激活（内容刷新为当前渲染产物，位置保留）
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")
        tool.view_doc("v", rm="docs/a.md")  # a 解绑，块保留为孤儿
        begin, _end = view_tree_markers("v")
        fence_idx_before = self.doc_text(tool, "docs/a.md").split("\n").index("```")
        tool.add("apps/late.tsx", desc="后到的")  # 方法层写入，不触发渲染（孤儿块不刷新）
        self.assertNotIn("late.tsx", self.doc_text(tool, "docs/a.md"))
        tool.view_doc("v", add="docs/a.md")  # 重新绑定：孤儿块原地激活
        text = self.doc_text(tool, "docs/a.md")
        self.assertEqual(text.count(begin), 1)
        self.assertIn("late.tsx", self.block(tool, "docs/a.md"))  # 内容已刷新
        self.assertEqual(text.split("\n").index("```"), fence_idx_before)  # 位置保留
        self.assertEqual(self.block(tool, "docs/a.md"), self.block(tool, "docs/b.md"))  # 重新入镜像


class ViewDocAddTest(ViewSandboxTest):
    """view-doc --add 校验契约：视图须存在、文档须存在、重复绑定拒绝、配置先行视图可后补。"""

    def make_bound(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        return tool

    def test_rejects_duplicate_binding(self):
        # 重复绑定报错拒绝（裁定）：--add 语义保持单一"绑定新文档"，重定位走 view-add 同 id upsert
        tool = self.make_bound()
        with self.assertRaises(ToolError) as ctx:
            tool.view_doc("v", add="docs/a.md")
        self.assertIn("已绑定", str(ctx.exception))
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])  # 配置不动
        undo_ops, _ = tool.history_summary()
        self.assertEqual(len(undo_ops), 1)  # 无半截历史（仅 view-add 那步）

    def test_rejects_missing_doc(self):
        tool = self.make_bound()
        with self.assertRaises(ToolError) as ctx:
            tool.view_doc("v", add="docs/missing.md")
        self.assertIn("docs/missing.md", str(ctx.exception))
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])  # 不落盘
        self.assertFalse((tool.repo_root / "docs" / "missing.md").exists())  # 不凭空创建

    def test_rejects_unknown_view(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        with self.assertRaises(ToolError) as ctx:
            tool.view_doc("ghost", add="docs/a.md")
        self.assertIn("ghost", str(ctx.exception))
        self.assertNotIn("views", tool.load())
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, [])

    def test_add_to_config_only_view(self):
        # 配置先行的视图（view-add 未给 --doc）可后补绑定文档
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"])
        self.assertNotIn("docs", tool.load()["views"]["v"])
        tool.view_doc("v", add="docs/a.md")
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/a.md"))

    def test_param_validation(self):
        tool = self.make_bound()
        with self.assertRaises(ToolError):
            tool.view_doc("v")  # --add/--rm 都不给
        with self.assertRaises(ToolError):
            tool.view_doc("v", add="docs/b.md", rm="docs/a.md")  # 同时给
        with self.assertRaises(ToolError):
            tool.view_doc("v", rm="docs/a.md", line=2)  # --line 依附 --add
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])


class ViewDocRmTest(ViewSandboxTest):
    """view-doc --rm：解绑保留块（孤儿不再刷新），剩余镜像照常刷新。"""

    def block(self, tool: TreeTool, rel: str, view_id: str = "v") -> str:
        begin, end = view_tree_markers(view_id)
        return block_content(self.doc_text(tool, rel), begin, end)

    def bind_two(self, tool: TreeTool) -> None:
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")

    def test_rm_unbinds_and_keeps_block(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        tool.view_doc("v", rm="docs/b.md")
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])  # 清单移除
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/b.md"))  # 块保留

    def test_rm_block_stale_after_data_change(self):
        # 解绑后数据变更：孤儿块不再刷新，其余镜像照常刷新
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        tool.view_doc("v", rm="docs/b.md")
        import types

        args = types.SimpleNamespace(
            path="apps/new.tsx", desc="新增", detail=None, rel=None, tags=None,
            dir=False, collapsed=None, hidden=None, git_ignore=None,
        )
        _cmd_add(tool, args)
        self.assertNotIn("new.tsx", self.block(tool, "docs/b.md"))  # 孤儿块不刷新
        self.assertIn("new.tsx", self.block(tool, "docs/a.md"))     # 绑定中的镜像刷新

    def test_rm_refreshes_remaining_mirrors(self):
        # --rm 本身是数据变更：剩余绑定文档的镜像随之刷新（块内容漂移被纠正）
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        begin, end = view_tree_markers("v")
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        lines.insert(lines.index(end), "手改漂移行")  # 模拟 a 块内容被手改
        self.write_doc(tool, "docs/a.md", "\n".join(lines))
        tool.view_doc("v", rm="docs/b.md")
        self.assertNotIn("手改漂移行", self.doc_text(tool, "docs/a.md"))  # 漂移被刷新纠正
        self.assertIn("main.tsx", self.block(tool, "docs/a.md"))

    def test_rejects_unbound_doc(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n", "docs/c.md": "# C\n"})
        self.bind_two(tool)
        with self.assertRaises(ToolError) as ctx:
            tool.view_doc("v", rm="docs/c.md")
        self.assertIn("未绑定", str(ctx.exception))
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md", "docs/b.md"])

    def test_rejects_missing_doc_on_rm(self):
        # 清单在、文档已被删：给出可理解错误（不落盘）
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        (tool.repo_root / "docs" / "b.md").unlink()
        with self.assertRaises(ToolError) as ctx:
            tool.view_doc("v", rm="docs/b.md")
        self.assertIn("docs/b.md", str(ctx.exception))
        self.assertIn("不存在", str(ctx.exception))
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md", "docs/b.md"])  # 不落盘

    def test_rm_last_doc_yields_config_only(self):
        # 解绑最后一个文档：视图退化为配置先行（空 docs 省略键），块保留
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", rm="docs/a.md")
        self.assertEqual(tool.load()["views"]["v"], {"filter": {"op": "under", "path": "apps"}})
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/a.md"))


class ViewDocPathologyTest(ViewSandboxTest):
    """病态拦截：同一文档内同 id 出现多于一个块时，相关命令报错并指明文档与块数，不做猜测性修复。"""

    def duplicate_block(self, tool: TreeTool, rel: str, view_id: str) -> None:
        """把文档中视图块的整段（含围栏）复制一份拼到尾部，构造同 id 双块病态。"""
        text = self.doc_text(tool, rel)
        begin, end = view_tree_markers(view_id)
        lines = text.split("\n")
        b, e = lines.index(begin), lines.index(end)
        block = lines[b - 1 : e + 2]  # 含上下围栏
        self.write_doc(tool, rel, text.rstrip("\n") + "\n\n" + "\n".join(block) + "\n")

    def test_multi_block_rejects_view_doc_add_atomically(self):
        # 病态文档已在绑定清单中：--add 新文档的 dry-run 覆盖全清单，落盘前拦截
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        self.duplicate_block(tool, "docs/a.md", "v")
        with self.assertRaises(ToolError) as ctx:
            tool.view_doc("v", add="docs/b.md")
        msg = str(ctx.exception)
        self.assertIn("docs/a.md", msg)  # 指明文档路径
        self.assertIn("2", msg)          # 指明块数
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])  # 拒绝保持原子
        self.assertNotIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/b.md"))
        undo_ops, _ = tool.history_summary()
        self.assertEqual(len(undo_ops), 1)  # 无半截历史

    def test_multi_block_rejects_view_add_upsert(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        self.duplicate_block(tool, "docs/a.md", "v")
        with self.assertRaises(ToolError) as ctx:
            tool.view_add("v", unders=["apps/ui"], doc="docs/a.md")  # upsert 重定位/改锚点
        msg = str(ctx.exception)
        self.assertIn("docs/a.md", msg)
        self.assertIn("2", msg)
        self.assertEqual(tool.load()["views"]["v"]["filter"], {"op": "under", "path": "apps"})  # 配置不动

    def test_multi_block_rejects_data_command_render(self):
        # 数据命令：写入成功在前、渲染在后——病态阻断渲染报错，数据已落盘；
        # 手改修复病态后重跑 render 即恢复（不做猜测性修复）
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        self.duplicate_block(tool, "docs/a.md", "v")
        import types

        args = types.SimpleNamespace(
            path="apps/new.tsx", desc="新增", detail=None, rel=None, tags=None,
            dir=False, collapsed=None, hidden=None, git_ignore=None,
        )
        with self.assertRaises(ToolError) as ctx:
            _cmd_add(tool, args)
        msg = str(ctx.exception)
        self.assertIn("docs/a.md", msg)
        self.assertIn("2", msg)
        self.assertIn("new.tsx", tool.load()["tree"]["apps"]["children"])  # 数据已写入
        # 修复病态（手改删除多余块）后 render 恢复正常
        begin, end = view_tree_markers("v")
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        first_end = lines.index(end)
        second_begin = lines.index(begin, first_end)
        second_fence_end = lines.index(end, first_end + 1) + 1  # 第二份块的结尾围栏
        repaired = lines[: second_begin - 1] + lines[second_fence_end + 1 :]  # 连同上方空行删掉第二份块
        self.write_doc(tool, "docs/a.md", "\n".join(repaired))
        tool.render()  # 不再抛错
        self.assertEqual(self.doc_text(tool, "docs/a.md").count(begin), 1)
        self.assertIn("new.tsx", self.doc_text(tool, "docs/a.md"))  # 镜像补齐刷新

    def test_multi_block_rejects_render_command(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        self.duplicate_block(tool, "docs/a.md", "v")
        with self.assertRaises(ToolError) as ctx:
            tool.render()
        msg = str(ctx.exception)
        self.assertIn("docs/a.md", msg)
        self.assertIn("2", msg)


class ViewDocUndoTest(ViewSandboxTest):
    """view-doc 撤销：单步历史，undo/redo 连同配置与渲染产物一并回放。"""

    def test_view_doc_add_single_history_step_and_undo(self):
        original_b = "# B\n\n正文段落。\n"
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": original_b})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md", line=2)
        undo_ops, _ = tool.history_summary()
        self.assertEqual(len(undo_ops), 2)
        self.assertIn("view-doc", undo_ops[-1])
        tool.undo()
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])   # 清单恢复
        self.assertEqual(self.doc_text(tool, "docs/b.md"), original_b)       # 渲染产物恢复

    def test_view_doc_add_redo_roundtrip(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md", line=2)
        after = self.doc_text(tool, "docs/b.md")
        tool.undo()
        tool.redo()
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md", "docs/b.md"])
        self.assertEqual(self.doc_text(tool, "docs/b.md"), after)  # 块内容与位置回放

    def test_undo_view_doc_rm_rebinds_and_refreshes(self):
        # 撤销 --rm：清单恢复，解绑时保留的孤儿块重新入镜像刷新（漂移被纠正）
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")
        tool.view_doc("v", rm="docs/b.md")
        begin, end = view_tree_markers("v")
        lines = self.doc_text(tool, "docs/b.md").split("\n")
        lines.insert(lines.index(end), "孤儿期间手改漂移")  # 孤儿期间块内容被手改
        self.write_doc(tool, "docs/b.md", "\n".join(lines))
        tool.undo()  # 撤销 --rm
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md", "docs/b.md"])
        text = self.doc_text(tool, "docs/b.md")
        self.assertEqual(text.count(begin), 1)
        self.assertNotIn("孤儿期间手改漂移", text)  # 块重新激活并刷新到当前数据
        self.assertEqual(
            block_content(text, begin, end),
            block_content(self.doc_text(tool, "docs/a.md"), begin, end),  # 重新入镜像
        )

    def test_redo_view_doc_rm_keeps_orphan_block(self):
        # 重做 --rm：清单再次移除，文档中的块保留为孤儿
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")
        tool.view_doc("v", rm="docs/b.md")
        tool.undo()
        tool.redo()
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/b.md"))  # 块保留


class ViewRmTest(ViewSandboxTest):
    """view-rm 默认语义：仅删视图实体（配置消失），各绑定文档的块原样保留为孤儿。"""

    def bind_two(self, tool: TreeTool) -> None:
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")

    def test_view_rm_removes_config_keeps_blocks(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        tool.view_rm("v")
        self.assertNotIn("views", tool.load())  # 视图实体消失（唯一视图删除后 views 键省略）
        begin, _end = view_tree_markers("v")
        self.assertIn(begin, self.doc_text(tool, "docs/a.md"))  # 各文档的块原样保留
        self.assertIn(begin, self.doc_text(tool, "docs/b.md"))

    def test_kept_blocks_no_longer_refreshed(self):
        # 孤儿块此后不再被任何渲染刷新：数据变更不触碰保留的块
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        tool.view_rm("v")
        import types

        args = types.SimpleNamespace(
            path="apps/new.tsx", desc="新增", detail=None, rel=None, tags=None,
            dir=False, collapsed=None, hidden=None, git_ignore=None,
        )
        _cmd_add(tool, args)  # CLI 层数据命令：写后自动渲染（v 已不在配置中）
        self.assertNotIn("new.tsx", self.doc_text(tool, "docs/a.md"))
        self.assertNotIn("new.tsx", self.doc_text(tool, "docs/b.md"))

    def test_view_rm_keeps_other_views_and_their_blocks(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_add("w", unders=["apps/ui"], doc="docs/b.md")
        tool.view_rm("v")
        self.assertEqual(list(tool.load()["views"]), ["w"])
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/a.md"))  # v 的块保留
        self.assertIn(view_tree_markers("w")[0], self.doc_text(tool, "docs/b.md"))  # w 照常绑定

    def test_rejects_unknown_view(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        with self.assertRaises(ToolError) as ctx:
            tool.view_rm("ghost")
        self.assertIn("ghost", str(ctx.exception))
        undo_ops, _ = tool.history_summary()
        self.assertEqual(undo_ops, [])  # 无半截历史

    def test_config_only_view_purge_noop(self):
        # 配置先行（无绑定文档）的视图：purge 无块可删，仅删配置
        tool = self.make_view_tool()
        tool.view_add("v", unders=["apps"])
        self.assertEqual(tool.view_rm("v", purge=True), 0)
        self.assertNotIn("views", tool.load())

    def test_view_list_no_longer_shows_removed_view(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_add("w", unders=["apps/ui"])
        tool.view_rm("v")
        self.assertEqual([x["id"] for x in tool.view_list()], ["w"])


class ViewRmPurgeTest(ViewSandboxTest):
    """view-rm --purge：按绑定清单逐一删块，前置校验恰好一个（原子拒绝），块外一字不动。"""

    def bind_two(self, tool: TreeTool) -> None:
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")

    def test_purge_removes_blocks_in_all_bound_docs(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        n = tool.view_rm("v", purge=True)
        self.assertEqual(n, 2)  # 返回实际清理的文档块数
        self.assertNotIn("views", tool.load())
        begin, _end = view_tree_markers("v")
        self.assertNotIn(begin, self.doc_text(tool, "docs/a.md"))
        self.assertNotIn(begin, self.doc_text(tool, "docs/b.md"))

    def test_purge_restores_append_tail_document(self):
        # 块追加到尾部（无 --line）：删除后文档还原为插入前形态
        original = "# 扩展说明\n\n前言段落。\n"
        tool = self.make_view_tool(docs={"docs/ext.md": original})
        tool.view_add("v", unders=["apps"], doc="docs/ext.md")
        tool.view_rm("v", purge=True)
        self.assertEqual(self.doc_text(tool, "docs/ext.md"), original)

    def test_purge_restores_mid_document_no_blank_glue(self):
        # 块按行插在文档中部（上下段落间）：删除后段落结构还原，无空行粘连
        original = "# 标题\n\n上段落。\n\n下段落。\n"
        tool = self.make_view_tool(docs={"docs/a.md": original})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=4)
        begin, _end = view_tree_markers("v")
        self.assertIn(begin, self.doc_text(tool, "docs/a.md"))
        tool.view_rm("v", purge=True)
        self.assertEqual(self.doc_text(tool, "docs/a.md"), original)

    def test_purge_atomic_rejection_on_missing_block(self):
        # 某文档块缺失：报错指明文档与现状，不产生半删状态（配置与另一文档的块原样）
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        self.write_doc(tool, "docs/b.md", "# B\n")  # b 的块被手删
        with self.assertRaises(ToolError) as ctx:
            tool.view_rm("v", purge=True)
        msg = str(ctx.exception)
        self.assertIn("docs/b.md", msg)
        self.assertIn("缺", msg)
        self.assertIn("views", tool.load())  # 配置保留
        begin, _end = view_tree_markers("v")
        self.assertIn(begin, self.doc_text(tool, "docs/a.md"))  # a 的块未被动
        undo_ops, _ = tool.history_summary()
        self.assertEqual(len(undo_ops), 2)  # 无半截历史（view-add + view-doc 两步）

    def test_purge_atomic_rejection_on_multiple_blocks(self):
        # 某文档同 id 多块（病态）：报错指明文档与块数，原子拒绝
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        text = self.doc_text(tool, "docs/b.md")
        begin, end = view_tree_markers("v")
        lines = text.split("\n")
        block = lines[lines.index(begin) - 1 : lines.index(end) + 2]
        self.write_doc(tool, "docs/b.md", text.rstrip("\n") + "\n\n" + "\n".join(block) + "\n")
        with self.assertRaises(ToolError) as ctx:
            tool.view_rm("v", purge=True)
        msg = str(ctx.exception)
        self.assertIn("docs/b.md", msg)
        self.assertIn("2", msg)
        self.assertIn("views", tool.load())
        self.assertEqual(self.doc_text(tool, "docs/a.md").count(begin), 1)  # a 未被动

    def test_purge_rejects_missing_doc(self):
        # 绑定文档在磁盘上不存在：可理解报错（与 view-doc --rm 对齐），不落盘
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        (tool.repo_root / "docs" / "b.md").unlink()
        with self.assertRaises(ToolError) as ctx:
            tool.view_rm("v", purge=True)
        msg = str(ctx.exception)
        self.assertIn("docs/b.md", msg)
        self.assertIn("不存在", msg)
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md", "docs/b.md"])  # 配置不动


class ViewRmUndoTest(ViewSandboxTest):
    """view-rm 撤销回滚：单步历史；undo/redo 把配置变更与全部绑定文档的块一并恢复。"""

    def bind_two(self, tool: TreeTool) -> None:
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")

    def test_view_rm_single_history_step(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_rm("v")
        undo_ops, redo_ops = tool.history_summary()
        self.assertEqual((len(undo_ops), redo_ops), (2, []))
        self.assertIn("view-rm", undo_ops[-1])

    def test_undo_view_rm_reactivates_blocks(self):
        # 撤销默认删除：配置恢复，保留的孤儿块重新入渲染（漂移被纠正）
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_rm("v")
        begin, end = view_tree_markers("v")
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        lines.insert(lines.index(end), "孤儿期间手改漂移")
        self.write_doc(tool, "docs/a.md", "\n".join(lines))
        op = tool.undo()
        self.assertIn("view-rm", op)
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md"])  # 配置恢复
        text = self.doc_text(tool, "docs/a.md")
        self.assertNotIn("孤儿期间手改漂移", text)  # 块重新激活并刷新
        self.assertIn("main.tsx", block_content(text, begin, end))

    def test_undo_view_rm_purge_restores_config_and_all_docs(self):
        # 撤销 purge：配置与全部绑定文档的块渲染产物一并恢复到操作前
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n\nA 段落。\n", "docs/b.md": "# B\n\nB 段落。\n"})
        self.bind_two(tool)
        before_a = self.doc_text(tool, "docs/a.md")
        before_b = self.doc_text(tool, "docs/b.md")
        tool.view_rm("v", purge=True)
        self.assertNotIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/a.md"))
        tool.undo()
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md", "docs/b.md"])
        self.assertEqual(self.doc_text(tool, "docs/a.md"), before_a)  # 全文恢复（含块）
        self.assertEqual(self.doc_text(tool, "docs/b.md"), before_b)

    def test_redo_view_rm_purge_removals_all_docs(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        self.bind_two(tool)
        tool.view_rm("v", purge=True)
        purged_a = self.doc_text(tool, "docs/a.md")
        purged_b = self.doc_text(tool, "docs/b.md")
        tool.undo()
        tool.redo()
        self.assertNotIn("views", tool.load())
        self.assertEqual(self.doc_text(tool, "docs/a.md"), purged_a)  # 删块后的形态回放
        self.assertEqual(self.doc_text(tool, "docs/b.md"), purged_b)

    def test_redo_view_rm_keeps_blocks(self):
        # 重做默认删除：配置再次消失，保留的块仍在
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_rm("v")
        tool.undo()
        tool.redo()
        self.assertNotIn("views", tool.load())
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/a.md"))


class ViewListTest(FilterSandboxTest):
    """view-list：id / 过滤器摘要 / 绑定文档数 / 块存在情况。"""

    def test_list_shows_summary(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        views = tool.view_list()
        self.assertEqual(len(views), 1)
        v = views[0]
        self.assertEqual(v["id"], "v")
        self.assertEqual(v["filter"], "under apps")
        self.assertEqual(len(v["docs"]), 1)
        self.assertEqual(v["docs"][0]["doc"], "docs/a.md")
        self.assertTrue(v["docs"][0]["block"])

    def test_list_reports_missing_block(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        self.write_doc(tool, "docs/a.md", "# A\n")  # 模拟块被手删
        v = tool.view_list()[0]
        self.assertFalse(v["docs"][0]["block"])

    def test_list_sorted_by_id_and_empty(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("zeta", unders=["apps"], doc="docs/a.md")
        tool.view_add("alpha", unders=["apps"])
        self.assertEqual([v["id"] for v in tool.view_list()], ["alpha", "zeta"])
        empty = self.make_tool()
        self.assertEqual(empty.view_list(), [])

    def test_compiled_filter_summary_readable(self):
        # 编译产物的摘要能概括锚点/标签/布尔结构（可读单行）
        tool = self.make_filter_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["docs", "apps"], tags=["doc"], excludes=["apps/ui"], doc="docs/a.md")
        summary = tool.view_list()[0]["filter"]
        self.assertEqual(summary, "and(or(under apps, under docs), tag doc, not(under apps/ui))")


class ViewUndoTest(ViewSandboxTest):
    """view-add 撤销：单步历史，undo 后配置与渲染产物一并恢复，redo 完整回放。"""

    def test_view_add_single_history_step(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        undo_ops, redo_ops = tool.history_summary()
        self.assertEqual((len(undo_ops), redo_ops), (1, []))
        self.assertIn("view-add", undo_ops[0])

    def test_undo_restores_config_and_document(self):
        original = "# A\n\n正文段落。\n"
        tool = self.make_view_tool(docs={"docs/a.md": original})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=2)
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/a.md"))
        op = tool.undo()
        self.assertIn("view-add", op)
        self.assertNotIn("views", tool.load())                              # 配置恢复
        self.assertEqual(self.doc_text(tool, "docs/a.md"), original)        # 渲染产物恢复

    def test_redo_restores_view_and_block_position(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n\n" + "\n".join(f"L{i}" for i in range(1, 16)) + "\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md", line=5)
        after = self.doc_text(tool, "docs/a.md")
        tool.undo()
        tool.redo()
        self.assertIn("views", tool.load())                          # 配置回放
        self.assertEqual(self.doc_text(tool, "docs/a.md"), after)    # 块内容与位置回放

    def test_undo_only_last_view_add(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("a1", unders=["apps"], doc="docs/a.md")
        tool.view_add("b1", unders=["apps/ui"], doc="docs/b.md")
        tool.undo()  # 仅撤销 b1
        views = tool.load().get("views", {})
        self.assertEqual(list(views), ["a1"])
        self.assertIn(view_tree_markers("a1")[0], self.doc_text(tool, "docs/a.md"))
        self.assertNotIn(view_tree_markers("b1")[0], self.doc_text(tool, "docs/b.md"))

    def test_data_change_undo_refreshes_view_blocks(self):
        # 视图存在时，数据命令的 undo/redo 联动刷新视图块（自动渲染含全部视图）
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        import types

        args = types.SimpleNamespace(
            path="apps/new.tsx", desc="新增", detail=None, rel=None, tags=None,
            dir=False, collapsed=None, hidden=None, git_ignore=None,
        )
        _cmd_add(tool, args)  # CLI 层：写后自动渲染默认视图 + 全部视图
        self.assertIn("new.tsx", self.doc_text(tool, "docs/a.md"))
        tool.undo()
        self.assertNotIn("new.tsx", self.doc_text(tool, "docs/a.md"))  # 块随数据回滚


class ViewsCompatTest(ViewSandboxTest):
    """兼容契约：无 views / 空 views 与现状一致；默认视图不受视图渲染影响。"""

    def test_no_views_render_touches_agents_only(self):
        tool = self.make_tool()  # 无 views 键
        updated = tool.render()
        self.assertEqual(updated, [tool.agents_md])  # 产物清单与现状一致：仅 AGENTS.md
        self.assertEqual(tool.render(), [])          # 幂等

    def test_default_view_renders_alongside_views(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        updated = tool.render()
        self.assertIn(tool.agents_md, updated)      # 默认视图照常（AGENTS.md 此前未渲染）
        self.assertIn("├── apps/      # 应用层", tool.agents_md.read_text(encoding="utf-8"))
        self.assertIn("`doc`", tool.agents_md.read_text(encoding="utf-8"))  # tags 块照常

    def test_dangling_anchor_render_skips_without_crash(self):
        # 锚点悬空（数据操作合法产物）时渲染跳过该视图，不炸整条渲染管线
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        before = self.doc_text(tool, "docs/a.md")
        tool.rm("apps")  # 锚点悬空
        tool.render()    # 不抛错
        self.assertEqual(self.doc_text(tool, "docs/a.md"), before)  # 块内容保持不动

    def test_missing_bound_doc_render_skips(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        (tool.repo_root / "docs" / "a.md").unlink()  # 绑定文档被删
        tool.render()  # 不抛错、不凭空重建文档
        self.assertFalse((tool.repo_root / "docs" / "a.md").exists())


class CheckViewDiagnosisTest(ViewSandboxTest):
    """check 错误级诊断：绑定文档缺块 / 同 id 多块 / 块内容漂移 / 文档缺失。

    不可渲染视图（锚点悬空等渲染期告警的同类病态）在 check 中降为告警，
    与渲染期两级保持一致，不推翻 T1 已定边界。
    """

    def bind_one(self, tool: TreeTool, rel: str = "docs/a.md") -> None:
        tool.view_add("v", unders=["apps"], doc=rel)

    def test_clean_view_repo_passes(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        self.bind_one(tool)
        tool.render()
        self.assertEqual(tool.check(), ([], []))

    def test_empty_selection_check_warns(self):
        # US19 后半句：空选中视图 view-add 告警放行后，check 也告警（识别"过滤器过窄"）
        # make_view_data 的 doc 标签已登记但无条目使用 → tag 求值为空集（合法过滤器）
        import io
        from contextlib import redirect_stdout

        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        buf = io.StringIO()
        with redirect_stdout(buf):
            tool.view_add("v", filt={"op": "tag", "tag": "doc"}, doc="docs/a.md")
        tool.render()
        self.assertIn("0 条", buf.getvalue())  # view-add 侧告警放行（T2 已交付）
        errors, warnings = tool.check()
        self.assertEqual(errors, [])  # 空集是告警不是错误
        hits = [w for w in warnings if "视图 v" in w and "0 条" in w]
        self.assertEqual(len(hits), 1)

    def test_missing_block_reported(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        self.bind_one(tool)
        tool.render()
        self.write_doc(tool, "docs/a.md", "# A\n（块被手删）\n")
        errors, _ = tool.check()
        view_errors = [e for e in errors if "视图 v" in e]
        self.assertEqual(len(view_errors), 1)  # 只报一条，不重复
        self.assertIn("缺标记块", view_errors[0])
        self.assertIn("docs/a.md", view_errors[0])
        self.assertIn("view-add", view_errors[0])  # 消息给出纠正出路

    def test_orphan_end_marker_reported(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        self.bind_one(tool)
        tool.render()
        begin, _end = view_tree_markers("v")
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        lines.remove(begin)  # 只删开始标记行 → begin 缺失、end 残留
        self.write_doc(tool, "docs/a.md", "\n".join(lines))
        errors, _ = tool.check()
        self.assertTrue(any("孤立结束标记" in e for e in errors))

    def test_multi_block_reported(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        self.bind_one(tool)
        tool.render()
        begin, end = view_tree_markers("v")
        text = append_view_block(self.doc_text(tool, "docs/a.md"), begin, end, "Demo/\n")
        self.write_doc(tool, "docs/a.md", text)
        errors, _ = tool.check()
        hits = [e for e in errors if "同 id" in e]
        self.assertEqual(len(hits), 1)
        self.assertIn("2 个", hits[0])
        self.assertIn("docs/a.md", hits[0])

    def test_content_drift_reported_and_render_heals(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        self.bind_one(tool)
        tool.render()
        begin, end = view_tree_markers("v")
        lines = self.doc_text(tool, "docs/a.md").split("\n")
        lines.insert(lines.index(end), "手改漂移行")  # 块内内容被手改
        self.write_doc(tool, "docs/a.md", "\n".join(lines))
        errors, _ = tool.check()
        hits = [e for e in errors if "视图 v" in e]
        self.assertEqual(len(hits), 1)
        self.assertIn("不一致", hits[0])
        self.assertIn("docs/a.md", hits[0])
        self.assertIn("view-add", hits[0])  # 纠正出路
        tool.render()  # 消息承诺的出路真实有效：重渲染后恢复全绿
        self.assertEqual(tool.check(), ([], []))

    def test_missing_bound_doc_reported(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        self.bind_one(tool)
        tool.render()
        (tool.repo_root / "docs" / "a.md").unlink()
        errors, _ = tool.check()
        hits = [e for e in errors if "视图 v" in e]
        self.assertEqual(len(hits), 1)
        self.assertIn("绑定文档不存在", hits[0])
        self.assertIn("docs/a.md", hits[0])

    def test_unrenderable_view_warns_instead_of_error(self):
        # 锚点悬空（数据命令的合法产物）：块存在性照查，内容比对降为告警（与渲染期两级一致）
        import io
        from contextlib import redirect_stderr

        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        self.bind_one(tool)
        tool.rm("apps")  # 锚点悬空（只写数据不渲染）
        with redirect_stderr(io.StringIO()):
            tool.render()  # 默认视图照常刷新，视图渲染跳过（stderr 告警）
        errors, warnings = tool.check()
        self.assertEqual(errors, [])
        self.assertTrue(any("不可渲染" in w for w in warnings))


class CheckOrphanScanTest(ViewSandboxTest):
    """check 告警级诊断：全仓库孤儿标记块扫描（未登记 id），豁免技能目录与代码围栏内示意行。"""

    def make_orphan(self, tool: TreeTool, rel: str, view_id: str, content: str = "Demo/\n") -> str:
        begin, end = view_tree_markers(view_id)
        text = append_view_block(self.doc_text(tool, rel), begin, end, content)
        self.write_doc(tool, rel, text)
        return text

    def test_orphan_block_reported_with_file_and_line(self):
        tool = self.make_view_tool(docs={"docs/g.md": "# G\n"})
        tool.view_add("v", unders=["apps"], doc="docs/g.md")
        tool.render()
        text = self.make_orphan(tool, "docs/g.md", "typo-id")
        begin, _end = view_tree_markers("typo-id")
        line_no = text.split("\n").index(begin) + 1
        errors, warnings = tool.check()
        self.assertEqual(errors, [])
        hits = [w for w in warnings if "typo-id" in w]
        self.assertEqual(len(hits), 1)
        self.assertIn(f"docs/g.md:{line_no}", hits[0])  # 文件与行号定位

    def test_orphan_warning_fails_strict(self):
        tool = self.make_view_tool(docs={"docs/g.md": "# G\n"})
        tool.view_add("v", unders=["apps"], doc="docs/g.md")
        tool.render()
        self.make_orphan(tool, "docs/g.md", "typo-id")
        errors, warnings = tool.check(strict=True)
        self.assertEqual(warnings, [])
        self.assertTrue(any("typo-id" in e and "E(strict)" in e for e in errors))

    def test_registered_id_kept_block_not_flagged(self):
        # view-doc --rm 的合法产物：解绑保留的块（id 仍登记）不误报孤儿、不报缺块
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")
        tool.view_doc("v", rm="docs/b.md")
        tool.render()
        self.assertEqual(tool.check(), ([], []))

    def test_bare_marker_line_reported(self):
        # 无围栏包裹的裸标记行（手抄半截）同样命中扫描
        tool = self.make_view_tool(docs={"docs/t.md": "# T\n"})
        tool.render()
        begin, end = view_tree_markers("ghost")
        self.write_doc(tool, "docs/t.md", f"# T\n\n{begin}\n{end}\n")
        errors, warnings = tool.check()
        self.assertEqual(errors, [])
        self.assertTrue(any("ghost" in w for w in warnings))

    def test_skill_dir_exempt(self):
        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.render()
        begin, end = view_tree_markers("ghost")
        note = tool.tree_json.parent / "NOTE.md"
        note.write_text(f"# 技能内部说明\n\n```\n{begin}\n内容示意\n{end}\n```\n", encoding="utf-8")
        _errors, warnings = tool.check()
        self.assertFalse(any("ghost" in w for w in warnings))

    def test_fenced_example_lines_exempt(self):
        # 代码围栏内的示意标记行（非紧贴围栏首行）不误报
        tool = self.make_view_tool(docs={"docs/t.md": "# T\n"})
        tool.render()
        begin, end = view_tree_markers("demo")
        doc = (
            "# T\n\n示例：\n\n```\n"
            "标记行格式如下：\n"
            f"{begin}\n"
            f"{end}\n"
            "```\n"
        )
        self.write_doc(tool, "docs/t.md", doc)
        errors, warnings = tool.check()
        self.assertEqual((errors, warnings), ([], []))

    def test_no_views_repo_still_scans(self):
        # 无 views 配置的仓库同样扫描孤儿块（只增告警能力，不改变错误级行为）
        tool = self.make_tool()
        tool.render()
        begin, end = view_tree_markers("ghost")
        docs_dir = tool.repo_root / "docs"
        docs_dir.mkdir()
        (docs_dir / "t.md").write_text(f"# T\n\n```\n{begin}\nDemo/\n{end}\n```\n", encoding="utf-8")
        errors, warnings = tool.check()
        self.assertEqual(errors, [])
        self.assertTrue(any("ghost" in w for w in warnings))


class CmdViewTest(FilterSandboxTest):
    """CLI 层 view-add / view-list：参数接线（快捷参数/清单文件）与输出。"""

    def test_cmd_view_add_wires_args(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_filter_tool(docs={"docs/a.md": "# A\n"})
        args = types.SimpleNamespace(
            view_id="v", under=["apps"], tag=None, exclude=None, filter=None, doc="docs/a.md", line=None,
            overrides=None, collapse=None, expand=None, hide=None, show=None,
        )
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_view_add(tool, args)
        self.assertEqual(tool.load()["views"]["v"]["filter"], {"op": "under", "path": "apps"})
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/a.md"))
        self.assertIn("v", buf.getvalue())

    def test_cmd_view_add_wires_shortcut_combo(self):
        import types

        tool = self.make_filter_tool(docs={"docs/a.md": "# A\n"})
        args = types.SimpleNamespace(
            view_id="v", under=["docs", "apps"], tag=["doc"], exclude=["apps/ui"],
            filter=None, doc="docs/a.md", line=None,
            overrides=None, collapse=None, expand=None, hide=None, show=None,
        )
        _cmd_view_add(tool, args)
        self.assertEqual(tool.load()["views"]["v"]["filter"], {
            "op": "and",
            "children": [
                {"op": "or", "children": [{"op": "under", "path": "apps"}, {"op": "under", "path": "docs"}]},
                {"op": "tag", "tag": "doc"},
                {"op": "not", "child": {"op": "under", "path": "apps/ui"}},
            ],
        })

    def test_cmd_view_add_wires_filter_manifest(self):
        import types

        tool = self.make_filter_tool(docs={"docs/a.md": "# A\n"})
        manifest = tool.repo_root / "filter.json"
        manifest.write_text(
            json.dumps({"filter": {"op": "tag", "tag": "core"}}, ensure_ascii=False),
            encoding="utf-8", newline="\n",
        )
        args = types.SimpleNamespace(
            view_id="v", under=None, tag=None, exclude=None,
            filter=str(manifest), doc="docs/a.md", line=None,
            overrides=None, collapse=None, expand=None, hide=None, show=None,
        )
        _cmd_view_add(tool, args)
        self.assertEqual(tool.load()["views"]["v"]["filter"], {"op": "tag", "tag": "core"})
        self.assertIn("main.tsx", self.doc_text(tool, "docs/a.md"))  # 清单表达式已渲染生效

    def test_cmd_view_add_rejects_bad_manifest(self):
        import types

        tool = self.make_filter_tool(docs={"docs/a.md": "# A\n"})
        cases = {
            "no-filter-key.json": '{"entries": 1}',
            "not-object.json": '["filter"]',
            "broken.json": '{"filter": ',
        }
        for name, content in cases.items():
            with self.subTest(manifest=name):
                manifest = tool.repo_root / name
                manifest.write_text(content, encoding="utf-8", newline="\n")
                args = types.SimpleNamespace(
                    view_id="v", under=None, tag=None, exclude=None,
                    filter=str(manifest), doc="docs/a.md", line=None,
                )
                with self.assertRaises(ToolError):
                    _cmd_view_add(tool, args)
        missing = types.SimpleNamespace(
            view_id="v", under=None, tag=None, exclude=None,
            filter=str(tool.repo_root / "ghost.json"), doc="docs/a.md", line=None,
        )
        with self.assertRaises(ToolError):
            _cmd_view_add(tool, missing)
        self.assertNotIn("views", tool.load())

    def test_cmd_view_add_passes_line(self):
        import types

        tool = self.make_view_tool(docs={"docs/a.md": "# A\n\nL1\nL2\nL3\n"})
        args = types.SimpleNamespace(
            view_id="v", under=["apps"], tag=None, exclude=None, filter=None, doc="docs/a.md", line=2,
            overrides=None, collapse=None, expand=None, hide=None, show=None,
        )
        _cmd_view_add(tool, args)
        self.assertEqual(self.doc_text(tool, "docs/a.md").split("\n")[1], "```")

    def test_cmd_view_list_output(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_add("w", unders=["apps/ui"], doc="docs/b.md")
        self.write_doc(tool, "docs/b.md", "# B\n")  # w 的块缺失
        args = types.SimpleNamespace()
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_view_list(tool, args)
        out = buf.getvalue()
        self.assertIn("共 2 个视图", out)
        self.assertIn("v", out)
        self.assertIn("under apps", out)
        self.assertIn("under apps/ui", out)
        self.assertIn("docs/a.md", out)
        self.assertIn("块存在", out)
        self.assertIn("块缺失", out)

    def test_cmd_view_list_empty(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_tool()
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_view_list(tool, types.SimpleNamespace())
        self.assertIn("无视图", buf.getvalue())


class CmdViewDocTest(ViewSandboxTest):
    """CLI 层 view-doc：参数接线与输出。"""

    def test_cmd_view_doc_add_wires_args(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        args = types.SimpleNamespace(view_id="v", add="docs/b.md", rm=None, line=None)
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_view_doc(tool, args)
        self.assertEqual(tool.load()["views"]["v"]["docs"], ["docs/a.md", "docs/b.md"])
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/b.md"))
        self.assertIn("docs/b.md", buf.getvalue())
        self.assertIn("单步历史", buf.getvalue())

    def test_cmd_view_doc_add_passes_line(self):
        import types

        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n\nL1\nL2\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        args = types.SimpleNamespace(view_id="v", add="docs/b.md", rm=None, line=2)
        _cmd_view_doc(tool, args)
        self.assertEqual(self.doc_text(tool, "docs/b.md").split("\n")[1], "```")

    def test_cmd_view_doc_rm_output(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")
        args = types.SimpleNamespace(view_id="v", add=None, rm="docs/b.md", line=None)
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_view_doc(tool, args)
        out = buf.getvalue()
        self.assertIn("已解绑", out)
        self.assertIn("docs/b.md", out)
        self.assertIn("保留", out)  # 提示块保留为孤儿
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/b.md"))


class CmdViewRmTest(ViewSandboxTest):
    """CLI 层 view-rm：参数接线与输出。"""

    def test_cmd_view_rm_default_output(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_view_tool(docs={"docs/a.md": "# A\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        args = types.SimpleNamespace(view_id="v", purge=False)
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_view_rm(tool, args)
        out = buf.getvalue()
        self.assertIn("v", out)
        self.assertIn("保留", out)  # 提示块保留为孤儿
        self.assertIn("单步历史", out)
        self.assertNotIn("views", tool.load())
        self.assertIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/a.md"))

    def test_cmd_view_rm_purge_output(self):
        import io
        import types
        from contextlib import redirect_stdout

        tool = self.make_view_tool(docs={"docs/a.md": "# A\n", "docs/b.md": "# B\n"})
        tool.view_add("v", unders=["apps"], doc="docs/a.md")
        tool.view_doc("v", add="docs/b.md")
        args = types.SimpleNamespace(view_id="v", purge=True)
        buf = io.StringIO()
        with redirect_stdout(buf):
            _cmd_view_rm(tool, args)
        out = buf.getvalue()
        self.assertIn("2", out)  # 清理了 2 个文档的块
        self.assertNotIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/a.md"))
        self.assertNotIn(view_tree_markers("v")[0], self.doc_text(tool, "docs/b.md"))


class SelfHostTest(unittest.TestCase):
    """自举冒烟：本技能自身的 tree.json 应通过 check（规范形态）。"""

    def test_self_check(self):
        skill_dir = Path(__file__).resolve().parents[1]
        repo_root = skill_dir.parents[2]
        tool = TreeTool(
            tree_json=skill_dir / "tree.json",
            agents_md=repo_root / "AGENTS.md",
            repo_root=repo_root,
            root_name=repo_root.name,
            history_path=default_history_path(repo_root, skill_dir),
        )
        if not tool.tree_json.exists():
            self.skipTest("tree.json 尚未迁移")
        errors, _ = tool.check()
        self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
