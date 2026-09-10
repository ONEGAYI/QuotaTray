import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { zh } from "../i18n/zh";
import type { NativeMeta, ProviderEntry } from "../types";
import { ProviderCard } from "./ProviderCard";

vi.mock("../i18n", () => ({ useLang: () => ({ lang: "zh", t: (key: keyof typeof zh) => zh[key] }) }));
vi.mock("../queries", () => ({
  useProviderQuery: () => ({ data: undefined, isFetching: false }),
  usePeakFlipTick: () => Date.UTC(2026, 8, 10),
}));

const meta: NativeMeta = {
  id: "deepseek", name: "DeepSeek", supports_plan_variant: false,
  uses_cli_credentials: false, uses_api_key2: false, console_url: null,
  custom_models: [], pricing_by_currency: {},
  pricing: {
    currency: "CNY", timezone_offset_minutes: 480, windows: [], default_model: "current",
    models: [
      { id: "current", display: "当前模型", plan: "pay_as_you_go", status: "active", windows: null, peak: { output: 2 }, off_peak: { output: 2 } },
      { id: "old", display: "旧模型", plan: "pay_as_you_go", status: "retired", windows: null, peak: { output: 77 }, off_peak: { output: 77 } },
    ],
  },
};

function render(model: string, suppliedMeta = meta) {
  const client = new QueryClient();
  const entry: ProviderEntry = { id: "test", name: "账户", kind: { type: "native", provider: "deepseek" }, enabled: true, pricing: { model } };
  const html = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <ProviderCard entry={entry} nativeMeta={suppliedMeta} intervalMinutes={5} thresholdPercent={10} onEdit={() => {}} />
    </QueryClientProvider>,
  );
  client.clear();
  return html;
}

describe("主窗定价的实际渲染", () => {
  it("可展开查看官方来源和真实核验日期", () => {
    const withSource: NativeMeta = {
      ...meta, pricing: { ...meta.pricing!, models: meta.pricing!.models.map((m) => ({ ...m, source_urls: ["https://example.com/pricing"], verified_at: "2026-09-09" })) },
    };
    const html = render("current", withSource);
    expect(html).toContain("官方模型资料");
    expect(html).toContain("https://example.com/pricing");
    expect(html).toContain("2026-09-09");
    expect(html).toMatch(/<button[^>]*type="button"[^>]*>https:\/\/example\.com\/pricing<\/button>/);
  });
  it("没有核验日期时明确显示未知", () => {
    expect(render("current")).toContain("核验时间未知");
  });
  it("下架模型保留原价并常显下架状态", () => {
    const html = render("old");
    expect(html).toContain("已下架");
    expect(html).toContain("77");
  });
  it("缺失模型明确显示价格未知", () => {
    expect(render("missing-A")).toContain("价格未知");
  });
});
