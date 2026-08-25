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
        # 字段固定顺序：desc, detail, rel, tags, children
        keys = list(out["tree"]["a.rs"])
        self.assertEqual(keys, ["desc", "detail", "children"])

    def test_field_order_canonical(self):
        node = {"children": {}, "tags": ["t"], "rel": ["a.rs"], "detail": ["d"], "desc": "x"}
        out = normalize_data({"tags": {"t": "说明"}, "tree": {"n": node}})
        self.assertEqual(list(out["tree"]["n"]), ["desc", "detail", "rel", "tags", "children"])


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
        self.assertEqual(tool.get("logs"), {"desc": "日志", "children": {}})

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
            path="apps/new.ts", desc="新文件", detail=None, rel=None, tags=tags, dir=False
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
