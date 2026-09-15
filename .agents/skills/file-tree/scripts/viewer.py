"""独立快照的只读文件树查看器：标准库 HTTP 服务 + 前端静态资源托管。

用法:
    python viewer.py <tree.json 路径> [--port N] [--host H]

- 默认绑定 127.0.0.1（G20）：只提供查看器页面资源与快照查询，
  不把任意源码目录作为静态目录暴露；远端访问请自行建立 SSH 隧道。
- 启动后打印访问地址，按 Ctrl+C 停止。
- 绝对只读（G04）：无任何数据写入口，不触发格式转换、不生成撤销历史、
  不重渲染 AGENTS.md；快照在内存中持有（G13）。
- 手动刷新（G12）：POST /api/refresh 从同一路径重读快照并原子替换
  （数据与索引一起换代，世代号 +1）；新快照非法时明确报错并保留旧数据，
  绝不写回或修改源文件。替换 tree.json 后刷新即可，无需重启或重新构建。
- 快照独立（G02）：tree.json 可位于仓库之外，无需源码、.git 或 AGENTS.md。
"""

from __future__ import annotations

import argparse
import json
import mimetypes
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, unquote, urlsplit

sys.path.insert(0, str(Path(__file__).parent))

from viewer_core import DEFAULT_PAGE_SIZE, Snapshot, ViewerError  # noqa: E402

SKILL_ROOT = Path(__file__).resolve().parents[1]
# 受控发行静态资源（G18）：随技能入库与部署——母体位于 dist/viewer/，
# 部署实例位于 <目标>/.agents/skills/file-tree/viewer/，运行只需 Python，
# 不依赖 Node、npm install 或前端构建
DEFAULT_STATIC_DIR = SKILL_ROOT / "viewer"
DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 8618
# 前端构建指引（#23 审查 Standards-3）：与 scripts/deploy.py、
# scripts/assemble_viewer.py 的同名常量逐字一致（deploy_test.py 锁定）；
# 跨包 import 不可行——部署实例只携带 dist 文件。
BUILD_GUIDE = "cd frontend && npm install && npm run build"


def build_guide_pre_html() -> str:
    """503 降级页的多行构建指引（由 BUILD_GUIDE 派生，不另写一份文案）。"""
    return "<pre>" + "\n".join(BUILD_GUIDE.split(" && ")) + "</pre>"
# 启动门槛（G21/G22/G23）：低于该版本拒绝启动并给可理解错误。
# 这是必要条件而非兼容承诺——实际支持范围以文档中的实测矩阵为准，
# 未实测的版本不宣称支持；门槛只随"确认需要更高特性"而升，不凭空抬高。
MIN_PYTHON = (3, 8)

MIME_TYPES = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".json": "application/json; charset=utf-8",
    ".svg": "image/svg+xml",
    ".png": "image/png",
    ".ico": "image/x-icon",
    ".txt": "text/plain; charset=utf-8",
    ".map": "application/json",
    ".woff": "font/woff",
    ".woff2": "font/woff2",
}


