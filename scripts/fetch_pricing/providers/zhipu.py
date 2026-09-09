# -*- coding: utf-8 -*-
"""智谱国内站（open.bigmodel.cn）定价抓取组件。

已实现
------
- CNY 按量价格矩阵：定价页是 Vue SPA（curl 只有壳），但全部模型价格
  **静态打包在主 bundle app.js** 里，锚点为对象字面量字段
  （name / inPrice / outPrice / hit / upDownText / intro），与 CSS 无关。
- 抓取链：GET /pricing 壳 → 提取 app.<hash>.js 完整 URL → GET bundle →
  正则提取模型块。hash 随部署变化，每次从壳动态解析，不硬编码。
- 限时促销双价（如 GLM-5.3-Flash）：数组 [现价, 原价]，取现价、
  原价三档存 promo_original，促销文案（intro）存 promo。
- 「输入长度 [x, y)」分档计价的 rowspan 续行（name 为空）：归属前一
  具名模型，档位文本并入 model_id（如 "GLM-5.1（输入长度 [32+)）"）。
- 智谱按量无峰谷：peak 与 off_peak 同价输出。

局限
----
- 仅 CNY；其他币种抛 SourceUnavailable（智谱开放平台按量计价币种单一）。
- app.js 数据块字段顺序若重构（字段改名/结构化重排）会 ParseError，
  需对照 fixtures 快照人工适配。
- GLM Coding Plan（订阅积分制）不在本组件范围：其倍率权益说明不是
  按量价格矩阵，仍在 pricing.rs 人工维护。
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

PROVIDER_ID = "zhipu"
DISPLAY_NAME = "智谱 BigModel（国内站）"
DEFAULT_CURRENCY = "CNY"

SHELL_URL = "https://open.bigmodel.cn/pricing"
_BUNDLE_BASE = "https://static.bigmodel.cn/wd-paas-front/"

SOURCE_NOTE = (
    f"{SHELL_URL} 为 Vue SPA，价格静态打包在主 bundle app.js 内；"
    "抓取链 = 壳 → app.<hash>.js → 对象字面量字段锚点解析"
)

# 对象块锚：{name:"..."} 起，到下一个 {name:" 或串尾
_BLOCK_START_RE = re.compile(r'\{name:"')
_FIELD_RES = {
    "name": re.compile(r'name:"([^"]*)"'),
    "intro": re.compile(r'intro:"([^"]*)"'),
    "inPrice": re.compile(r"inPrice:\[([^\]]*)\]"),
    "outPrice": re.compile(r"outPrice:\[([^\]]*)\]"),
    "hit": re.compile(r"hit:\[([^\]]*)\]"),
    # upDownText 数组有单元素（"1M"/"200K"/"/"）与多元素分档
    # （"输入长度 [0, 32)","输出长度 [0.2+)"）两种形态，整体捕获后切分
    "upDown": re.compile(r"upDownText:\[([^\]]*)\]"),
}
_PRICE_RE = re.compile(r"^(\d+(?:\.\d+)?)元$")


def _parse_price_array(raw_array: str, field: str) -> list:
    """["0.4元","0.8元"] → [0.4, 0.8]；[现价, 原价]（双元素时）。

    「免费」槽位按 0.0 收录（2026-09-09 快照：GLM-4.7-Flash、
    GLM-4.6V-Flash 三价全免）；「免费」与数字混排视为结构歧义，拒绝。
    """
    values = []
    for item in raw_array.split(","):
        item = item.strip().strip('"')
        if item == "免费":
            values.append(0.0)
            continue
        m = _PRICE_RE.match(item)
        if not m:
            raise ParseError(
                f"不支持的价格值 {item!r}（字段 {field}，可能数据结构变更）"
            )
        values.append(float(m.group(1)))
    if not values or len(values) > 2:
        raise ParseError(f"字段 {field} 价格数组长度异常：{raw_array!r}")
    if 0.0 in values and len(values) == 2:
        raise ParseError(
            f"字段 {field} 出现免费与数字混排：{raw_array!r}（结构歧义，需人工确认）"
        )
    return values


def parse_app_js(js_text: str) -> PricingSnapshot:
    """解析 app.js 主 bundle → PricingSnapshot。结构漂移抛 ParseError。"""
    starts = [m.start() for m in _BLOCK_START_RE.finditer(js_text)]
    models = []
    last_named = None  # rowspan 续行的归属模型
    for i, start in enumerate(starts):
        end = starts[i + 1] if i + 1 < len(starts) else len(js_text)
        block = js_text[start:end]

        m_in = _FIELD_RES["inPrice"].search(block)
        if m_in is None:
            continue  # 非价格块（导航/场景配置等）

        name = _FIELD_RES["name"].search(block).group(1)
        fields = {}
        for key in ("outPrice", "hit", "upDown"):
            m = _FIELD_RES[key].search(block)
            if m is None:
                raise ParseError(
                    f"价格块 {name or '(续行)'} 缺少 {key} 字段（结构可能已变更）"
                )
            fields[key] = m.group(1)
        # intro 为可选字段（部分块缺失），缺省无促销文案
        m_intro = _FIELD_RES["intro"].search(block)
        intro = m_intro.group(1) if m_intro else ""

        prices = {
            "miss": _parse_price_array(m_in.group(1), "inPrice"),
            "out": _parse_price_array(fields["outPrice"], "outPrice"),
            "hit": _parse_price_array(fields["hit"], "hit"),
        }
        if len({len(v) for v in prices.values()}) != 1:
            raise ParseError(f"价格块 {name or '(续行)'} 三价数组长度不一致")

        tier = Tier(prices["hit"][0], prices["miss"][0], prices["out"][0])
        promo_original = None
        if len(prices["miss"]) == 2:  # 促销双价：[现价, 原价]
            promo_original = Tier(
                prices["hit"][1], prices["miss"][1], prices["out"][1]
            )

        # upDownText：单元素=上下文长度；含「输入长度/输出长度」=分档计价。
        # 元素切分必须引号感知——档位文本自身含逗号（如 "[0, 32)"）
        up_down_parts = re.findall(r'"([^"]*)"', fields["upDown"])
        up_down = "·".join(up_down_parts)
        is_tiered = any(
            p.startswith(("输入长度", "输出长度")) for p in up_down_parts
        )
        if name:
            model_id = f"{name}（{up_down}）" if is_tiered else name
            last_named = name
        else:  # rowspan 分档续行：归属前一具名模型
            if last_named is None:
                raise ParseError("分档续行出现在任何具名模型之前")
            model_id = f"{last_named}（{up_down}）"

        models.append(
            ModelPricing(
                model_id=model_id,
                version=up_down,
                # intro 是官网徽标（如"新品"），仅在有促销原价时才归为促销文案
                promo=intro if promo_original is not None else "",
                promo_original=promo_original,
                off_peak=tier,
                peak=tier,  # 无峰谷
            )
        )

    if not models:
        raise ParseError("bundle 中未找到任何价格块（结构可能已变更）")
    return PricingSnapshot(
        provider=PROVIDER_ID,
        currency=DEFAULT_CURRENCY,
        source_url=SHELL_URL,
        fetched_at=datetime.now(timezone.utc).isoformat(timespec="seconds"),
        models=models,
    )


def extract_app_js_url(shell_html: str) -> str:
    """从 /pricing 壳 HTML 提取主 bundle 完整 URL（hash 随部署变化）。"""
    m = re.search(r'(https://[^"\']+?/js/app\.[0-9a-f]+\.js)', shell_html)
    if m:
        return m.group(1)
    m = re.search(r'"(js/app\.[0-9a-f]+\.js)"', shell_html)
    if m:
        return _BUNDLE_BASE + m.group(1)
    raise ParseError("壳 HTML 中未找到 app bundle 引用（部署结构可能已变更）")


def fetch(currency: str = None) -> PricingSnapshot:
    """网络抓取入口：壳 → app.js → 解析。仅支持 CNY。"""
    if currency not in (None, DEFAULT_CURRENCY):
        raise SourceUnavailable(
            f"智谱国内站仅提供 CNY 计价，不支持 {currency}"
        )
    shell = http_get(SHELL_URL)
    bundle_url = extract_app_js_url(shell)
    return parse_app_js(http_get(bundle_url))
