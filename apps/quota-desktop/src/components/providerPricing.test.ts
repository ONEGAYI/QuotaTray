import { describe, expect, it } from "vitest";
import type { NativeMeta, ProviderEntry } from "../types";
import {
  isPeakAt,
  pricingModelChoices,
  resolveProviderPricingView,
  withProviderModel,
} from "./providerPricing";

const meta: NativeMeta = {
  id: "deepseek",
  name: "DeepSeek",
  pricing: {
    currency: "CNY",
    timezone_offset_minutes: 480,
    default_model: "flash",
    windows: [{ days: ["wed"], start: "09:00", end: "12:00" }],
    models: [
      {
        id: "flash",
        display: "V4 Flash",
        plan: "pay_as_you_go",
        windows: null,
        peak: { cache_hit_input: 0.1, cache_miss_input: 3, output: 9 },
        off_peak: { cache_hit_input: 0.05, cache_miss_input: 1.5, output: 4.5 },
      },
      {
        id: "pro",
        display: "V4 Pro",
        plan: "pay_as_you_go",
        windows: null,
        peak: { cache_hit_input: 0.2, cache_miss_input: 5, output: 12 },
        off_peak: { cache_hit_input: 0.1, cache_miss_input: 2.5, output: 6 },
      },
    ],
  },
  pricing_by_currency: {},
    supports_plan_variant: false,
    uses_cli_credentials: false,
  custom_models: [],
};

const entry: ProviderEntry = {
  id: "DS1",
  name: "DeepSeek 主号",
  kind: { type: "native", provider: "deepseek" },
  enabled: true,
  api_key_enc: "v1:ciphertext",
};