class ViewerServer(ThreadingHTTPServer):
    """持有内存快照与静态资源目录的服务；请求线程为 daemon，随主进程退出。

    快照版本管理（G12"同一快照版本"）：
    - 世代号 generation 从 1 起，每次成功刷新 +1；查询响应统一盖章，
      前端据此识别回答来自哪个版本、绝不混用；
    - 刷新 = 从启动时指定的同一路径重读构造新 Snapshot（锁外，读失败
      保留旧快照），随后仅持锁原子替换引用并递增世代号——替换是单次
      赋值，任何请求要么全用旧快照、要么全用新快照；
    - 每个请求经 current() 一次取 (快照, 世代号) 配对引用，保证响应
      内容与世代号永远一致。
    """

    daemon_threads = True

    def __init__(
        self,
        address: tuple[str, int],
        handler: type[BaseHTTPRequestHandler],
        tree_json: Path,
        snapshot: Snapshot,
        static_dir: Path,
        quiet: bool = False,
        host_check: bool = False,
    ):
        super().__init__(address, handler)
        self.tree_json = tree_json  # 刷新始终重读这一路径（G12：同一路径替换）
        self.static_dir = static_dir
        self.quiet = quiet
        # 默认绑定（127.0.0.1）时开启 Host 头校验（#23 审查 C2，防 DNS
        # rebinding 读取本地快照）；显式 --host 自定义绑定视为用户自行开放
        # 网络暴露，跳过校验（文档另有 SSH 隧道方案）
        self.host_check = host_check
        self._snapshot = snapshot
        self._generation = 1
        self._lock = threading.Lock()

    @property
    def snapshot(self) -> Snapshot:
        snapshot, _ = self.current()
        return snapshot

    @property
    def generation(self) -> int:
        with self._lock:
            return self._generation

    def current(self) -> tuple[Snapshot, int]:
        """取当前配对版本：同一请求内先调用一次，之后全用这份引用。"""
        with self._lock:
            return self._snapshot, self._generation

    def refresh_snapshot(self) -> tuple[Snapshot, int]:
        """重读同一路径快照；成功才原子替换并递增世代号，失败原样保留。

        构造新 Snapshot 在锁外进行：读文件/建索引期间查询继续走旧快照，
      不被阻塞；并发刷新时后完成者胜，世代号仍严格递增无混用。
        """
        fresh = Snapshot(self.tree_json)  # 失败抛 ViewerError，旧快照不受影响
        with self._lock:
            self._snapshot = fresh
            self._generation += 1
            return fresh, self._generation


