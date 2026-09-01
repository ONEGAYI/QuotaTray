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
    _cmd_mv,
    _cmd_mv_batch,
    _cmd_query,
    _cmd_rm_batch,
    _cmd_root,
    default_history_path,
    normalize_data,
    replace_block,
    resolve_git_dir,
    sort_key,
    split_rel_path,
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


class SandboxTest(unittest.TestCase):
    """基类：为每个用例搭临时沙箱并返回配置好的 TreeTool。"""

    def make_tool(self, data: dict | None = None, git_files: set[str] | None = None, history_limit: int = 20) -> TreeTool:
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
            collapsed=None, hidden=None,
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
        args = types.SimpleNamespace(kw=None, tag=None, rel_of=None, json=True)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            _cmd_query(tool, args)
        payload = json.loads(buf.getvalue())
        by_path = {e["path"]: e for e in payload}
        self.assertEqual(by_path["apps"]["kind"], "dir")
        self.assertEqual(by_path["Cargo.toml"]["kind"], "file")

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

    def test_get_missing_raises(self):
        tool = self.make_tool()
        with self.assertRaises(ToolError):
            tool.get("nope.rs")


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
