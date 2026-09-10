# -*- coding: utf-8 -*-
"""DeepSeek 官网定价抓取组件。

已实现
------
- 中文定价页（CNY）：https://api-docs.deepseek.com/zh-cn/quick_start/pricing
- 英文定价页（USD）：https://api-docs.deepseek.com/quick_start/pricing
  英文页曾于 2026-09-09 路由故障（服务端返回另一篇文档），
  2026-09-10 复测已恢复，与中文页同构（新版两列表）。
  两页均为 Docusaurus SSG 静态渲染，HTTP 直出完整表格，无需浏览器。
  解析锚点为内容自描述关键词（表头首格「模型」/ MODEL、计费项、
  时段、价格格式），不依赖 CSS class——官方新增模型列/行时无需改
  解析器；列数变化由「价格格数 = 模型数」校验自适应。
- 表头模型 ID 的脚注引用上标（如 deepseek-flash(1)）剥离后使用，
  防止脏 ID 进候选数据。

局限
----
- 价格格式按语言通道各自适配：中文页仅认「X元」、英文页仅认 $X；
  交叉格式（如中文页出现 $）按改版信号 ParseError（fail loud）。
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
    f"中文页 {ZH_URL}（CNY）+ 英文页 {EN_URL}（USD），"
    "两页 SSG 直出同构表格，可确定性抓取"
)

#: 多币种声明：主入口按序逐币种抓取（协议见 providers/__init__.py）
SUPPORTED_CURRENCIES = ("CNY", "USD")

_PEAK_DOUBLE_TOLERANCE = 1e-9

_PRICE_CNY_RE = re.compile(r"^(\d+(?:\.\d+)?)元$")
_PRICE_USD_RE = re.compile(r"^\$([\d.]+)$")

#: 表头/版本行尾部的脚注引用上标（形如 (1)、(2)），提取模型 ID 后剥离
_FOOTNOTE_REF_RE = re.compile(r"\(\d+\)$")

#: 每语言通道的解析锚点。关键词匹配一律在压缩空白 + casefold 后的
#: 单元格文本上进行（中文不受影响，英文大小写风格变化不误报）。
_ZH_ANCHORS = {
    "currency": "CNY",
    "source_url": ZH_URL,
    "model_head": "模型",
    "version_head": "模型版本",
    "input_kw": "百万tokens输入",
    "output_kw": "百万tokens输出",
    "cache_hit_kw": "缓存命中",
    "cache_miss_kw": "缓存未命中",
    "off_peak_kw": "空闲时段",
    "peak_kw": "高峰时段",
    "price_re": _PRICE_CNY_RE,
    "price_hint": "当前仅适配「X元」格式",
}

_EN_ANCHORS = {
    "currency": "USD",
    "source_url": EN_URL,
    "model_head": "model",
    "version_head": "modelversion",
    "input_kw": "inputtokens",
    "output_kw": "outputtokens",
    "cache_hit_kw": "cachehit",
    "cache_miss_kw": "cachemiss",
    "off_peak_kw": "off-peak",
    "peak_kw": "peak",
    "price_re": _PRICE_USD_RE,
    "price_hint": "当前仅适配 $X 美元格式",
}


def _strip_tags(fragment: str) -> str:
    """去标签并压缩全部空白（中文文本无空格语义，压缩利于关键词匹配）。"""
    return re.sub(r"\s+", "", re.sub(r"<[^>]+>", "", fragment))


def _cells(row_html: str) -> list:
    return [_strip_tags(c) for c in re.findall(r"<td[^>]*>(.*?)</td>", row_html, re.S)]


def _billing_kind(text: str, anchors: dict):
    """识别计费项格。输入/输出关键词前缀用于排除「输出长度」等同字干绕行。"""
    folded = text.lower()
    has_input = anchors["input_kw"] in folded
    has_output = anchors["output_kw"] in folded
    if not (has_input or has_output):
        return None
    if anchors["cache_miss_kw"] in folded:
        return "miss"
    if anchors["cache_hit_kw"] in folded:
        return "hit"
    if has_output:
        return "out"
    return None


def _tier_kind(text: str, anchors: dict):
    folded = text.lower()
    if folded == anchors["off_peak_kw"]:
        return "off_peak"
    if folded == anchors["peak_kw"]:
        return "peak"
    return None


def parse_zh_html(html: str) -> PricingSnapshot:
    """解析中文定价页 HTML → PricingSnapshot（CNY）。结构漂移抛 ParseError。"""
    return _parse_html(html, _ZH_ANCHORS)


def parse_en_html(html: str) -> PricingSnapshot:
    """解析英文定价页 HTML → PricingSnapshot（USD）。结构漂移抛 ParseError。"""
    return _parse_html(html, _EN_ANCHORS)


def _parse_html(html: str, anchors: dict) -> PricingSnapshot:
    models = _parse_models(_select_pricing_table(html, anchors), anchors)
    snapshot = PricingSnapshot(
        provider=PROVIDER_ID,
        currency=anchors["currency"],
        source_url=anchors["source_url"],
        fetched_at=datetime.now(timezone.utc).isoformat(timespec="seconds"),
        models=models,
    )
    assert_peak_double_off_peak(snapshot)
    return snapshot


def _select_pricing_table(html: str, anchors: dict) -> str:
    for table in re.findall(r"<table[^>]*>(.*?)</table>", html, re.S):
        rows = re.findall(r"<tr[^>]*>(.*?)</tr>", table, re.S)
        if not rows or not _cells(rows[0]):
            continue
        if _cells(rows[0])[0].lower() == anchors["model_head"]:
            return table
    raise ParseError("未找到定价表：页面缺少表头为模型列的表格（官网可能已改版）")


def _parse_models(table_html: str, anchors: dict) -> list:
    rows = [_cells(r) for r in re.findall(r"<tr[^>]*>(.*?)</tr>", table_html, re.S)]

    model_ids = None
    versions = {}
    prices = {}  # (model_idx, tier_kind, billing_kind) -> float
    last_billing = None  # rowspan 计费项格只出现在该计费项首行，延续行继承

    for cells in rows:
        if not cells:
            continue
        head = cells[0].lower()
        if head == anchors["model_head"]:
            model_ids = [_FOOTNOTE_REF_RE.sub("", mid) for mid in cells[1:]]
            continue
        if head == anchors["version_head"]:
            for i, v in enumerate(cells[1:]):
                versions[i] = v
            continue

        billing = None
        tier = None
        tier_at = None
        for idx, cell in enumerate(cells):
            if billing is None:
                billing = _billing_kind(cell, anchors)
            if tier is None:
                tier = _tier_kind(cell, anchors)
                if tier is not None:
                    tier_at = idx

        if tier is None:
            continue
        if model_ids is None:
            raise ParseError("时段行出现在模型表头之前，无法定位价格列")
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
            m = anchors["price_re"].match(cell)
            if not m:
                raise ParseError(
                    f"不支持的价格格 {cell!r}（{anchors['price_hint']}，可能改版）"
                )
            values.append(float(m.group(1)))
        for i, v in enumerate(values):
            prices[(i, tier, last_billing)] = v

    if model_ids is None:
        raise ParseError("定价表中未找到模型表头行")

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
    """网络抓取入口。默认 CNY（中文页）；USD 走英文页；未知币种显式失败。"""
    if currency not in (None, *SUPPORTED_CURRENCIES):
        raise SourceUnavailable(
            f"DeepSeek 定价页仅 {'/'.join(SUPPORTED_CURRENCIES)} 计价，"
            f"不支持 {currency}"
        )
    if currency == "USD":
        return parse_en_html(http_get(EN_URL))
    return parse_zh_html(http_get(ZH_URL))
