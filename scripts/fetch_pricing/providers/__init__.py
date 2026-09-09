# -*- coding: utf-8 -*-
"""平台定价抓取组件目录。

每个平台一个模块，遵守统一协议（供上层 fetch_pricing.py 主入口路由）：

- ``PROVIDER_ID``     平台标识（CLI 路由名，小写）
- ``DISPLAY_NAME``    展示名
- ``SOURCE_NOTE``     来源与局限说明（URL、币种、已知断供/漂移）
- ``fetch(currency)`` 抓取入口，返回通用 ``PricingSnapshot``；
                      结构漂移抛 ``ParseError``，来源断供抛 ``SourceUnavailable``，
                      网络故障抛 ``FetchError``

通用数据结构与异常定义在 ``model.py``；平台注册表在上层主入口维护。
已实现：deepseek。
"""

from .model import (  # noqa: F401
    FetchError,
    ModelPricing,
    ParseError,
    PricingSnapshot,
    SourceUnavailable,
    Tier,
)

__all__ = [
    "FetchError",
    "ModelPricing",
    "ParseError",
    "PricingSnapshot",
    "SourceUnavailable",
    "Tier",
]
