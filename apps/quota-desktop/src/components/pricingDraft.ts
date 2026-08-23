import type {
  PeakWindow,
  PresetModel,
  PresetPricing,
  PriceTier,
  PricingConfig,
} from "../types";

/** 价格与偏移保留为文本，允许用户编辑未完成的数值。 */
export interface PricingDraft {
  custom: boolean;
  scheduleCustom: boolean;
  model: string;
  tz: string;
  currency: string;
  windows: PeakWindow[];
  peakHit: string;
  peakMiss: string;
  peakOut: string;
  offHit: string;
  offMiss: string;
  offOut: string;
}

/** model-only（预置平台模型选择）不算完整自定义。 */
export function isFullCustom(
  pricing: PricingConfig | undefined,
  preset: PresetPricing | null,
): boolean {
  if (pricing == null) return false;
  return (
    pricing.windows != null ||
    pricing.peak != null ||
    pricing.off_peak != null ||
    pricing.currency != null ||
    pricing.timezone_offset_minutes != null ||
    (!preset && pricing.model != null)
  );
}

export function draftFrom(
  pricing: PricingConfig | undefined,
  preset: PresetPricing | null,
): PricingDraft {
  const tierStr = (tier: PriceTier | undefined, key: keyof PriceTier) =>
    tier?.[key] == null ? "" : String(tier[key]);
  return {
    custom: isFullCustom(pricing, preset),
    scheduleCustom: pricing?.windows != null,
    model: pricing?.model ?? "",
    tz:
      pricing?.timezone_offset_minutes == null
        ? ""
        : String(pricing.timezone_offset_minutes),
    currency: pricing?.currency ?? "",
    windows:
      pricing?.windows?.map((window) => ({
        days: [...window.days],
        start: window.start,
        end: window.end,
      })) ?? [],
    peakHit: tierStr(pricing?.peak, "cache_hit_input"),
    peakMiss: tierStr(pricing?.peak, "cache_miss_input"),
    peakOut: tierStr(pricing?.peak, "output"),
    offHit: tierStr(pricing?.off_peak, "cache_hit_input"),
    offMiss: tierStr(pricing?.off_peak, "cache_miss_input"),
    offOut: tierStr(pricing?.off_peak, "output"),
  };
}

/** 草稿转换为保存形状：未启用自定义时只保留非默认模型选择。 */
export function buildPricing(
  draft: PricingDraft,
  preset: PresetPricing | null,
): PricingConfig | undefined {
  const config: PricingConfig = {};
  const model = draft.model.trim();
  if (
    model &&
    !(preset != null && model.toLowerCase() === preset.default_model.toLowerCase())
  ) {
    config.model = model;
  }
  if (!draft.custom) {
    return preset && Object.keys(config).length ? config : undefined;
  }

  const timezone = draft.tz.trim();
  if (timezone !== "" && Number.isFinite(Number(timezone))) {
    config.timezone_offset_minutes = Number(timezone);
  }
  if (draft.currency.trim()) config.currency = draft.currency.trim();

  if (draft.scheduleCustom) {
    const windows = draft.windows.filter(
      (window) => window.days.length > 0 && window.start && window.end,
    );
    if (windows.length > 0) config.windows = windows;
  }

  const tier = (hit: string, miss: string, output: string): PriceTier | undefined => {
    const result: PriceTier = {};
    const entries: [keyof PriceTier, string][] = [
      ["cache_hit_input", hit],
      ["cache_miss_input", miss],
      ["output", output],
    ];
    for (const [key, raw] of entries) {
      const value = raw.trim();
      if (value !== "" && Number.isFinite(Number(value))) result[key] = Number(value);
    }
    return Object.keys(result).length ? result : undefined;
  };

  const peak = tier(draft.peakHit, draft.peakMiss, draft.peakOut);
  const offPeak = tier(draft.offHit, draft.offMiss, draft.offOut);
  if (peak) config.peak = peak;
  if (offPeak) config.off_peak = offPeak;
  return Object.keys(config).length ? config : undefined;
}

export function selectedPresetModel(
  preset: PresetPricing | null,
  model: string,
): PresetModel | undefined {
  if (!preset) return undefined;
  return (
    preset.models.find((item) => item.id.toLowerCase() === model.trim().toLowerCase()) ??
    preset.models.find((item) => item.id === preset.default_model)
  );
}

/** 价格展示：最多两位小数并去除尾零。 */
export function formatPrice(value: number | undefined): string {
  return value == null ? "—" : String(parseFloat(value.toFixed(2)));
}

/** 分钟偏移转换为 UTC±HH:MM。 */
export function formatUtcOffset(minutes: number): string {
  const sign = minutes < 0 ? "−" : "+";
  const absolute = Math.abs(minutes);
  const hours = String(Math.floor(absolute / 60)).padStart(2, "0");
  const remainder = String(absolute % 60).padStart(2, "0");
  return `UTC${sign}${hours}:${remainder}`;
}