describe("Provider 卡片定价视图", () => {
  it("默认模型在高峰窗口内解析高峰三档价格", () => {
    const now = Date.UTC(2026, 7, 19, 1, 30); // 周三，北京 09:30
    expect(resolveProviderPricingView(entry, meta, now)).toMatchObject({
      modelId: "flash",
      modelLabel: "V4 Flash",
      period: "peak",
      currency: "CNY",
      tier: { cache_hit_input: 0.1, cache_miss_input: 3, output: 9 },
    });
  });

  it("窗口外解析为空闲价格", () => {
    const now = Date.UTC(2026, 7, 19, 6, 0); // 周三，北京 14:00
    expect(resolveProviderPricingView(entry, meta, now)).toMatchObject({
      period: "off_peak",
      tier: { cache_hit_input: 0.05, cache_miss_input: 1.5, output: 4.5 },
    });
  });

  it("模型级窗口优先于平台级窗口，订阅项不伪造三档价格", () => {
    const subscriptionMeta: NativeMeta = {
      id: "zhipu",
      name: "智谱",
      pricing: {
        currency: "CNY",
        timezone_offset_minutes: 480,
        default_model: "glm-5.3",
        windows: [],
        models: [
          {
            id: "glm-5.3",
            display: "GLM-5.3",
            plan: "pay_as_you_go",
            windows: null,
            peak: { output: 28 },
            off_peak: { output: 28 },
          },
          {
            id: "coding-plan",
            display: "GLM Coding Plan",
            plan: "subscription",
            windows: [{ days: ["wed"], start: "14:00", end: "18:00" }],
            peak: {},
            off_peak: {},
          },
        ],
      },
      pricing_by_currency: {},
    supports_plan_variant: false,
    uses_cli_credentials: false,
      custom_models: [],
    };
    const coding = { ...entry, pricing: { model: "coding-plan" } };
    expect(
      resolveProviderPricingView(coding, subscriptionMeta, Date.UTC(2026, 7, 19, 7, 0)),
    ).toMatchObject({
      modelId: "coding-plan",
      plan: "subscription",
      period: "peak",
      tier: null,
    });
  });

  it("按查询结果币种选择 DeepSeek 对应预置套", () => {
    const usdMeta: NativeMeta = {
      ...meta,
      pricing_by_currency: {
        USD: {
          ...meta.pricing!,
          currency: "USD",
          models: meta.pricing!.models.map((model) => ({
            ...model,
            peak: { output: 1.1 },
            off_peak: { output: 0.55 },
          })),
        },
      },
    };
    expect(
      resolveProviderPricingView(entry, usdMeta, Date.UTC(2026, 7, 19, 6, 0), "usd"),
    ).toMatchObject({ currency: "USD", tier: { output: 0.55 } });
  });

  it("自定义模型库撞名优先，缺失窗口与币种回退平台级但缺失价格档不回退", () => {
    const libraryMeta: NativeMeta = {
      ...meta,
      custom_models: [
        {
          id: "flash",
          display: "V4 Flash（自算）",
          peak: { output: 9.1 },
        },
      ],
    };
    const explicit = { ...entry, pricing: { model: "FLASH" } };
    const view = resolveProviderPricingView(
      explicit,
      libraryMeta,
      Date.UTC(2026, 7, 19, 1, 30),
    );
    expect(view).toMatchObject({
      modelLabel: "V4 Flash（自算）",
      plan: "pay_as_you_go",
      period: "peak",
      currency: "CNY",
      tier: { output: 9.1 },
    });
    expect(
      resolveProviderPricingView(explicit, libraryMeta, Date.UTC(2026, 7, 19, 6, 0))?.tier,
    ).toBeNull();
  });

  it("无预置平台只在条目显式引用自定义模型时生成定价视图", () => {
    const libraryOnly: NativeMeta = {
      id: "siliconflow",
      name: "SiliconFlow",
      pricing: null,
      pricing_by_currency: {},
    supports_plan_variant: false,
    uses_cli_credentials: false,
      custom_models: [
        {
          id: "glm-5.2",
          display: "GLM-5.2 转售价",
          timezone_offset_minutes: 0,
          windows: [{ days: ["wed"], start: "09:00", end: "12:00" }],
          peak: { output: 3 },
          off_peak: { output: 1.5 },
          currency: "USD",
        },
      ],
    };
    const plain = { ...entry, kind: { type: "native" as const, provider: "siliconflow" } };
    expect(resolveProviderPricingView(plain, libraryOnly)).toBeNull();
    expect(
      resolveProviderPricingView(
        { ...plain, pricing: { model: "GLM-5.2" } },
        libraryOnly,
        Date.UTC(2026, 7, 19, 10, 0),
      ),
    ).toMatchObject({
      modelLabel: "GLM-5.2 转售价",
      period: "peak",
      currency: "USD",
      tier: { output: 3 },
    });
  });

  it("模型选项保留隐式官方默认项，并给撞名自定义模型独立显式选项", () => {
    const choices = pricingModelChoices(meta.pricing, [
      { id: "flash", display: "V4 Flash（自算）", peak: { output: 9.1 } },
    ]);
    expect(choices.map((choice) => [choice.value, choice.label, choice.source])).toEqual([
      ["default", "V4 Flash", "preset"],
      ["model:flash", "V4 Flash（自算）", "custom"],
      ["model:pro", "V4 Pro", "preset"],
    ]);
  });

  it("模型匹配大小写不敏感，自定义整档价格覆盖预置", () => {
    const customized: ProviderEntry = {
      ...entry,
      pricing: {
        model: "PRO",
        peak: { output: 20 },
      },
    };
    const now = Date.UTC(2026, 7, 19, 1, 30);
    expect(resolveProviderPricingView(customized, meta, now)).toEqual({
      modelId: "pro",
      modelLabel: "V4 Pro",
      period: "peak",
      tier: { output: 20 },
      currency: "CNY",
      plan: "pay_as_you_go",
    });
  });

  it("接受 core 同样容忍的单数字时刻与首尾空白", () => {
    const customized: ProviderEntry = {
      ...entry,
      pricing: { windows: [{ days: ["wed"], start: " 9:00 ", end: " 12:0 " }] },
    };
    expect(resolveProviderPricingView(customized, meta, Date.UTC(2026, 7, 19, 1, 30))?.period)
      .toBe("peak");
  });

  it("无预置且 pricing 为空对象时不生成定价视图", () => {
    const template: ProviderEntry = {
      ...entry,
      kind: { type: "template", request: { url: "https://example.com" }, windows: [] },
      pricing: {},
    };
    expect(resolveProviderPricingView(template, undefined)).toBeNull();
  });

  it("只有自定义模型标签且无价格时保留标签但不伪造空价格档", () => {
    const template: ProviderEntry = {
      ...entry,
      kind: { type: "template", request: { url: "https://example.com" }, windows: [] },
      pricing: { model: "custom-model" },
    };
    expect(resolveProviderPricingView(template, undefined)).toMatchObject({
      modelLabel: "custom-model",
      tier: null,
    });
  });

  it("未知模型作为展示标签，价格回退默认模型", () => {
    const customized: ProviderEntry = { ...entry, pricing: { model: "我的模型" } };
    const view = resolveProviderPricingView(customized, meta, Date.UTC(2026, 7, 19, 6, 0));
    expect(view).toMatchObject({ modelId: "flash", modelLabel: "我的模型" });
    expect(view?.tier?.output).toBe(4.5);
  });

  it("切换非默认模型保留其他覆盖与密文", () => {
    const customized: ProviderEntry = {
      ...entry,
      pricing: { currency: "USD", windows: [], off_peak: { output: 7 } },
    };
    expect(withProviderModel(customized, "pro")).toEqual({
      ...customized,
      pricing: { currency: "USD", windows: [], off_peak: { output: 7 }, model: "pro" },
    });
  });

  it("切回默认模型删除显式 model，空 pricing 同时清理", () => {
    expect(withProviderModel({ ...entry, pricing: { model: "pro" } }, null))
      .toEqual(entry);
  });
});

describe("峰谷窗口边界", () => {
  const wednesday = Date.UTC(2026, 7, 19, 0, 0);

  it("窗口使用左闭右开边界", () => {
    const windows = [{ days: ["wed" as const], start: "09:00", end: "12:00" }];
    expect(isPeakAt(windows, 0, wednesday + 9 * 3_600_000)).toBe(true);
    expect(isPeakAt(windows, 0, wednesday + 12 * 3_600_000)).toBe(false);
  });

  it("24:00 可作为当天结束上界", () => {
    const windows = [{ days: ["wed" as const], start: "23:00", end: "24:00" }];
    expect(isPeakAt(windows, 0, wednesday + 23 * 3_600_000 + 59 * 60_000)).toBe(true);
  });

  it("空窗口始终为空闲", () => {
    expect(isPeakAt([], undefined, wednesday)).toBe(false);
  });
});
