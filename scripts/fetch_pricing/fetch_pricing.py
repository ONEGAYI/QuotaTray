# -*- coding: utf-8 -*-
"""官网定价确定性抓取：主入口（provider 路由 + 一键全抓）。

用法
----
    python scripts/fetch_pricing/fetch_pricing.py              # 一键全抓
    python scripts/fetch_pricing/fetch_pricing.py deepseek     # 指定平台
    python scripts/fetch_pricing/fetch_pricing.py --format json

    契约测试（离线，不依赖网络）：
    python scripts/fetch_pricing/tests/fetch_pricing.tests.py

产出用途
--------
结构化输出官网现行价格矩阵，供人工核对后更新
``crates/quota-core/src/pricing.rs`` 的预置定价。本脚本不自动改源码。

已实现
------
- deepseek：中文定价页（CNY）+ 英文定价页（USD，2026-09-10 从断供
  恢复）。Docusaurus SSG 直出，内容关键词锚点解析，双币按序产出。
- zhipu：智谱国内站（CNY）。定价页为 Vue SPA，价格静态打包在主 bundle
  app.js 内——抓取链 = 壳 → app.<hash>.js → 对象字面量字段锚点解析。
- zai：Z.ai 国际站（USD）。SSG 直出，表头形态锚点（Model/Input/Cached
  Input），<del> 划线原价识别。
- 限时促销双价：现价进主档、原价三档存 promo_original、文案存 promo。

局限
----
- 币种通道：deepseek 双币（CNY 中文页 + USD 英文页）、zhipu 仅 CNY、
  zai 仅 USD；多币种平台由 SUPPORTED_CURRENCIES 声明（见 providers/
  __init__.py 协议）。
- 仅抓按量价格矩阵；峰谷时段窗口（DeepSeek）、订阅制积分倍率
  （GLM Coding Plan / Z.ai Coding Plan）不在范围，仍人工维护。
- 官网改版时脚本 ParseError 退出（fail loud），需对照 fixtures 快照
  人工适配锚点；fixtures/ 即锚点契约的固化基线。
"""

import argparse
import json
import os
import sys
from dataclasses import asdict

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from providers import deepseek, zai, zhipu  # noqa: E402

# 平台注册表：新增抓取组件时在此登记（模块需遵守 providers/__init__.py 协议）
PROVIDERS = {
    deepseek.PROVIDER_ID: deepseek,
    zhipu.PROVIDER_ID: zhipu,
    zai.PROVIDER_ID: zai,
}


def default_targets() -> list:
    """一键全抓的目标清单：当前为全部注册平台。"""
    return sorted(PROVIDERS.keys())


def provider_currencies(provider) -> tuple:
    """平台的币种展开清单：声明 SUPPORTED_CURRENCIES 者按声明顺序逐币种
    抓取；未声明者回退单默认币种（zhipu/zai 等单币平台无需声明）。"""
    return getattr(provider, "SUPPORTED_CURRENCIES", (provider.DEFAULT_CURRENCY,))


def parse_args(argv=None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="官网定价确定性抓取")
    parser.add_argument(
        "targets",
        nargs="*",
        choices=sorted(PROVIDERS.keys()),
        help="要抓取的平台（省略 = 全部注册平台）",
    )
    parser.add_argument(
        "--format", choices=("table", "json"), default="table", help="输出格式"
    )
    return parser.parse_args(argv)


def _display_width(text: str) -> int:
    """按 East Asian 宽度计显示列数（全角算 2），供中文对齐。"""
    import unicodedata

    return sum(
        2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1 for ch in text
    )


def _pad(text: str, width: int) -> str:
    return text + " " * max(width - _display_width(text), 0)


def _fmt_price(v: float) -> str:
    return f"{v:g}"


def _print_table(snapshot) -> None:
    print(f"{snapshot.provider}（{snapshot.currency}）  来源 {snapshot.source_url}")
    print(f"抓取于 {snapshot.fetched_at}")
    print(
        _pad("模型", 34)
        + _pad("档位", 6)
        + _pad("命中", 10)
        + _pad("未命中", 10)
        + _pad("输出", 10)
        + _pad("促销(原价)", 26)
    )
    for m in snapshot.models:
        promo = ""
        if m.promo_original is not None:
            p = m.promo_original
            promo = f"{m.promo} 原价({_fmt_price(p.hit)}/{_fmt_price(p.miss)}/{_fmt_price(p.out)})"[
                :24
            ]
        for label, tier in (("空闲", m.off_peak), ("高峰", m.peak)):
            cells = [
                m.model_id if label == "空闲" else "",
                label,
                _fmt_price(tier.hit),
                _fmt_price(tier.miss),
                _fmt_price(tier.out),
                promo if label == "空闲" else "",
            ]
            widths = (34, 6, 10, 10, 10, 26)
            print("".join(_pad(c, w) for c, w in zip(cells, widths)))
    print()


def _print_json(snapshot) -> None:
    print(json.dumps(asdict(snapshot), ensure_ascii=False, indent=2))


def main(argv=None) -> int:
    args = parse_args(argv)
    targets = args.targets or default_targets()

    failures = []
    for target in targets:
        provider = PROVIDERS[target]
        print(f"== {provider.DISPLAY_NAME}（{target}）==", file=sys.stderr)
        print(f"来源说明：{provider.SOURCE_NOTE}", file=sys.stderr)
        currencies = provider_currencies(provider)
        for currency in currencies:
            # 多币种平台以「平台/币种」标记成功与失败，单币种保持平台名
            label = f"{target}/{currency}" if len(currencies) > 1 else target
            try:
                snapshot = provider.fetch(currency)
            except Exception as exc:  # 单平台失败不阻断其余，最终汇总退出
                failures.append((label, exc))
                print(f"[失败] {label}: {exc}", file=sys.stderr)
                continue
            if args.format == "json":
                _print_json(snapshot)
            else:
                _print_table(snapshot)

    if failures:
        print(
            f"共 {len(failures)} 个平台失败：{', '.join(t for t, _ in failures)}",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
