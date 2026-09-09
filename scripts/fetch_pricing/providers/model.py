# -*- coding: utf-8 -*-
"""通用数据结构与异常：跨平台共享，不含任何平台特定逻辑。

单元为「百万 tokens 价格」。峰谷两档（peak / off_peak）每档三价：
缓存命中输入（hit）、缓存未命中输入（miss）、输出（out）。
"""

from dataclasses import dataclass, field
from typing import List, Optional


class ParseError(Exception):
    """页面结构与预期不符（官网改版/锚点漂移）。

    契约：结构漂移必须抛此错而非输出半截数据——宁可失败，不可错数据。
    """


class SourceUnavailable(Exception):
    """定价来源当前不可用（页面下线/路由故障），且无可靠替代来源。"""


class FetchError(Exception):
    """网络层故障（超时、非 2xx 等）。"""


@dataclass(frozen=True)
class Tier:
    """单档三价：hit=缓存命中输入，miss=缓存未命中输入，out=输出。"""

    hit: float
    miss: float
    out: float


@dataclass
class ModelPricing:
    """单模型峰谷价格。model_id 保留官网原始标识（如 deepseek-v4-flash），
    与 core 预置的短 id（flash）映射关系在 pricing.rs 侧人工维护。

    promo / promo_original 表达限时促销：promo 为官网促销文案（可空），
    promo_original 为促销前三档原价（无促销或未标注划线原价时为 None）。
    """

    model_id: str
    version: str = ""
    promo: str = ""
    promo_original: Optional[Tier] = None
    off_peak: Tier = field(default_factory=lambda: Tier(0.0, 0.0, 0.0))
    peak: Tier = field(default_factory=lambda: Tier(0.0, 0.0, 0.0))


@dataclass
class PricingSnapshot:
    """一次抓取的结构化结果。峰谷时段窗口不在抓取范围（见主入口局限说明）。"""

    provider: str
    currency: str
    source_url: str
    fetched_at: str
    models: List[ModelPricing]