class ViewerHandler(BaseHTTPRequestHandler):
    server: ViewerServer
    server_version = "file-tree-viewer/1.0"

    # ------------------------------------------------------------------
    # 路由：/api/* 快照查询（响应统一带 generation）；POST /api/refresh
    # 为唯一例外放行的 POST（不触任何数据写入口，仅重读）；其余写方法 405
    # ------------------------------------------------------------------

    def do_GET(self):
        if self._reject_bad_host():
            return
        parsed = urlsplit(self.path)
        try:
            if parsed.path == "/api/root":
                snap, gen = self.server.current()
                self._send_json(200, {"generation": gen, **snap.root_info()})
            elif parsed.path == "/api/children":
                self._api_children(parse_qs(parsed.query))
            elif parsed.path == "/api/detail":
                self._api_detail(parse_qs(parsed.query))
            elif parsed.path == "/api/search":
                self._api_search(parse_qs(parsed.query))
            elif parsed.path.startswith("/api/"):
                self._send_json(404, {"error": f"未知接口: {parsed.path}"})
            else:
                self._serve_static(parsed.path)
        except ViewerError as exc:
            self._send_json(exc.status, {"error": str(exc)})

    def do_POST(self):
        if self._reject_bad_host():
            return
        parsed = urlsplit(self.path)
        if parsed.path == "/api/refresh":
            self._api_refresh()
            return
        self._reject_write()

    def do_PUT(self):
        if self._reject_bad_host():
            return
        self._reject_write()

    def do_DELETE(self):
        if self._reject_bad_host():
            return
        self._reject_write()

    def do_PATCH(self):
        if self._reject_bad_host():
            return
        self._reject_write()

    def _reject_bad_host(self) -> bool:
        """默认绑定下校验 Host 头（防 DNS rebinding）；已拒绝时返回 True。

        只接受回环地址形态（127.0.0.1 / localhost / [::1]，可带端口），
        其余（含缺失）一律 403 并说明放行范围；自定义 --host 绑定跳过。
        """
        if not self.server.host_check:
            return False
        raw = (self.headers.get("Host") or "").strip()
        if raw.startswith("["):  # IPv6 字面量 [::1]:port
            hostname = raw.split("]", 1)[0] + "]"
        else:
            host_part, _, port_part = raw.rpartition(":")
            hostname = host_part if (host_part and port_part.isdigit()) else raw
        if hostname.lower() in ("127.0.0.1", "localhost", "[::1]"):
            return False
        self._send_json(
            403,
            {
                "error": (
                    "Host 头不在允许范围：默认绑定只接受 127.0.0.1 / localhost / [::1]"
                    "（防 DNS rebinding）。远端访问请用 SSH 隧道，或以 --host 显式绑定"
                )
            },
        )
        return True

    def _reject_write(self):
        self._send_json(405, {"error": "只读查看器：不支持写请求"})

    def _api_refresh(self):
        """手动刷新（G12）：重读同一路径；成功原子替换并递增世代号。

        失败（非法 JSON / 结构非法 / 不可读）明确报错且响应带旧世代号与
        refreshed=false——旧快照保持可用，前端据此标明"未刷新"。
        这里不向 do_POST 外抛 ViewerError：错误体需要附带旧世代号。
        """
        try:
            snap, gen = self.server.refresh_snapshot()
        except ViewerError as exc:
            _, old_gen = self.server.current()
            self._send_json(
                exc.status,
                {"error": str(exc), "generation": old_gen, "refreshed": False},
            )
            return
        self._send_json(200, {"generation": gen, "refreshed": True, **snap.root_info()})

    def _api_children(self, query: dict):
        snap, gen = self.server.current()
        path = (query.get("path") or [""])[0]
        children = snap.children(path)
        self._send_json(200, {"generation": gen, "path": path, "children": children})

    def _api_detail(self, query: dict):
        snap, gen = self.server.current()
        path = (query.get("path") or [""])[0]
        if not path:
            raise ViewerError("缺少 path 参数", 400)
        self._send_json(200, {"generation": gen, **snap.detail(path)})

    def _api_search(self, query: dict):
        """组合搜索（G08）：kw / tag / under / depth / page / page_size 全部可选。

        数值参数非法（非整数、越界）报 400 可读错误；kw/tag/under 空串视为未提供。
        """
        snap, gen = self.server.current()

        def first(name: str) -> str | None:
            value = (query.get(name) or [""])[0].strip()
            return value or None

        def positive_int(name: str, default: int | None = None) -> int | None:
            raw = first(name)
            if raw is None:
                return default
            try:
                return int(raw)
            except ValueError:
                raise ViewerError(f"{name} 必须是整数: {raw!r}", 400) from None

        kw = first("kw")
        tag = first("tag")
        under = first("under")
        depth = positive_int("depth")
        page = positive_int("page", default=1)
        page_size = positive_int("page_size", default=DEFAULT_PAGE_SIZE)
        self._send_json(
            200,
            {
                "generation": gen,
                **snap.search(
                    kw=kw, tag=tag, under=under, depth=depth, page=page, page_size=page_size
                ),
            },
        )

    # ------------------------------------------------------------------
    # 静态资源：仅托管查看器前端构建目录（G20），拒绝目录穿越
    # ------------------------------------------------------------------

    def _serve_static(self, url_path: str):
        static_dir = self.server.static_dir
        if not (static_dir / "index.html").is_file():
            self._send_html(
                503,
                "<!doctype html><html lang=\"zh\"><meta charset=\"utf-8\">"
                "<h3>前端资源缺失</h3>"
                "<p>查看器缺少发行页面资源（未找到 index.html）。本目录应为技能自带的"
                " viewer/ 静态资源；若确为源码形态，请在技能目录执行：</p>"
                + build_guide_pre_html()
                + "<p>构建并组装到 viewer/ 后重新启动查看器即可浏览；快照查询 API（/api/root 等）当前仍可用。</p>",
            )
            return
        rel = unquote(url_path).lstrip("/")
        if not rel:
            rel = "index.html"
        parts = [p for p in rel.split("/") if p not in ("", ".")]
        if not parts or any(p == ".." for p in parts) or "\\" in rel:
            self._send_json(404, {"error": "资源不存在"})
            return
        target = static_dir.joinpath(*parts)
        try:
            target.resolve().relative_to(static_dir.resolve())
        except ValueError:
            self._send_json(404, {"error": "资源不存在"})
            return
        if not target.is_file():
            self._send_json(404, {"error": "资源不存在"})
            return
        mime = (
            MIME_TYPES.get(target.suffix.lower())
            or mimetypes.guess_type(target.name)[0]
            or "application/octet-stream"
        )
        data = target.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    # ------------------------------------------------------------------
    # 输出
    # ------------------------------------------------------------------

    def _send_json(self, status: int, payload: dict):
        self._send_bytes(status, json.dumps(payload, ensure_ascii=False).encode("utf-8"),
                         "application/json; charset=utf-8")

    def _send_html(self, status: int, html: str):
        self._send_bytes(status, html.encode("utf-8"), "text/html; charset=utf-8")

    def _send_bytes(self, status: int, data: bytes, content_type: str):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, format, *args):  # noqa: A002 - 基类签名
        if self.server.quiet:
            return
        super().log_message(format, *args)


