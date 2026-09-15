"""查看器性能基准：对指定快照实测加载/查询/刷新的 CPU 时间与墙钟（G14 证据）。

用法:
    python bench_viewer.py <tree.json> [--repeat 5]

口径（与工单 #20"证据口径"一致，不冒充 GUI 性能）：
- 本脚本只测**服务端**（Python 进程）指标：
  * 加载 = Snapshot 构造（读盘 + 解析 + 建索引）
  * 查询 = children/detail/search 在内存模型上的耗时
  * 刷新 = refresh_snapshot 重读同一路径并原子替换
  * HTTP = 真实起服务（127.0.0.1 随机端口）后 http.client 往返墙钟
- CPU 时间用 time.process_time()（进程级，含服务线程），墙钟用
  time.perf_counter()；两者分开报告，不互相冒充。
- 滚动流畅度与浏览器 DOM 挂载行数属 GUI 指标：jsdom 断言见
  frontend 测试（VirtualTree.test.tsx），真浏览器验证由集成票补做。
- 每项操作重复 --repeat 次，报告 min / median / max；首次运行包含
  冷缓存效应，正是真实加载场景，不做预热剔除。
"""

from __future__ import annotations

import argparse
import http.client
import json
import statistics
import sys
import threading
import time
from pathlib import Path
from urllib.parse import quote

sys.path.insert(0, str(Path(__file__).parent))

import viewer  # noqa: E402
from viewer_core import Snapshot  # noqa: E402


def measure(fn, repeat: int) -> dict:
    """重复执行并分别记录 CPU/墙钟（秒）。"""
    cpu_samples: list[float] = []
    wall_samples: list[float] = []
    for _ in range(repeat):
        cpu0, wall0 = time.process_time(), time.perf_counter()
        fn()
        cpu_samples.append(time.process_time() - cpu0)
        wall_samples.append(time.perf_counter() - wall0)
    return {
        "cpu_ms": _fmt(cpu_samples),
        "wall_ms": _fmt(wall_samples),
    }


def _fmt(seconds: list[float]) -> str:
    ms = sorted(s * 1000 for s in seconds)
    return f"min {ms[0]:.1f} / 中位 {statistics.median(ms):.1f} / max {ms[-1]:.1f}"


def http_measure(server, path: str, repeat: int, method: str = "GET") -> dict:
    """HTTP 往返墙钟（客户端视角，含网络栈与 JSON 编解码）。"""
    host, port = server.server_address[:2]
    latencies: list[float] = []

    def once() -> bytes:
        conn = http.client.HTTPConnection(host, port, timeout=30)
        try:
            conn.request(method, path)
            resp = conn.getresponse()
            return resp.read()
        finally:
            conn.close()

    for _ in range(repeat):
        t0 = time.perf_counter()
        body = once()
        latencies.append(time.perf_counter() - t0)
    ms = sorted(s * 1000 for s in latencies)
    return {
        "wall_ms": f"min {ms[0]:.1f} / 中位 {statistics.median(ms):.1f} / max {ms[-1]:.1f}",
        "resp_bytes": len(body),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="bench_viewer.py", description="查看器服务端性能基准")
    parser.add_argument("tree_json", type=Path, help="快照路径（建议用 gen_viewer_sample.py 合成）")
    parser.add_argument("--repeat", type=int, default=5, help="每项重复次数（默认 5）")
    args = parser.parse_args(argv)

    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8")
        except (AttributeError, ValueError, OSError):
            pass

    repeat = max(1, args.repeat)
    snapshot_path = args.tree_json
    rows: list[tuple[str, str]] = []

    # 1) 加载（Snapshot 构造 = 服务启动时的快照解析 + 建索引）
    snap = Snapshot(snapshot_path)  # 先构造一次拿合法探测路径
    rows.append(("加载：Snapshot 构造（解析+三索引）", json.dumps(measure(lambda: Snapshot(snapshot_path), repeat), ensure_ascii=False)))

    # 探测路径：根级第一项（公共 API，不触私有结构）
    deep_path = snap.children("")[0]["path"] if snap.children("") else ""
    q = lambda s: quote(s, safe="")  # noqa: E731

    # 2) 内存查询（不经 HTTP）：children / detail / search
    rows.append(("内存查询：children(根)", json.dumps(measure(lambda: snap.children(""), repeat), ensure_ascii=False)))
    rows.append((f"内存查询：detail({deep_path[:28]}…)", json.dumps(measure(lambda: snap.detail(deep_path), repeat), ensure_ascii=False)))
    rows.append(("内存查询：search kw=序号（编号子串）", json.dumps(measure(lambda: snap.search(kw="序号 0421"), repeat), ensure_ascii=False)))
    rows.append(("内存查询：search kw=性能样本（中文）", json.dumps(measure(lambda: snap.search(kw="性能样本"), repeat), ensure_ascii=False)))
    rows.append(("内存查询：search tag=test", json.dumps(measure(lambda: snap.search(tag="test"), repeat), ensure_ascii=False)))

    # 3) 真实 HTTP 服务（线程内 serve_forever）+ 客户端往返
    import tempfile

    with tempfile.TemporaryDirectory() as static_dir:
        server = viewer.create_server(
            snapshot_path, port=0, static_dir=Path(static_dir), quiet=True
        )
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            rows.append(("HTTP：GET /api/root", json.dumps(http_measure(server, "/api/root", repeat=repeat), ensure_ascii=False)))
            rows.append(("HTTP：GET /api/children(根)", json.dumps(http_measure(server, "/api/children", repeat=repeat), ensure_ascii=False)))
            rows.append(("HTTP：GET /api/detail(深层)", json.dumps(http_measure(server, f"/api/detail?path={q(deep_path)}", repeat=repeat), ensure_ascii=False)))
            rows.append(("HTTP：GET /api/search kw=性能样本", json.dumps(http_measure(server, f"/api/search?kw={q('性能样本')}", repeat=repeat), ensure_ascii=False)))
            # 4) 刷新（服务端口径：重读+替换；经 HTTP 触发的墙钟另测）
            rows.append(("刷新：refresh_snapshot（进程内）", json.dumps(measure(server.refresh_snapshot, repeat), ensure_ascii=False)))
            rows.append(("HTTP：POST /api/refresh（含响应序列化）", json.dumps(http_measure(server, "/api/refresh", method="POST", repeat=repeat), ensure_ascii=False)))
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)

    # 输出
    print(f"快照: {snapshot_path}（{snap.counts['total']} 条目）")
    print(f"Python: {sys.version.split()[0]}  重复 {repeat} 次（含冷启动，不预热）")
    print()
    width = max(len(name) for name, _ in rows)
    for name, detail in rows:
        payload = json.loads(detail)
        cpu = payload.get("cpu_ms", "-")
        wall = payload["wall_ms"]
        extra = f"  响应 {payload['resp_bytes']/1000:.1f}KB" if "resp_bytes" in payload else ""
        print(f"{name.ljust(width)}  CPU(ms) {cpu}  墙钟(ms) {wall}{extra}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
