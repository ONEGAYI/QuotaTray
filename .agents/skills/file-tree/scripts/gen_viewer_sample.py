"""查看器大样本生成器：合成指定文件数的 tree.json（性能实测用，G14）。

用法:
    python gen_viewer_sample.py --files 10000 --out sample_10k.json
    python gen_viewer_sample.py --files 100000 --out sample_100k.json

产物是结构合法的快照（可直接交给 viewer.py 浏览），特征覆盖：
- 中文目录与中文描述（src源码、文档中心、性能样本 等）
- tags（doc/script/test/core）与 rel（正向关联 + 周期性悬空目标）
- hidden / collapsed / git-ignore 三态标志穿插
- 决定性输出：同一 --files/--subdirs 参数生成逐字节相同的样本，可复现

样本本身不入库；本脚本作为 fixture 工具随技能源码入库。
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# 顶层目录布局（含 3 个中文目录，覆盖 Unicode 路径与搜索）
TOP_DIRS = [
    "src",
    "docs",
    "tests",
    "tools",
    "源码库",
    "文档中心",
    "性能样本",
    "integration",
    "examples",
    "benchmarks",
]
TAGS = {
    "doc": "说明文档",
    "script": "维护脚本",
    "test": "契约测试",
    "core": "核心模块",
}
FILES_PER_SUBDIR = 10  # 每个子目录固定 10 个文件（10 万 = 10 顶层 × 1000 子目录）


def _file_entry(top: str, sub: int, idx: int, prev_path: str | None) -> dict:
    """构造一个文件条目：desc/detail/rel/tags 按下标规律分布，保证可搜索。"""
    seq = sub * FILES_PER_SUBDIR + idx
    entry: dict = {
        "kind": "file",
        "desc": f"{top} 模块 {sub:04d} 组第 {idx} 号文件（序号 {seq:06d}）",
    }
    if seq % 7 == 0:
        entry["detail"] = [f"{top} 性能样本说明第一行（{seq:06d}）", "第二行：虚拟化滚动覆盖用"]
    if seq % 3 == 0:
        entry["tags"] = ["test"] if seq % 6 == 0 else ["doc"]
    if seq % 5 == 0:
        entry["tags"] = [*entry.get("tags", []), "script"] if seq % 10 else ["script", "core"]
    # rel：每目录第 0 个文件指向同目录前一文件；每 97 个出现一个悬空目标
    if idx == 0 and prev_path is not None:
        entry["rel"] = [prev_path]
    if seq % 97 == 13:
        entry.setdefault("rel", []).append(f"{top}/悬空目标_{seq}.gone")
    if seq % 53 == 7:
        entry["hidden"] = True
    if seq % 71 == 11:
        entry["git-ignore"] = seq % 142 == 11
    return entry


def build_tree(files: int, subdirs: int | None) -> dict:
    """构造完整快照数据：每顶层 subdirs 个子目录 × 10 文件 + 顶层散件。"""
    if subdirs is None:
        # 由目标文件数反推：均摊到 10 个顶层（向下取整，误差 ≤ 10 文件）
        subdirs = max(1, files // (len(TOP_DIRS) * FILES_PER_SUBDIR))
    tree: dict = {}
    for top in TOP_DIRS:
        children: dict = {}
        for sub in range(subdirs):
            sub_name = f"mod{sub:04d}"
            sub_children: dict = {}
            prev_path: str | None = None
            for idx in range(FILES_PER_SUBDIR):
                name = f"file{sub * FILES_PER_SUBDIR + idx:06d}.ts"
                sub_children[name] = _file_entry(top, sub, idx, prev_path)
                prev_path = f"{top}/{sub_name}/{name}"
            sub_dir: dict = {"kind": "dir", "desc": f"{top} 子模块 {sub:04d}", "children": sub_children}
            if sub % 101 == 17:
                sub_dir["collapsed"] = True
            children[sub_name] = sub_dir
        tree[top] = {"kind": "dir", "desc": f"{top} 顶层目录（性能样本）", "children": children}
    tree["README.md"] = {
        "kind": "file",
        "desc": "性能样本说明：由 gen_viewer_sample.py 合成",
        "detail": ["合成快照，仅用于加载/搜索/刷新/滚动性能实测。"],
        "tags": ["doc"],
        "rel": ["src/mod0000/file000000.ts"],
    }
    tree[".gitignore"] = {"kind": "file", "desc": "样本忽略规则", "git-ignore": False}
    tree["空目录"] = {"kind": "dir", "desc": "空目录样本", "children": {}}
    return {"root": "性能样本仓库", "tags": TAGS, "tree": tree}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="gen_viewer_sample.py",
        description="合成查看器性能实测用的大样本 tree.json（决定性、含中文/rel/标签）",
    )
    parser.add_argument("--files", type=int, default=10000, help="目标文件数（默认 1 万，按结构向下取整）")
    parser.add_argument("--subdirs", type=int, default=None, help="每顶层子目录数（默认由 files 反推）")
    parser.add_argument("--out", type=Path, required=True, help="输出 tree.json 路径（建议不入库）")
    args = parser.parse_args(argv)

    if args.files <= 0:
        print("--files 必须为正整数", file=sys.stderr)
        return 2

    data = build_tree(args.files, args.subdirs)
    total = sum(1 for _ in _iter_paths(data["tree"]))
    text = json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n"
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8", newline="\n")
    print(
        f"已生成 {args.out}：{total} 条目（约 {args.out.stat().st_size / 1e6:.1f} MB，"
        f"目标文件数 {args.files}，subdirs={args.subdirs or '自动'}）"
    )
    return 0


def _iter_paths(tree: dict):
    """迭代全部条目（计数用）。"""
    stack = [(tree, "")]
    while stack:
        node, prefix = stack.pop()
        for name, child in node.items():
            path = f"{prefix}/{name}" if prefix else name
            yield path
            if isinstance(child.get("children"), dict):
                stack.append((child["children"], path))


if __name__ == "__main__":
    sys.exit(main())