def python_version_error(version_info=None) -> str | None:
    """检测解释器是否达到启动门槛（G21/G22）；合格返回 None，否则返回用户可读错误。

    用 sys.version_info 而非语法推断：门槛是启动的必要条件，具体支持
    范围以实测矩阵为准（G23），不从"语法能解析"推导兼容承诺。
    """
    info = sys.version_info if version_info is None else version_info
    if tuple(info[:3]) < MIN_PYTHON:
        current = ".".join(str(part) for part in info[:3])
        required = ".".join(str(part) for part in MIN_PYTHON)
        return (
            f"Python 版本过低：当前 {current}，需要 {required}+。"
            "请安装满足要求的 Python 后重试（查看器不会自动升级运行时）"
        )
    return None


def create_server(
    tree_json: Path | str,
    host: str = DEFAULT_HOST,
    port: int = 0,
    static_dir: Path | str | None = None,
    quiet: bool = False,
) -> ViewerServer:
    """构造并绑定服务（不启动事件循环）；快照非法抛 ViewerError。

    port=0 时由系统分配空闲端口，实际地址见 server.server_address。
    """
    snapshot_path = Path(tree_json)
    snapshot = Snapshot(snapshot_path)  # 加载失败（ViewerError）直接上抛，不启动服务
    static = Path(static_dir if static_dir is not None else DEFAULT_STATIC_DIR)
    return ViewerServer(
        (host, port),
        ViewerHandler,
        snapshot_path,
        snapshot,
        static,
        quiet,
        host_check=host == DEFAULT_HOST,
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="viewer.py",
        description="独立快照的只读文件树查看器（Python 标准库实现，默认绑定 127.0.0.1）",
    )
    parser.add_argument("tree_json", help="tree.json 快照路径（可位于仓库之外）")
    parser.add_argument("--host", default=DEFAULT_HOST, help=f"监听地址（默认 {DEFAULT_HOST}）")
    parser.add_argument(
        "--port", type=int, default=DEFAULT_PORT, help=f"监听端口（默认 {DEFAULT_PORT}，0 表示随机）"
    )

    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8")  # 管道输出确定性编码（Windows 控制台默认 ANSI）
        except (AttributeError, ValueError, OSError):
            pass

    # 启动前置检查（G21/G22）：必要运行条件缺失时给可理解错误退出，
    # 不在后台尝试升级系统运行时；版本门槛先于参数解析——旧解释器上
    # 用户最先需要知道的是版本不满足，而不是用法错误
    version_error = python_version_error()
    if version_error is not None:
        print(f"无法启动查看器：{version_error}", file=sys.stderr)
        return 2

    args = parser.parse_args(argv)

    snapshot_path = Path(args.tree_json)
    if not snapshot_path.exists():
        print(f"无法启动查看器：快照文件不存在: {snapshot_path}", file=sys.stderr)
        return 2
    if not snapshot_path.is_file():
        print(f"无法启动查看器：快照路径不是文件: {snapshot_path}", file=sys.stderr)
        return 2
    try:
        server = create_server(snapshot_path, host=args.host, port=args.port)
    except ViewerError as exc:
        print(f"无法启动查看器：{exc}", file=sys.stderr)
        return 2

    host, port = server.server_address[:2]
    counts = server.snapshot.counts
    print("文件树只读查看器")
    print(f"快照: {snapshot_path}（{counts['total']} 条目：{counts['dirs']} 目录 / {counts['files']} 文件）")
    if not (server.static_dir / "index.html").is_file():
        print(f"提示: 发行页面资源缺失（缺 {server.static_dir / 'index.html'}），页面暂不可用，API 仍可访问")
        print(f"      组装方法（需现代构建机的 Node）: {BUILD_GUIDE}")
    print(f"访问地址: http://{host}:{port}/")
    print("按 Ctrl+C 停止")
    sys.stdout.flush()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n已停止")
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
