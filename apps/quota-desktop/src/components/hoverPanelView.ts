import type { ProviderEntry, UsageData } from "../types";

export interface HoverRingView {
  fillPercent: number;
  center: string;
}

export function resolveHoverProvider(
  providers: ProviderEntry[],
  trayIconEntryId: string | null,
): ProviderEntry | null {
  const selected = trayIconEntryId
    ? providers.find((provider) => provider.id === trayIconEntryId && provider.enabled)
    : undefined;
  return selected ?? providers.find((provider) => provider.enabled) ?? null;
}

export function hoverRingView(
  data: UsageData | undefined,
  unitsPerCircle: number,
): HoverRingView | null {
  if (!data || data.is_valid === false) return null;

  let usedPercent: number | null = null;
  if (data.unit === "%" && data.used != null) usedPercent = data.used;
  else if (data.used != null && data.total != null && data.total > 0) {
    usedPercent = (data.used / data.total) * 100;
  }
  if (usedPercent != null && Number.isFinite(usedPercent)) {
    const remaining = Math.max(0, Math.min(100, 100 - usedPercent));
    return { fillPercent: remaining, center: `${Math.round(remaining)}%` };
  }

  if (data.remaining == null || !Number.isFinite(data.remaining)) return null;
  const remaining = Math.max(0, data.remaining);
  const perRing = Number.isFinite(unitsPerCircle) && unitsPerCircle > 0 ? unitsPerCircle : 100;
  const full = Math.floor(remaining / perRing);
  const fraction = (remaining % perRing) / perRing;
  const layerCount = full + (fraction > 0 ? 1 : 0);
  const fillPercent = layerCount > 4 ? 100 : fraction > 0 ? fraction * 100 : remaining > 0 ? 100 : 0;
  return { fillPercent, center: compactAmount(remaining) };
}

function compactAmount(value: number): string {
  const rounded = Math.round(value);
  if (rounded < 10_000) return String(rounded);
  if (rounded < 1_000_000) {
    const thousands = value / 1_000;
    return thousands >= 10 ? `${Math.round(thousands)}k` : `${Math.floor(thousands * 10) / 10}k`;
  }
  if (rounded < 1_000_000_000) {
    const millions = value / 1_000_000;
    return millions >= 10 ? `${Math.round(millions)}M` : `${Math.floor(millions * 10) / 10}M`;
  }
  const billions = value / 1_000_000_000;
  return billions >= 10 ? `${Math.round(billions)}B` : `${Math.floor(billions * 10) / 10}B`;
}
