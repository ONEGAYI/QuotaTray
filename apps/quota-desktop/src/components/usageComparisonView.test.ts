import { describe, expect, it } from "vitest";
import type { UsageComparisonSeries } from "../types";
import {
  addUsageComparison,
  detailComparisonIds,
  initialUsageComparisons,
  partitionCompatibleUsageScopes,
  removeUsageComparison,
  resolveUsageComparisonMetrics,
  shouldShowFocusedGap,
  usageComparisonConflict,
  usageTooltipDock,
  usageComparisonId,
} from "./usageComparisonView";

const base: UsageComparisonSeries[] = [
  { provider_id: "p1", window_key: "w1", color_slot: 2, metric: "percent" },
  { provider_id: "p2", window_key: "w2", color_slot: 0, metric: "absolute" },
];

describe("使用统计比较组合逻辑", () => {
  it("新增组合分配最低空闲色槽：同窗口异度量可共存，同度量重复与四条上限被拒绝", () => {
    expect(addUsageComparison(base, { providerId: "p3", windowKey: "w3", metric: "percent" })).toEqual({
      ok: true,
      value: [...base, { provider_id: "p3", window_key: "w3", metric: "percent", color_slot: 1 }],
    });
    expect(addUsageComparison(base, { providerId: "p1", windowKey: "w1", metric: "percent" }).ok).toBe(false);
    // 同窗口另一度量是独立组合项（issue #143 双产），占独立色槽
    expect(addUsageComparison(base, { providerId: "p1", windowKey: "w1", metric: "absolute" })).toEqual({
      ok: true,
      value: [...base, { provider_id: "p1", window_key: "w1", metric: "absolute", color_slot: 1 }],
    });
    expect(addUsageComparison([
      ...base,
      { provider_id: "p3", window_key: "w3", metric: "percent", color_slot: 1 },
      { provider_id: "p4", window_key: "w4", metric: "absolute", color_slot: 3 },
    ], { providerId: "p5", windowKey: "w5", metric: "percent" }).ok).toBe(false);
  });

  it("组合 ID 含度量维度：同窗口两条度量不撞键，分隔控制字符仍无拼接碰撞", () => {
    expect(usageComparisonId("p1", "w1", "percent")).not.toBe(usageComparisonId("p1", "w1", "absolute"));
    expect(usageComparisonId("a\u0000b", "c", "percent")).not.toBe(usageComparisonId("a", "b\u0000c", "percent"));
  });

  it("删除仅移除指定度量的组合并保留同窗口另一度量", () => {
    expect(removeUsageComparison(base, "p1", "w1", "percent")).toEqual([base[1]]);
    const dual: UsageComparisonSeries[] = [
      { provider_id: "p1", window_key: "w1", color_slot: 2, metric: "percent" },
      { provider_id: "p1", window_key: "w1", color_slot: 3, metric: "absolute" },
    ];
    expect(removeUsageComparison(dual, "p1", "w1", "percent")).toEqual([dual[1]]);
  });

  it("未初始化时自动选择首个候选（携带度量），显式空数组保持空态", () => {
    expect(initialUsageComparisons(null, [{ providerId: "p1", windowKey: "w1", metric: "absolute" }])).toEqual([
      { provider_id: "p1", window_key: "w1", metric: "absolute", color_slot: 0 },
    ]);
    expect(initialUsageComparisons([], [{ providerId: "p1", windowKey: "w1", metric: "percent" }])).toEqual([]);
    expect(initialUsageComparisons(null, [])).toEqual([]);
  });

  it("存量无 metric 组合按现有派生回填度量：percent 优先、无百分比退金额、无候选保持缺省", () => {
    const candidates = [
      { providerId: "p1", windowKey: "w1", metric: "percent" as const },
      { providerId: "p1", windowKey: "w1", metric: "absolute" as const },
      { providerId: "p2", windowKey: "w2", metric: "absolute" as const },
    ];
    expect(resolveUsageComparisonMetrics([
      { provider_id: "p1", window_key: "w1", color_slot: 0 },
      { provider_id: "p2", window_key: "w2", color_slot: 1 },
      { provider_id: "p9", window_key: "w9", color_slot: 2 },
      { provider_id: "p1", window_key: "w1", color_slot: 3, metric: "absolute" },
    ], candidates)).toEqual([
      { provider_id: "p1", window_key: "w1", color_slot: 0, metric: "percent" },
      { provider_id: "p2", window_key: "w2", color_slot: 1, metric: "absolute" },
      { provider_id: "p9", window_key: "w9", color_slot: 2 },
      { provider_id: "p1", window_key: "w1", color_slot: 3, metric: "absolute" },
    ]);
  });

  it("百分比可与一种绝对单位共存，不允许第二种绝对单位", () => {
    expect(usageComparisonConflict(["%", "CNY"], "%")).toBeNull();
    expect(usageComparisonConflict(["%", "CNY"], "CNY")).toBeNull();
    expect(usageComparisonConflict(["%", "CNY"], "USD")).toBe("CNY");
  });

  it("聚焦后详情只返回聚焦项，清除后返回全量", () => {
    expect(detailComparisonIds(["a", "b", "c"], "b")).toEqual(["b"]);
    expect(detailComparisonIds(["a", "b", "c"], null)).toEqual(["a", "b", "c"]);
    expect(detailComparisonIds(["a", "b", "c"], "missing")).toEqual(["a", "b", "c"]);
  });

  it("迁移带来的第二种绝对单位会被分区并可向用户报告", () => {
    const scopes = [
      { id: "percent", metric: "percent" as const, unit: "%" },
      { id: "cny", metric: "absolute" as const, unit: "CNY" },
      { id: "usd", metric: "absolute" as const, unit: "USD" },
    ];
    expect(partitionCompatibleUsageScopes(scopes)).toEqual({
      visible: scopes.slice(0, 2),
      hidden: [scopes[2]],
      absoluteUnit: "CNY",
    });
  });

  it("删除不存在的组合保持原数组语义不变", () => {
    expect(removeUsageComparison(base, "missing", "missing", "percent")).toEqual(base);
  });

  it("长期缺失灰区只在单条聚焦时显示于聚焦项", () => {
    expect(shouldShowFocusedGap(null, "a")).toBe(false);
    expect(shouldShowFocusedGap("a", "b")).toBe(false);
    expect(shouldShowFocusedGap("a", "a")).toBe(true);
  });

  it("气泡吸附到光标相反半区", () => {
    expect(usageTooltipDock(80, 400)).toBe("bottom");
    expect(usageTooltipDock(320, 400)).toBe("top");
    expect(usageTooltipDock(200, 400)).toBe("top");
  });
});
