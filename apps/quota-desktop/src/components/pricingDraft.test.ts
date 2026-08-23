import { describe, expect, it } from "vitest";
import type { PresetPricing, PricingConfig } from "../types";
import {
  buildPricing,
  draftFrom,
  formatUtcOffset,
  formatPrice,
  isFullCustom,
  selectedPresetModel,
} from "./pricingDraft";

const preset: PresetPricing = {
  timezone_offset_minutes: 480,
  currency: "CNY",
  default_model: "flash",
  windows: [{ days: ["mon", "tue"], start: "00:30", end: "08:30" }],
  models: [
    {
      id: "flash",
      display: "V4 Flash",
      plan: "pay_as_you_go",
      windows: null,
      peak: { cache_hit_input: 0.1, cache_miss_input: 2, output: 3 },
      off_peak: { cache_hit_input: 0.1, cache_miss_input: 1, output: 2 },
    },
    {
      id: "pro",
      display: "V4 Pro",
      plan: "pay_as_you_go",
      windows: null,
      peak: { cache_hit_input: 0.2, cache_miss_input: 3, output: 5 },
      off_peak: { cache_hit_input: 0.1, cache_miss_input: 1.5, output: 3 },
    },
  ],
};

describe("峰谷 GUI 草稿契约", () => {
  it("无自定义时保持平台默认模型且不生成 pricing", () => {
    const draft = draftFrom(undefined, preset);
    expect(draft.custom).toBe(false);
    expect(draft.scheduleCustom).toBe(false);
    expect(selectedPresetModel(preset, draft.model)?.id).toBe("flash");
    expect(buildPricing(draft, preset)).toBeUndefined();
  });

  it("只切换预置模型时仅保存 model", () => {
    const draft = { ...draftFrom(undefined, preset), model: "pro" };
    expect(buildPricing(draft, preset)).toEqual({ model: "pro" });
  });

  it("显式选择与默认 id 撞名的自定义模型时仍保存 model", () => {
    const draft = { ...draftFrom(undefined, preset), model: "flash" };
    expect(
      buildPricing(draft, preset, [{ id: "flash", display: "V4 Flash（自算）" }]),
    ).toEqual({ model: "flash" });
  });

  it("无预置平台关闭峰谷定价后不保留模型标签", () => {
    const draft = { ...draftFrom({ model: "自定义模型" }, null), custom: false };
    expect(buildPricing(draft, null)).toBeUndefined();
  });

  it("自定义模式可单独覆盖时段，其余字段继续省略", () => {
    const draft = {
      ...draftFrom(undefined, preset),
      custom: true,
      scheduleCustom: true,
      windows: [{ days: ["wed" as const], start: "09:00", end: "12:00" }],
    };
    expect(buildPricing(draft, preset)).toEqual({
      windows: [{ days: ["wed"], start: "09:00", end: "12:00" }],
    });
  });

  it("沿用预置时段时不保存草稿中的窗口", () => {
    const draft = {
      ...draftFrom(undefined, preset),
      custom: true,
      scheduleCustom: false,
      windows: [{ days: ["wed" as const], start: "09:00", end: "12:00" }],
    };
    expect(buildPricing(draft, preset)).toBeUndefined();
  });

  it("已有自定义配置可恢复为对应的界面状态", () => {
    const initial: PricingConfig = {
      model: "pro",
      timezone_offset_minutes: 0,
      currency: "USD",
      windows: [{ days: ["sat"], start: "10:00", end: "18:00" }],
      peak: { output: 8 },
    };
    const draft = draftFrom(initial, preset);
    expect(draft.custom).toBe(true);
    expect(draft.scheduleCustom).toBe(true);
    expect(draft.model).toBe("pro");
    expect(draft.tz).toBe("0");
    expect(draft.currency).toBe("USD");
    expect(draft.peakOut).toBe("8");
  });

  it("格式化 UTC 分钟偏移为易读文本", () => {
    expect(formatUtcOffset(480)).toBe("UTC+08:00");
    expect(formatUtcOffset(-330)).toBe("UTC−05:30");
    expect(formatUtcOffset(0)).toBe("UTC+00:00");
  });

  it("美元小额价格保留四位精度，避免高峰与空闲档撞价", () => {
    expect(formatPrice(0.014)).toBe("0.014");
    expect(formatPrice(0.007)).toBe("0.007");
    expect(formatPrice(0.1)).toBe("0.1");
  });

  it("时区覆盖也属于完整自定义", () => {
    expect(isFullCustom({ timezone_offset_minutes: 0 }, preset)).toBe(true);
  });
});
