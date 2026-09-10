# -*- coding: utf-8 -*-
"""fetch_pricing 契约测试。

跑法：python scripts/fetch_pricing/tests/fetch_pricing.tests.py

契约范围：
- DeepSeek 中文定价页（CNY）与英文定价页（USD）解析
  （fixture 固化 2026-09-10 新版两列快照，离线可测）
- 结构漂移 fail loud（改版报错而非输出错数据）
- USD 通道经英文页快照产出候选；未知币种显式失败
- 主入口 provider 路由表与多币种展开

网络不在契约内：fetch 的 HTTP 路径由人工触发主入口验证。
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from providers import deepseek, zai, zhipu  # noqa: E402
import fetch_pricing as entry  # noqa: E402

FIXTURES = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")


def load_zh_fixture() -> str:
    with open(
        os.path.join(FIXTURES, "deepseek_zh_2026-09-10.html"), encoding="utf-8"
    ) as f:
        return f.read()


def load_en_fixture() -> str:
    with open(
        os.path.join(FIXTURES, "deepseek_en_2026-09-10.html"), encoding="utf-8"
    ) as f:
        return f.read()


def load_zhipu_fixture() -> str:
    with open(
        os.path.join(FIXTURES, "zhipu_app_2026-09-09.js"), encoding="utf-8"
    ) as f:
        return f.read()


def load_zai_fixture() -> str:
    with open(
        os.path.join(FIXTURES, "zai_en_2026-09-09.html"), encoding="utf-8"
    ) as f:
        return f.read()


class ParseDeepSeekZhTest(unittest.TestCase):
    """中文页解析契约：两列模型集合（无脚注尾巴）、三档新价、峰=谷×2。"""

    @classmethod
    def setUpClass(cls):
        cls.snapshot = deepseek.parse_zh_html(load_zh_fixture())

    def test_provider_metadata(self):
        self.assertEqual(self.snapshot.provider, "deepseek")
        self.assertEqual(self.snapshot.currency, "CNY")
        self.assertIn("api-docs.deepseek.com", self.snapshot.source_url)

    def test_model_ids_without_footnote_refs(self):
        """表头模型 ID 带脚注上标（deepseek-flash(1)），解析后必须剥离。"""
        self.assertEqual(
            [m.model_id for m in self.snapshot.models],
            ["deepseek-flash", "deepseek-v4-pro"],
        )

    def test_model_versions(self):
        """模型版本行应一并提取（版本号 bump 是模型升级信号）。"""
        by_id = {m.model_id: m.version for m in self.snapshot.models}
        self.assertEqual(by_id["deepseek-flash"], "DeepSeek-V4.1-Flash")
        self.assertEqual(by_id["deepseek-v4-pro"], "DeepSeek-V4-Pro-0813")

    def test_flash_prices(self):
        flash = self.snapshot.models[0]
        self.assertEqual(
            (flash.off_peak.hit, flash.off_peak.miss, flash.off_peak.out),
            (0.02, 1.0, 4.0),
        )
        self.assertEqual(
            (flash.peak.hit, flash.peak.miss, flash.peak.out),
            (0.04, 2.0, 8.0),
        )

    def test_pro_prices(self):
        pro = self.snapshot.models[1]
        self.assertEqual(
            (pro.off_peak.hit, pro.off_peak.miss, pro.off_peak.out),
            (0.15, 4.5, 13.5),
        )
        self.assertEqual(
            (pro.peak.hit, pro.peak.miss, pro.peak.out),
            (0.30, 9.0, 27.0),
        )

    def test_peak_is_double_off_peak_validated(self):
        """高峰=空闲×2 是官网脚注规则，解析后必须自校验通过。"""
        deepseek.assert_peak_double_off_peak(self.snapshot)

    def test_non_price_rows_ignored(self):
        """并发限制、上下文长度等行不得混入价格结果。"""
        for m in self.snapshot.models:
            for tier in (m.peak, m.off_peak):
                for v in (tier.hit, tier.miss, tier.out):
                    self.assertIsInstance(v, float)


class ParseDeepSeekEnTest(unittest.TestCase):
    """英文页解析契约（2026-09-10 恢复，与中文页同构两列表）：
    USD 模型集合、三档新价、峰=谷×2。"""

    @classmethod
    def setUpClass(cls):
        cls.snapshot = deepseek.parse_en_html(load_en_fixture())

    def test_provider_metadata(self):
        self.assertEqual(self.snapshot.provider, "deepseek")
        self.assertEqual(self.snapshot.currency, "USD")
        self.assertEqual(self.snapshot.source_url, deepseek.EN_URL)

    def test_model_ids_without_footnote_refs(self):
        self.assertEqual(
            [m.model_id for m in self.snapshot.models],
            ["deepseek-flash", "deepseek-v4-pro"],
        )

    def test_model_versions(self):
        by_id = {m.model_id: m.version for m in self.snapshot.models}
        self.assertEqual(by_id["deepseek-flash"], "DeepSeek-V4.1-Flash")
        self.assertEqual(by_id["deepseek-v4-pro"], "DeepSeek-V4-Pro-0813")

    def test_flash_prices(self):
        flash = self.snapshot.models[0]
        self.assertEqual(
            (flash.off_peak.hit, flash.off_peak.miss, flash.off_peak.out),
            (0.003, 0.15, 0.6),
        )
        self.assertEqual(
            (flash.peak.hit, flash.peak.miss, flash.peak.out),
            (0.006, 0.3, 1.2),
        )

    def test_pro_prices(self):
        pro = self.snapshot.models[1]
        self.assertEqual(
            (pro.off_peak.hit, pro.off_peak.miss, pro.off_peak.out),
            (0.022, 0.66, 1.98),
        )
        self.assertEqual(
            (pro.peak.hit, pro.peak.miss, pro.peak.out),
            (0.044, 1.32, 3.96),
        )

    def test_peak_is_double_off_peak_validated(self):
        deepseek.assert_peak_double_off_peak(self.snapshot)


class FailLoudTest(unittest.TestCase):
    """结构漂移必须抛 ParseError，绝不输出半截猜测数据。"""

    def _parse(self, html: str):
        return deepseek.parse_zh_html(html)

    def test_missing_peak_row_raises(self):
        html = load_zh_fixture()
        # 删除首个「高峰时段」行（输出档），三档缺一必须报错
        mutated = html.replace("<tr><td>高峰时段", "<tr><td>XX", 1)
        with self.assertRaises(deepseek.ParseError):
            self._parse(mutated)

    def test_non_numeric_price_raises(self):
        html = load_zh_fixture()
        mutated = html.replace("0.02元", "免费", 1)
        with self.assertRaises(deepseek.ParseError):
            self._parse(mutated)

    def test_price_cell_count_mismatch_raises(self):
        html = load_zh_fixture()
        # 删掉一个价格单元格，数字列数与模型数不符必须报错
        mutated = html.replace("0.02元", "", 1)
        with self.assertRaises(deepseek.ParseError):
            self._parse(mutated)

    def test_empty_html_raises(self):
        with self.assertRaises(deepseek.ParseError):
            self._parse("<html><body></body></html>")

    def test_peak_double_violation_raises(self):
        """高峰≠空闲×2 时自校验必须拦截（防解析错位）。"""
        html = load_zh_fixture()
        # 只改峰价不改谷价，破坏 2 倍关系；parse 内嵌自校验应立即拦截
        mutated = html.replace("0.04元", "0.05元", 1)
        with self.assertRaises(deepseek.ParseError):
            self._parse(mutated)

    def test_non_dollar_price_in_en_page_raises(self):
        """英文页价格必须是 $X 美元格式；出现「X元」即改版信号，fail loud。"""
        html = load_en_fixture()
        mutated = html.replace("$0.006", "0.006元", 1)
        with self.assertRaises(deepseek.ParseError):
            deepseek.parse_en_html(mutated)


class UsdChannelTest(unittest.TestCase):
    """英文页 2026-09-10 已从断供恢复：USD 通道经英文页快照产出候选；
    未知币种仍显式失败（绝不按汇率折算）。"""

    def test_usd_candidates_from_en_page(self):
        snapshot = deepseek.parse_en_html(load_en_fixture())
        self.assertEqual(snapshot.currency, "USD")
        self.assertEqual(
            [m.model_id for m in snapshot.models],
            ["deepseek-flash", "deepseek-v4-pro"],
        )

    def test_unknown_currency_unsupported(self):
        with self.assertRaises(deepseek.SourceUnavailable):
            deepseek.fetch(currency="EUR")


class ParseZhipuAppJsTest(unittest.TestCase):
    """智谱国内站解析契约：app.js 打包数据、促销双价、分档续行、无峰谷。"""

    @classmethod
    def setUpClass(cls):
        cls.snapshot = zhipu.parse_app_js(load_zhipu_fixture())

    def test_provider_metadata(self):
        self.assertEqual(self.snapshot.provider, "zhipu")
        self.assertEqual(self.snapshot.currency, "CNY")

    def test_glm_53_matches_preset(self):
        """GLM-5.3 三价与预置 zhipu_api (2.0, 8.0, 28.0) 对齐。"""
        glm53 = next(m for m in self.snapshot.models if m.model_id == "GLM-5.3")
        self.assertEqual(
            (glm53.off_peak.hit, glm53.off_peak.miss, glm53.off_peak.out),
            (2.0, 8.0, 28.0),
        )
        self.assertEqual(glm53.promo, "")
        self.assertIsNone(glm53.promo_original)

    def test_glm_53_flash_promo(self):
        """新模型 GLM-5.3-Flash：双价取现价，原价进 promo_original。"""
        flash = next(m for m in self.snapshot.models if m.model_id == "GLM-5.3-Flash")
        self.assertEqual(
            (flash.off_peak.hit, flash.off_peak.miss, flash.off_peak.out),
            (0.115, 0.4, 1.4),
        )
        self.assertEqual(flash.promo, "5折限时两周至09-09")
        self.assertEqual(
            (flash.promo_original.hit, flash.promo_original.miss, flash.promo_original.out),
            (0.23, 0.8, 2.8),
        )

    def test_glm_5_turbo_matches_preset(self):
        """GLM-5-Turbo 为分档计价模型：model_id 带档位后缀，
        预置 (1.2, 5.0, 22.0) 对应其第一档（输入长度 [0, 32)）。"""
        turbo = next(
            m
            for m in self.snapshot.models
            if m.model_id.startswith("GLM-5-Turbo") and "[0, 32)" in m.model_id
        )
        self.assertEqual(
            (turbo.off_peak.hit, turbo.off_peak.miss, turbo.off_peak.out),
            (1.2, 5.0, 22.0),
        )

    def test_tiered_context_rows_keep_model_identity(self):
        """「输入长度 [32+)」分档续行（name 为空）须归属前一模型并带档位标注。"""
        tiered = [
            m
            for m in self.snapshot.models
            if m.model_id.startswith("GLM-5.1") and "32+)" in m.model_id
        ]
        self.assertEqual(len(tiered), 1)
        self.assertEqual(tiered[0].off_peak.miss, 8.0)

    def test_no_peak_valley(self):
        """智谱按量无峰谷：peak 与 off_peak 同价。"""
        for m in self.snapshot.models:
            self.assertEqual(m.peak, m.off_peak)

    def test_non_numeric_price_raises(self):
        mutated = load_zhipu_fixture().replace('"0.4元"', '"免费"', 1)
        with self.assertRaises(zhipu.ParseError):
            zhipu.parse_app_js(mutated)

    def test_missing_hit_field_raises(self):
        mutated = load_zhipu_fixture().replace(',hit:["0.115元","0.23元"]', "", 1)
        with self.assertRaises(zhipu.ParseError):
            zhipu.parse_app_js(mutated)

    def test_shell_app_js_url_extraction(self):
        shell = (
            '<script src="https://at.alicdn.com/t/c/font_1.js"></script>'
            '<script src="https://static.bigmodel.cn/wd-paas-front/js/app.64d2f69c.js"></script>'
        )
        self.assertEqual(
            zhipu.extract_app_js_url(shell),
            "https://static.bigmodel.cn/wd-paas-front/js/app.64d2f69c.js",
        )

    def test_non_cny_unsupported(self):
        with self.assertRaises(zhipu.SourceUnavailable):
            zhipu.fetch(currency="USD")


class ParseZaiHtmlTest(unittest.TestCase):
    """Z.ai 国际站解析契约：SSG 表格、del 划线原价、无峰谷、无 GLM-5-Turbo。"""

    @classmethod
    def setUpClass(cls):
        cls.snapshot = zai.parse_en_html(load_zai_fixture())

    def test_provider_metadata(self):
        self.assertEqual(self.snapshot.provider, "zai")
        self.assertEqual(self.snapshot.currency, "USD")

    def test_glm_53_matches_preset(self):
        """GLM-5.3 三价与预置 zai_api (0.26, 1.4, 4.4) 对齐。"""
        glm53 = next(m for m in self.snapshot.models if m.model_id == "GLM-5.3")
        self.assertEqual(
            (glm53.off_peak.hit, glm53.off_peak.miss, glm53.off_peak.out),
            (0.26, 1.4, 4.4),
        )

    def test_glm_53_flash_strikethrough_original(self):
        """GLM-5.3-Flash 双价：<del>原价</del> 现价，取现价、原价存档。"""
        flash = next(m for m in self.snapshot.models if m.model_id == "GLM-5.3-Flash")
        self.assertEqual(
            (flash.off_peak.hit, flash.off_peak.miss, flash.off_peak.out),
            (0.015, 0.075, 0.25),
        )
        self.assertEqual(
            (flash.promo_original.hit, flash.promo_original.miss, flash.promo_original.out),
            (0.03, 0.15, 0.50),
        )

    def test_no_glm_5_turbo(self):
        """契约固化：国际站定价页无 GLM-5-Turbo（与预置 zai_api 的偏差，
        预置核对时人工裁决）。"""
        self.assertFalse(
            any(m.model_id == "GLM-5-Turbo" for m in self.snapshot.models)
        )

    def test_covers_latest_text_and_vision_tables(self):
        ids = {m.model_id for m in self.snapshot.models}
        self.assertTrue({"GLM-5.3-Flash", "GLM-5.3", "GLM-5.2"} <= ids)
        self.assertIn("GLM-4.7-FlashX", ids)  # Text Models 表
        self.assertIn("GLM-4.6V", ids)  # Vision Models 表

    def test_non_price_tables_excluded(self):
        """Built-in Tools / 图像 / 视频 / Agents 表不得混入。"""
        ids = {m.model_id for m in self.snapshot.models}
        self.assertNotIn("Web Search", ids)
        self.assertNotIn("GLM-Image", ids)

    def test_no_peak_valley(self):
        for m in self.snapshot.models:
            self.assertEqual(m.peak, m.off_peak)

    def test_non_dollar_price_raises(self):
        mutated = load_zai_fixture().replace(">$1.4<", ">1.4 元<", 1)
        with self.assertRaises(zai.ParseError):
            zai.parse_en_html(mutated)

    def test_non_usd_unsupported(self):
        with self.assertRaises(zai.SourceUnavailable):
            zai.fetch(currency="CNY")


class EntryRoutingTest(unittest.TestCase):
    """主入口路由契约。"""

    def test_registry_covers_all_providers(self):
        """注册表 = 已实现平台全集；新增平台在此登记。"""
        self.assertEqual(set(entry.PROVIDERS.keys()), {"deepseek", "zhipu", "zai"})

    def test_default_targets_is_all_providers(self):
        self.assertEqual(entry.default_targets(), ["deepseek", "zai", "zhipu"])

    def test_unknown_provider_rejected(self):
        with self.assertRaises(SystemExit):
            entry.parse_args(["nonexistent"])

    def test_provider_currencies(self):
        """币种展开契约：deepseek 双币（CNY 中文页 + USD 英文页），
        其余平台默认单币（未声明 SUPPORTED_CURRENCIES 时回退默认币种）。"""
        self.assertEqual(entry.provider_currencies(deepseek), ("CNY", "USD"))
        self.assertEqual(entry.provider_currencies(zai), ("USD",))
        self.assertEqual(entry.provider_currencies(zhipu), ("CNY",))


if __name__ == "__main__":
    unittest.main(verbosity=2)
