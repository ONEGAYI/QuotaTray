import type {
  CustomModelDef,
  NativeMeta,
  PeakWindow,
  PlanKind,
  PresetPricing,
  PriceTier,
  PricingConfig,
  ProviderEntry,
  Weekday,
} from "../types";

export interface ProviderPricingView {
  modelId?: string;
  modelLabel?: string;
  /** 生效模型生命周期（与 core ResolvedModelStatus 同口径）：
   * active/retired = 命中官方模型（retired 显示最后已知价）；
   * missing = 显式指定但未命中（价格未知，不借默认模型）；
   * custom = 自定义库模型或无预置纯自定义。 */
  modelStatus: "active" | "retired" | "missing" | "custom";
  period: "peak" | "off_peak";
  tier: PriceTier | null;
  currency?: string;
  plan: PlanKind;
}

export interface PricingModelChoice {
  value: string;
  modelId: string | null;
  label: string;
  plan: PlanKind;
  source: "preset" | "custom";
  /** retired = 已下架（列表默认不提供；仅当前值引用时保留并标注）。 */
  status: "active" | "retired" | "custom";
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

function presetForCurrency(nativeMeta: NativeMeta | undefined, currency: string | undefined) {
  const normalized = currency?.trim().toUpperCase();
  return (normalized ? nativeMeta?.pricing_by_currency?.[normalized] : undefined)
    ?? nativeMeta?.pricing
    ?? null;
}

export function pricingModelChoices(
  preset: PresetPricing | null,
  customModels: CustomModelDef[],
  /** 当前条目引用的模型 id（大小写不敏感）：retired 当前值保留在列表
   * 尾部并标注已下架，避免打开编辑页即丢失配置（spec §5.2）。 */
  currentModelId?: string,
): PricingModelChoice[] {
  const choices: PricingModelChoice[] = [];
  const defaultModel = preset?.models.find((model) => model.id === preset.default_model);
  if (defaultModel) {
    choices.push({
      value: "default",
      modelId: null,
      label: defaultModel.display,
      plan: defaultModel.plan,
      source: "preset",
      status: "active",
    });
  }
  for (const model of customModels) {
    choices.push({
      value: `model:${model.id}`,
      modelId: model.id,
      label: model.display,
      plan: "pay_as_you_go",
      source: "custom",
      status: "custom",
    });
  }
  const customIds = new Set(customModels.map((model) => model.id.toLowerCase()));
  for (const model of preset?.models ?? []) {
    if (model.id === preset?.default_model || customIds.has(model.id.toLowerCase())) continue;
    // 新选择列表默认只提供 active 模型；retired 不主动列出
    if (model.status === "retired") continue;
    choices.push({
      value: `model:${model.id}`,
      modelId: model.id,
      label: model.display,
      plan: model.plan,
      source: "preset",
      status: "active",
    });
  }
  // 当前值引用的 retired 模型：追加保留（不丢配置），展示层标注已下架
  if (currentModelId) {
    const current = preset?.models.find(
      (model) =>
        model.status === "retired" &&
        model.id.toLowerCase() === currentModelId.toLowerCase(),
    );
    if (current && !choices.some((c) => c.modelId?.toLowerCase() === current.id.toLowerCase())) {
      choices.push({
        value: `model:${current.id}`,
        modelId: current.id,
        label: current.display,
        plan: current.plan,
        source: "preset",
        status: "retired",
      });
    }
  }
  return choices;
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
  usageCurrency?: string,
): ProviderPricingView | null {
  const preset = presetForCurrency(nativeMeta, usageCurrency);
  const library = nativeMeta?.custom_models ?? [];
  const custom = entry.pricing;
  if (!preset && (!custom || !pricingNotEmpty(custom))) return null;

  // 模型选择链（与 core resolve_impl 同口径，T-02 语义）：
  // 自定义库（撞名优先）→ active/retired 官方（数据照常生效）→
  // 显式指定未命中 = missing（不借默认模型的价格/模型级时段/计费模式）；
  // 只有未指定模型时才用 active 默认模型。
  const requestedModel = custom?.model;
  const libraryModel = requestedModel
    ? library.find((model) => model.id.toLowerCase() === requestedModel.toLowerCase())
    : undefined;
  const presetModel = requestedModel
    ? preset?.models.find((model) => model.id.toLowerCase() === requestedModel.toLowerCase())
    : undefined;
  const defaultModel = preset?.models.find((model) => model.id === preset.default_model);
  // 显式指定未命中 → 不选中任何模型（missing：价格/时段/计费不借默认）
  const model = libraryModel
    ?? presetModel
    ?? (requestedModel ? undefined : defaultModel);
  const modelStatus: ProviderPricingView["modelStatus"] = libraryModel
    ? "custom"
    : presetModel
      ? presetModel.status === "retired"
        ? "retired"
        : "active"
      : requestedModel
        ? "missing"
        : defaultModel
          ? "active"
          : "custom";
  const modelLabel = (libraryModel ?? presetModel)?.display ?? requestedModel ?? defaultModel?.display;
  const modelWindows = model?.windows ?? undefined;
  const windows = custom?.windows ?? modelWindows ?? preset?.windows ?? [];
  const timezoneOffsetMinutes =
    custom?.timezone_offset_minutes
    ?? (libraryModel ? libraryModel.timezone_offset_minutes : undefined)
    ?? preset?.timezone_offset_minutes;
  const period = isPeakAt(windows, timezoneOffsetMinutes, nowMs) ? "peak" : "off_peak";
  const customTier = period === "peak" ? custom?.peak : custom?.off_peak;
  const modelTier = period === "peak" ? model?.peak : model?.off_peak;
  const tier = tierNotEmpty(customTier)
    ? customTier
    : tierNotEmpty(modelTier) ? modelTier : null;
  // 计费模式只从命中的模型取（missing 不借默认的订阅计费）
  const plan: PlanKind = presetModel
    ? presetModel.plan
    : !requestedModel && defaultModel
      ? defaultModel.plan
      : "pay_as_you_go";

  return {
    modelId: model?.id,
    modelLabel,
    modelStatus,
    period,
    tier,
    currency: custom?.currency ?? libraryModel?.currency ?? preset?.currency,
    plan,
  };
}

/** 模型即时切换：默认模型省略字段，且空 pricing 不落盘。 */
export function withProviderModel(
  entry: ProviderEntry,
  modelId: string | null,
): ProviderEntry {
  const pricing: PricingConfig = { ...(entry.pricing ?? {}) };
  if (modelId == null) {
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
