# -*- coding: utf-8 -*-
"""DeepSeek 官网定价抓取组件。

已实现
------
- 中文定价页（CNY）：https://api-docs.deepseek.com/zh-cn/quick_start/pricing
  Docusaurus SSG 静态渲染，HTTP 直出完整表格，无需浏览器。
  解析锚点为内容自描述关键词（「百万tokens输入（缓存命中）」「空闲/高峰时段」
  「X元」），不依赖 CSS class——官方新增模型列/行时无需改解析器。

局限
----
- 英文定价页（USD）2026-09-09 实测断供：sitemap 唯一路径
  /quick_start/pricing 服务端返回的是另一篇文档（Your First API Call），
  第三方渲染缓存停留在 V3.1 时代。USD 通道显式抛 SourceUnavailable，
  绝不按汇率折算假数据。
- 价格格式仅适配「X元」；出现 $ / 千分位等格式会 ParseError（fail loud）。
- 峰谷时段窗口（工作日 09:00-12:00 / 14:00-18:00 北京时间）不解析：
  它在表格脚注正文里、变动极少，预置窗口已在 pricing.rs 固化维护。
- 「高峰 = 空闲 × 2」按官网脚注规则做解析后自校验，规则变更时会拦截。
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

PROVIDER_ID = "deepseek"
DISPLAY_NAME = "DeepSeek"
DEFAULT_CURRENCY = "CNY"

ZH_URL = "https://api-docs.deepseek.com/zh-cn/quick_start/pricing"
EN_URL = "https://api-docs.deepseek.com/quick_start/pricing"

SOURCE_NOTE = (
    f"中文页 {ZH_URL}（SSG 直出，可确定性抓取）；"
    f"英文页 {EN_URL} 2026-09-09 实测路由故障，USD 档暂无来源"
)

_PEAK_DOUBLE_TOLERANCE = 1e-9

_PRICE_CELL_RE = re.compile(r"^(\d+(?:\.\d+)?)元$")


def _strip_tags(fragment: str) -> str:
    """去标签并压缩全部空白（中文文本无空格语义，压缩利于关键词匹配）。"""
    return re.sub(r"\s+", "", re.sub(r"<[^>]+>", "", fragment))


def _cells(row_html: str) -> list:
    return [_strip_tags(c) for c in re.findall(r"<td[^>]*>(.*?)</td>", row_html, re.S)]


def _billing_kind(text: str):
    """识别计费项格。「百万tokens」前缀用于排除「输出长度」等同字干绕行。"""
    if "百万tokens" not in text:
        return None
    if "缓存未命中" in text:
        return "miss"
    if "缓存命中" in text:
        return "hit"
    if "输出" in text:
        return "out"
    return None


def _tier_kind(text: str):
    if text == "空闲时段":
        return "off_peak"
    if text == "高峰时段":
        return "peak"
    return None


def parse_zh_html(html: str) -> PricingSnapshot:
    """解析中文定价页 HTML → PricingSnapshot。结构漂移抛 ParseError。"""
    models = _parse_models(_select_pricing_table(html))
    snapshot = PricingSnapshot(
        provider=PROVIDER_ID,
        currency="CNY",
        source_url=ZH_URL,
        fetched_at=datetime.now(timezone.utc).isoformat(timespec="seconds"),
        models=models,
    )
    assert_peak_double_off_peak(snapshot)
    return snapshot


def _select_pricing_table(html: str) -> str:
    for table in re.findall(r"<table[^>]*>(.*?)</table>", html, re.S):
        rows = re.findall(r"<tr[^>]*>(.*?)</tr>", table, re.S)
        if rows and _cells(rows[0]) and _cells(rows[0])[0] == "模型":
            return table
    raise ParseError("未找到定价表：页面缺少首格为「模型」的表格（官网可能已改版）")


def _parse_models(table_html: str) -> list:
    rows = [_cells(r) for r in re.findall(r"<tr[^>]*>(.*?)</tr>", table_html, re.S)]

    model_ids = None
    versions = {}
    prices = {}  # (model_idx, tier_kind, billing_kind) -> float
    last_billing = None  # rowspan 计费项格只出现在该计费项首行，延续行继承

    for cells in rows:
        if not cells:
            continue
        if cells[0] == "模型":
            model_ids = cells[1:]
            continue
        if cells[0] == "模型版本":
            for i, v in enumerate(cells[1:]):
                versions[i] = v
            continue

        billing = None
        tier = None
        tier_at = None
        for idx, cell in enumerate(cells):
            if billing is None:
                billing = _billing_kind(cell)
            if tier is None:
                tier = _tier_kind(cell)
                if tier is not None:
                    tier_at = idx

        if tier is None:
            continue
        if model_ids is None:
            raise ParseError("时段行出现在「模型」行之前，无法定位价格列")
        if billing is not None:
            last_billing = billing
        if last_billing is None:
            raise ParseError("时段行缺少计费项标签（缓存命中/未命中/输出）")

        value_cells = cells[tier_at + 1 :]
        if len(value_cells) != len(model_ids):
            raise ParseError(
                f"价格格数 {len(value_cells)} 与模型数 {len(model_ids)} 不符"
            )
        values = []
        for cell in value_cells:
            m = _PRICE_CELL_RE.match(cell)
            if not m:
                raise ParseError(
                    f"不支持的价格格 {cell!r}（当前仅适配「X元」格式，可能改版）"
                )
            values.append(float(m.group(1)))
        for i, v in enumerate(values):
            prices[(i, tier, last_billing)] = v

    if model_ids is None:
        raise ParseError("定价表中未找到「模型」行")

    result = []
    for i, model_id in enumerate(model_ids):
        kwargs = {}
        for tier_kind in ("off_peak", "peak"):
            triple = []
            for billing in ("hit", "miss", "out"):
                try:
                    triple.append(prices[(i, tier_kind, billing)])
                except KeyError:
                    raise ParseError(
                        f"模型 {model_id} 缺少 {tier_kind} 档 {billing} 价格"
                    ) from None
            kwargs[tier_kind] = Tier(*triple)
        result.append(
            ModelPricing(
                model_id=model_id, version=versions.get(i, ""), **kwargs
            )
        )
    return result


def assert_peak_double_off_peak(snapshot: PricingSnapshot) -> None:
    """官网脚注规则：高峰价 = 空闲价 × 2。违反即解析错位或规则变更。"""
    for m in snapshot.models:
        for attr in ("hit", "miss", "out"):
            peak = getattr(m.peak, attr)
            off = getattr(m.off_peak, attr)
            if abs(peak - 2 * off) > _PEAK_DOUBLE_TOLERANCE:
                raise ParseError(
                    f"{m.model_id} 的 {attr} 高峰价 {peak} ≠ 空闲价 {off} × 2"
                    "（解析错位，或官网峰谷规则已变更）"
                )


def fetch(currency: str = None) -> PricingSnapshot:
    """网络抓取入口。默认 CNY（中文页）；USD 因英文页断供显式失败。"""
    if currency not in (None, DEFAULT_CURRENCY):
        raise SourceUnavailable(
            f"DeepSeek {currency} 档无来源：英文定价页 {EN_URL} "
            "2026-09-09 实测路由故障（返回非定价页内容），修复前 USD 不可抓取"
        )
    return parse_zh_html(http_get(ZH_URL))
