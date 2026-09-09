# -*- coding: utf-8 -*-
"""共用 HTTP GET 助手：UA、超时、FetchError 语义统一。"""

import urllib.error
import urllib.request

from .model import FetchError

_USER_AGENT = "QuotaTray-pricing-fetch/1.0 (+https://github.com/ONEGAYI/QuotaTray)"
_TIMEOUT_SECONDS = 30


def http_get(url: str) -> str:
    request = urllib.request.Request(url, headers={"User-Agent": _USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=_TIMEOUT_SECONDS) as resp:
            return resp.read().decode("utf-8")
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise FetchError(f"抓取 {url} 失败：{exc}") from exc
