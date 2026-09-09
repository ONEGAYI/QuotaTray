# -*- coding: utf-8 -*-
"""Z.ai 国际站（docs.z.ai）定价抓取组件。

已实现
------
- USD 按量价格矩阵：定价页为 SSG 静态直出（curl 无需浏览器），
  URL https://docs.z.ai/guides/overview/pricing 。
- 解析「表头为 Model / Input / Cached Input」的表格（Latest / Text /
  Vision 三张模型表）；Built-in Tools、图像/视频/音频、Agents 表按
  表头形态排除。
- 限时促销双价：单元格为 <del>$原价</del> $现价，取现价、原价三档存
  promo_original（页面无促销文案，promo 留空）。
- 三价不齐的行跳过收录（如 GLM-4-32B-0414-128K 无缓存计价，Cached
  Input 列为 "-"）：不猜测语义，宁缺毋错。
- Z.ai 按量无峰谷：peak 与 off_peak 同价输出。

局限
----
- 仅 USD；其他币种抛 SourceUnavailable。
- 2026-09-09 快照固化事实：国际站定价页无 GLM-5-Turbo（预置 zai_api
  中的 glm-5-turbo 与官网现状存在偏差，核对时人工裁决）。
- Coding Plan（订阅积分制）不在本组件范围，仍在 pricing.rs 人工维护。
"""

import re
from datetime import datetime, timezone

from ._http import http_get
from .model import (
    ModelPricing,
    ParseError,
    PricingSnapshot,
    SourceUnavailable,
    Tier,
)

PROVIDER_ID = "zai"
DISPLAY_NAME = "Z.ai（智谱国际站）"
DEFAULT_CURRENCY = "USD"

PRICING_URL = "https://docs.z.ai/guides/overview/pricing"

SOURCE_NOTE = f"{PRICING_URL} SSG 直出，表头形态锚点解析（Model/Input/Cached Input）"

_MODEL_TABLE_HEAD = ("Model", "Input", "Cached Input")
_PRICE_STRIKE_RE = re.compile(r"<del>\$([\d.]+)</del>\s*\$([\d.]+)")
_PRICE_PLAIN_RE = re.compile(r"^\$([\d.]+)$")


def _strip_tags(fragment: str) -> str:
    return re.sub(r"\s+", " ", re.sub(r"<[^>]+>", "", fragment)).strip()


def _parse_price_cell(cell_html: str) -> tuple:
    """价格单元格 → (现价, 原价或 None)。del 划线 = 原价，其余文本 = 现价。

    「Free」按 0.0 收录（与智谱「免费」策略一致）。
    """
    m = _PRICE_STRIKE_RE.search(cell_html)
    if m:
        return float(m.group(2)), float(m.group(1))
    text = _strip_tags(cell_html)
    if text == "Free":
        return 0.0, None
    m = _PRICE_PLAIN_RE.match(text)
    if m:
        return float(m.group(1)), None
    raise ParseError(f"不支持的价格格 {text!r}（可能改版为非 $ 数字格式）")


def parse_en_html(html: str) -> PricingSnapshot:
    """解析英文定价页 HTML → PricingSnapshot。结构漂移抛 ParseError。"""
    models = []
    seen = set()
    for table in re.findall(r"<table[^>]*>(.*?)</table>", html, re.S):
        rows = re.findall(r"<tr[^>]*>(.*?)</tr>", table, re.S)
        if not rows:
            continue
        head = [_strip_tags(c) for c in re.findall(r"<t[dh][^>]*>(.*?)</t[dh]>", rows[0], re.S)]
        if tuple(head[:3]) != _MODEL_TABLE_HEAD:
            continue  # 工具/图像/视频/Agent 等非按量模型表

        for row in rows[1:]:
            cells = re.findall(r"<td[^>]*>(.*?)</td>", row, re.S)
            if len(cells) < 5:
                raise ParseError(
                    f"模型行单元格数 {len(cells)} < 5（表格结构可能已变更）"
                )
            model_id = _strip_tags(cells[0])
            if not model_id or model_id in seen:
                continue
            plain = [_strip_tags(c) for c in (cells[1], cells[2], cells[4])]
            # "-" 与 "\\" 表示该模型无此项计费（如无缓存计价），三价不齐不收录
            if any(c in ("-", "\\") for c in plain):
                continue
            in_cur, in_orig = _parse_price_cell(cells[1])
            hit_cur, hit_orig = _parse_price_cell(cells[2])
            out_cur, out_orig = _parse_price_cell(cells[4])
            promo_original = None
            if any(o is not None for o in (in_orig, hit_orig, out_orig)):
                promo_original = Tier(hit_orig, in_orig, out_orig)
            tier = Tier(hit_cur, in_cur, out_cur)
            seen.add(model_id)
            models.append(
                ModelPricing(
                    model_id=model_id,
                    promo_original=promo_original,
                    off_peak=tier,
                    peak=tier,  # 无峰谷
                )
            )

    if not models:
        raise ParseError("未找到模型定价表（页面可能已改版）")
    return PricingSnapshot(
        provider=PROVIDER_ID,
        currency=DEFAULT_CURRENCY,
        source_url=PRICING_URL,
        fetched_at=datetime.now(timezone.utc).isoformat(timespec="seconds"),
        models=models,
    )


def fetch(currency: str = None) -> PricingSnapshot:
    """网络抓取入口。仅支持 USD。"""
    if currency not in (None, DEFAULT_CURRENCY):
        raise SourceUnavailable(f"Z.ai 定价页仅 USD 计价，不支持 {currency}")
    return parse_en_html(http_get(PRICING_URL))
