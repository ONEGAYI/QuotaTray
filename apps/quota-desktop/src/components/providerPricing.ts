import type {
  NativeMeta,
  PeakWindow,
  PresetPricing,
  PriceTier,
  PricingConfig,
  ProviderEntry,
  Weekday,
} from "../types";

export interface ProviderPricingView {
  modelId?: string;
  modelLabel?: string;
  period: "peak" | "off_peak";
  tier: PriceTier | null;
  currency?: string;
}

const DAY_BY_INDEX: Weekday[] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

function tierNotEmpty(tier: PriceTier | undefined): tier is PriceTier {
  return Boolean(
    tier &&
      (tier.cache_hit_input != null || tier.cache_miss_input != null || tier.output != null),
  );
}

function pricingNotEmpty(pricing: PricingConfig): boolean {
  return (
    pricing.model != null ||
    pricing.timezone_offset_minutes != null ||
    pricing.windows != null ||
    tierNotEmpty(pricing.peak) ||
    tierNotEmpty(pricing.off_peak) ||
    pricing.currency != null
  );
}

function parseMinutes(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed === "24:00") return 24 * 60;
  const parts = trimmed.split(":");
  if (parts.length !== 2) return null;
  const hourText = parts[0].trim();
  const minuteText = parts[1].trim();
  if (!/^\d+$/.test(hourText) || !/^\d+$/.test(minuteText)) return null;
  const hours = Number(hourText);
  const minutes = Number(minuteText);
  if (hours > 23 || minutes > 59) return null;
  return hours * 60 + minutes;
}

function clockAt(nowMs: number, timezoneOffsetMinutes: number | undefined) {
  if (timezoneOffsetMinutes == null) {
    const local = new Date(nowMs);
    return {
      day: DAY_BY_INDEX[local.getDay()],
      minutes: local.getHours() * 60 + local.getMinutes(),
    };
  }
  const shifted = new Date(nowMs + timezoneOffsetMinutes * 60_000);
  return {
    day: DAY_BY_INDEX[shifted.getUTCDay()],
    minutes: shifted.getUTCHours() * 60 + shifted.getUTCMinutes(),
  };
}

export function isPeakAt(
  windows: PeakWindow[],
  timezoneOffsetMinutes: number | undefined,
  nowMs: number,
): boolean {
  const clock = clockAt(nowMs, timezoneOffsetMinutes);
  return windows.some((window) => {
    if (!window.days.includes(clock.day)) return false;
    const start = parseMinutes(window.start);
    const end = parseMinutes(window.end);
    return start != null && end != null && start < end && clock.minutes >= start && clock.minutes < end;
  });
}

/** 前端只读镜像 core pricing::resolve，用于 Provider 卡片即时展示。 */
export function resolveProviderPricingView(
  entry: ProviderEntry,
  nativeMeta: NativeMeta | undefined,
  nowMs = Date.now(),
): ProviderPricingView | null {
  const preset = nativeMeta?.pricing ?? null;
  const custom = entry.pricing;
  if (!preset && (!custom || !pricingNotEmpty(custom))) return null;

  const requestedModel = custom?.model;
  const matchedModel = requestedModel
    ? preset?.models.find((model) => model.id.toLowerCase() === requestedModel.toLowerCase())
    : undefined;
  const defaultModel = preset?.models.find((model) => model.id === preset.default_model);
  const model = matchedModel ?? defaultModel;
  const modelLabel = matchedModel?.display ?? requestedModel ?? defaultModel?.display;
  const windows = custom?.windows ?? preset?.windows ?? [];
  const timezoneOffsetMinutes =
    custom?.timezone_offset_minutes ?? preset?.timezone_offset_minutes;
  const period = isPeakAt(windows, timezoneOffsetMinutes, nowMs) ? "peak" : "off_peak";
  const customTier = period === "peak" ? custom?.peak : custom?.off_peak;
  const presetTier = period === "peak" ? model?.peak : model?.off_peak;
  const tier = tierNotEmpty(customTier) ? customTier : (presetTier ?? null);

  return {
    modelId: model?.id,
    modelLabel,
    period,
    tier,
    currency: custom?.currency ?? preset?.currency,
  };
}

/** 模型即时切换：默认模型省略字段，且空 pricing 不落盘。 */
export function withProviderModel(
  entry: ProviderEntry,
  preset: PresetPricing,
  modelId: string,
): ProviderEntry {
  const pricing: PricingConfig = { ...(entry.pricing ?? {}) };
  if (modelId.toLowerCase() === preset.default_model.toLowerCase()) {
    delete pricing.model;
  } else {
    pricing.model = modelId;
  }
  if (!pricingNotEmpty(pricing)) {
    const { pricing: _pricing, ...withoutPricing } = entry;
    return withoutPricing;
  }
  return { ...entry, pricing };
}
