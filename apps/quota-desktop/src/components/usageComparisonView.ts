import type { UsageComparisonSeries } from "../types";
import type { UsageMetricType } from "./usageChartView";

export const MAX_USAGE_COMPARISONS = 4;

interface UnitBearingScope {
  metric: "percent" | "absolute";
  unit: string;
}

export interface UsageComparisonCandidateKey {
  providerId: string;
  windowKey: string;
  metric: UsageMetricType;
}

export type AddUsageComparisonResult =
  | { ok: true; value: UsageComparisonSeries[] }
  | { ok: false; reason: "duplicate" | "limit" };

/** 组合唯一键含度量维度（issue #143 双产）：同窗口的金额与百分比是两个
 *  独立组合项。metric 缺省（存量未解析形态，如窗口已无候选的组合）落入
 *  null 槽位，与任一显式度量不撞键。 */
export function usageComparisonId(providerId: string, windowKey: string, metric?: UsageMetricType): string {
  return JSON.stringify([providerId, windowKey, metric]);
}

export function initialUsageComparisons(
  stored: UsageComparisonSeries[] | null,
  candidates: UsageComparisonCandidateKey[],
): UsageComparisonSeries[] {
  if (stored !== null) return stored;
  const first = candidates[0];
  return first
    ? [{ provider_id: first.providerId, window_key: first.windowKey, metric: first.metric, color_slot: 0 }]
    : [];
}

/** 存量无 metric 的组合按现有单选派生回填度量（percent 优先、无百分比
 *  原材料退金额）：用于本轮匹配与展示；回填不主动写盘，但回填产物是
 *  增删组合操作的基底数组——用户增删组合保存时随选区整体显式化落盘
 *  （惰性迁移语义，PR #146 review 修正口径）。窗口无候选（Provider
 *  已删或无数据）时保持缺省，图表侧按不可绘制处理。 */
export function resolveUsageComparisonMetrics(
  selections: readonly UsageComparisonSeries[],
  candidates: readonly UsageComparisonCandidateKey[],
): UsageComparisonSeries[] {
  return selections.map((selection) => {
    if (selection.metric != null) return selection;
    for (const metric of ["percent", "absolute"] as const) {
      if (candidates.some((candidate) => (
        candidate.providerId === selection.provider_id
        && candidate.windowKey === selection.window_key
        && candidate.metric === metric
      ))) {
        return { ...selection, metric };
      }
    }
    return selection;
  });
}

export function addUsageComparison(
  current: UsageComparisonSeries[],
  candidate: UsageComparisonCandidateKey,
): AddUsageComparisonResult {
  if (current.some((item) => (
    item.provider_id === candidate.providerId
    && item.window_key === candidate.windowKey
    && item.metric === candidate.metric
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
      metric: candidate.metric,
      color_slot: colorSlot,
    }],
  };
}

export function removeUsageComparison(
  current: UsageComparisonSeries[],
  providerId: string,
  windowKey: string,
  metric?: UsageMetricType,
): UsageComparisonSeries[] {
  return current.filter((item) => (
    item.provider_id !== providerId || item.window_key !== windowKey || item.metric !== metric
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
