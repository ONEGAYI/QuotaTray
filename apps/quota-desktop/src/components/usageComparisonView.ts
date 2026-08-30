import type { UsageComparisonSeries } from "../types";

export const MAX_USAGE_COMPARISONS = 4;

interface UnitBearingScope {
  metric: "percent" | "absolute";
  unit: string;
}

export interface UsageComparisonCandidateKey {
  providerId: string;
  windowKey: string;
}

export type AddUsageComparisonResult =
  | { ok: true; value: UsageComparisonSeries[] }
  | { ok: false; reason: "duplicate" | "limit" };

export function usageComparisonId(providerId: string, windowKey: string): string {
  return JSON.stringify([providerId, windowKey]);
}

export function initialUsageComparisons(
  stored: UsageComparisonSeries[] | null,
  candidates: UsageComparisonCandidateKey[],
): UsageComparisonSeries[] {
  if (stored !== null) return stored;
  const first = candidates[0];
  return first ? [{ provider_id: first.providerId, window_key: first.windowKey, color_slot: 0 }] : [];
}

export function addUsageComparison(
  current: UsageComparisonSeries[],
  candidate: UsageComparisonCandidateKey,
): AddUsageComparisonResult {
  if (current.some((item) => (
    item.provider_id === candidate.providerId && item.window_key === candidate.windowKey
  ))) return { ok: false, reason: "duplicate" };
  if (current.length >= MAX_USAGE_COMPARISONS) return { ok: false, reason: "limit" };
  const used = new Set(current.map((item) => item.color_slot));
  const colorSlot = [0, 1, 2, 3].find((slot) => !used.has(slot));
  if (colorSlot == null) return { ok: false, reason: "limit" };
  return {
    ok: true,
    value: [...current, {
      provider_id: candidate.providerId,
      window_key: candidate.windowKey,
      color_slot: colorSlot,
    }],
  };
}

export function removeUsageComparison(
  current: UsageComparisonSeries[],
  providerId: string,
  windowKey: string,
): UsageComparisonSeries[] {
  return current.filter((item) => (
    item.provider_id !== providerId || item.window_key !== windowKey
  ));
}

export function usageComparisonConflict(existingUnits: string[], candidateUnit: string): string | null {
  if (candidateUnit === "%") return null;
  const absoluteUnit = existingUnits.find((unit) => unit !== "%");
  return absoluteUnit && absoluteUnit !== candidateUnit ? absoluteUnit : null;
}

export function partitionCompatibleUsageScopes<T extends UnitBearingScope>(scopes: T[]): {
  visible: T[];
  hidden: T[];
  absoluteUnit: string | null;
} {
  const absoluteUnit = scopes.find((scope) => scope.metric === "absolute")?.unit ?? null;
  const visible = scopes.filter((scope) => scope.metric === "percent" || scope.unit === absoluteUnit);
  return {
    visible,
    hidden: scopes.filter((scope) => !visible.includes(scope)),
    absoluteUnit,
  };
}

export function detailComparisonIds(ids: string[], focusedId: string | null): string[] {
  return focusedId && ids.includes(focusedId) ? [focusedId] : ids;
}

export function shouldShowFocusedGap(focusedId: string | null, seriesId: string): boolean {
  return focusedId === seriesId;
}

export function usageTooltipDock(anchorY: number, chartHeight: number): "top" | "bottom" {
  return anchorY < chartHeight / 2 ? "bottom" : "top";
}
