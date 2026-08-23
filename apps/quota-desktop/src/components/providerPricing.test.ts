import { describe, expect, it } from "vitest";
import type { NativeMeta, ProviderEntry } from "../types";
import { isPeakAt, resolveProviderPricingView, withProviderModel } from "./providerPricing";

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
        peak: { cache_hit_input: 0.1, cache_miss_input: 3, output: 9 },
        off_peak: { cache_hit_input: 0.05, cache_miss_input: 1.5, output: 4.5 },
      },
      {
        id: "pro",
        display: "V4 Pro",
        peak: { cache_hit_input: 0.2, cache_miss_input: 5, output: 12 },
        off_peak: { cache_hit_input: 0.1, cache_miss_input: 2.5, output: 6 },
      },
    ],
  },
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
    expect(withProviderModel(customized, meta.pricing!, "pro")).toEqual({
      ...customized,
      pricing: { currency: "USD", windows: [], off_peak: { output: 7 }, model: "pro" },
    });
  });

  it("切回默认模型删除显式 model，空 pricing 同时清理", () => {
    expect(withProviderModel({ ...entry, pricing: { model: "pro" } }, meta.pricing!, "flash"))
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
