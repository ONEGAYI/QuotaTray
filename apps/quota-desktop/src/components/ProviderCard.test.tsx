import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";
import type { NativeMeta, ProviderEntry, SnapshotEntry } from "../types";
import { ProviderCard } from "./ProviderCard";

// 渲染语言可切换（zh/en 成对断言用）：vi.hoisted 先于模块求值初始化
const mockLang = vi.hoisted(() => ({ current: "zh" as "zh" | "en" }));
vi.mock("../i18n", () => ({
  useLang: () => ({
    lang: mockLang.current,
    t: (key: keyof typeof zh) => (mockLang.current === "zh" ? zh : en)[key],
  }),
}));
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

/** 主数值区与高亮方向（T-22 剩余口径）：经启动快照注入数据渲染，
 *  与主页真实链路同构（deriveProviderCardState 的 snapshot 分支）。
 *  primaryMetric 模拟条目级主度量偏好（T-24，缺省 = auto）。 */
function renderWithSnapshot(
  data: SnapshotEntry["data"],
  thresholdPercent: number,
  primaryMetric?: ProviderEntry["primary_metric"],
): string {
  const client = new QueryClient();
  const entry: ProviderEntry = {
    id: "test", name: "账户", kind: { type: "native", provider: "deepseek" }, enabled: true,
    primary_metric: primaryMetric,
  };
  const html = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <ProviderCard
        entry={entry}
        nativeMeta={meta}
        intervalMinutes={5}
        thresholdPercent={thresholdPercent}
        snapshot={{ data, at: Date.UTC(2026, 8, 26, 12, 0, 0) }}
        onEdit={() => {}}
      />
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

describe("主数值区与高亮方向（T-22 剩余口径）", () => {
  it("百分比主数值翻转为剩余：label「剩余额度」，值 58%（used 42）", () => {
    const html = renderWithSnapshot([{ used: 42, unit: "%" }], 20);
    expect(html).toContain("剩余额度");
    expect(html).toContain("58%");
    // 已用措辞不得残留于主数值区
    expect(html).not.toContain("已用");
  });

  it("多窗口 label 带剩余短标签：「剩余 5h」/「剩余 周限」", () => {
    const html = renderWithSnapshot(
      [
        { used: 42, unit: "%", plan_name: "GLM Coding Plan（5h）" },
        { used: 80, unit: "%", plan_name: "GLM Coding Plan（week）" },
      ],
      20,
    );
    expect(html).toContain("剩余 5h");
    expect(html).toContain("剩余 周限");
    expect(html).toContain("58%");
    expect(html).toContain("20%");
  });

  it("金额分支保留「可用余额」label（值本就是 remaining，无方向可翻）", () => {
    const html = renderWithSnapshot([{ remaining: 62.97, unit: "CNY" }], 20);
    expect(html).toContain("可用余额");
    expect(html).toContain("62.97");
  });

  it("红色高亮方向翻转：剩余 ≤ 阈值触发（与后端 breach 同时机）", () => {
    // used 42 → 剩余 58：阈值 60 触发、50 不触发、58 恰等触发、57 不触发
    expect(renderWithSnapshot([{ used: 42, unit: "%" }], 60)).toContain("has-balance-alert");
    expect(renderWithSnapshot([{ used: 42, unit: "%" }], 50)).not.toContain("has-balance-alert");
    expect(renderWithSnapshot([{ used: 42, unit: "%" }], 58)).toContain("is-alert");
    expect(renderWithSnapshot([{ used: 42, unit: "%" }], 57)).not.toContain("is-alert");
  });

  it("多窗口任一窗口剩余达标即高亮；百分比数据缺失不触发", () => {
    // 5h 剩余 58 不达标、week 剩余 20 ≤ 30 → 卡片高亮
    expect(
      renderWithSnapshot(
        [
          { used: 42, unit: "%", plan_name: "GLM Coding Plan（5h）" },
          { used: 80, unit: "%", plan_name: "GLM Coding Plan（week）" },
        ],
        30,
      ),
    ).toContain("has-balance-alert");
    // 金额窗口算不出剩余百分比：即使余额很低也不走高亮路径（与后端 breach 仅 % 口径一致）
    expect(renderWithSnapshot([{ remaining: 1.5, unit: "CNY" }], 20)).not.toContain("has-balance-alert");
  });
});

describe("主数值区主度量偏好分档（T-24）", () => {
  it("amount 档主数值优先金额：label「可用余额」，即使可算百分比", () => {
    // used/total 可换算 85% + remaining 有值 → auto 显示百分比、amount 显示金额
    const html = renderWithSnapshot(
      [{ used: 30, total: 200, remaining: 62.97, unit: "CNY" }],
      20,
      "amount",
    );
    expect(html).toContain("可用余额");
    expect(html).toContain("62.97");
    expect(html).not.toContain("剩余额度");
  });

  it("amount 档混合窗口逐窗口回退：金额窗口用金额、纯百分比窗口回退百分比", () => {
    const html = renderWithSnapshot(
      [
        { used: 30, total: 200, remaining: 62.97, unit: "CNY", plan_name: "GLM Coding Plan（MCP）" },
        { used: 42, unit: "%", plan_name: "GLM Coding Plan（5h）" },
      ],
      20,
      "amount",
    );
    // MCP 窗口有金额 → 金额值（多窗口金额档 label 带窗口名）
    expect(html).toContain("62.97");
    // 5h 窗口无 remaining → 静默回退百分比「剩余 5h」族
    expect(html).toContain("剩余 5h");
    expect(html).toContain("58%");
  });

  it("percent/auto 档维持推断基线：可算百分比优先（不回归）", () => {
    const both = [{ used: 30, total: 200, remaining: 62.97, unit: "CNY" }];
    expect(renderWithSnapshot(both, 20)).toContain("剩余额度");
    expect(renderWithSnapshot(both, 20, "auto")).toContain("85%");
    expect(renderWithSnapshot(both, 20, "percent")).toContain("85%");
  });
});

describe("主数值区英文措辞（PR #146 review：与 Rust i18n.rs 的 Left 成对）", () => {
  afterEach(() => {
    mockLang.current = "zh";
  });

  it("en 百分比 label 为 Left（单窗口），不残留 Remaining", () => {
    mockLang.current = "en";
    const html = renderWithSnapshot([{ used: 42, unit: "%" }], 20);
    expect(html).toContain("Left");
    expect(html).not.toContain("Remaining");
    expect(html).toContain("58%");
  });

  it("en 多窗口 label 带剩余短标注：Left 5h / Left weekly（与 zh 剩余 5h 成对）", () => {
    mockLang.current = "en";
    const html = renderWithSnapshot(
      [
        { used: 42, unit: "%", plan_name: "GLM Coding Plan（5h）" },
        { used: 80, unit: "%", plan_name: "GLM Coding Plan（week）" },
      ],
      20,
    );
    expect(html).toContain("Left 5h");
    expect(html).toContain("Left weekly");
    expect(html).not.toContain("Remaining");
  });
});
