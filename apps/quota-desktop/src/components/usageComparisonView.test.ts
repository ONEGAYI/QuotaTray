import { describe, expect, it } from "vitest";
import type { UsageComparisonSeries } from "../types";
import {
  addUsageComparison,
  detailComparisonIds,
  initialUsageComparisons,
  partitionCompatibleUsageScopes,
  removeUsageComparison,
  shouldShowFocusedGap,
  usageComparisonConflict,
  usageTooltipDock,
  usageComparisonId,
} from "./usageComparisonView";

const base: UsageComparisonSeries[] = [
  { provider_id: "p1", window_key: "w1", color_slot: 2 },
  { provider_id: "p2", window_key: "w2", color_slot: 0 },
];

describe("使用统计比较组合逻辑", () => {
  it("新增组合分配最低空闲色槽，重复与四条上限被拒绝", () => {
    expect(addUsageComparison(base, { providerId: "p3", windowKey: "w3" })).toEqual({
      ok: true,
      value: [...base, { provider_id: "p3", window_key: "w3", color_slot: 1 }],
    });
    expect(addUsageComparison(base, { providerId: "p1", windowKey: "w1" }).ok).toBe(false);
    expect(addUsageComparison([
      ...base,
      { provider_id: "p3", window_key: "w3", color_slot: 1 },
      { provider_id: "p4", window_key: "w4", color_slot: 3 },
    ], { providerId: "p5", windowKey: "w5" }).ok).toBe(false);
  });

  it("组合 ID 对包含分隔控制字符的键仍无拼接碰撞", () => {
    expect(usageComparisonId("a\u0000b", "c")).not.toBe(usageComparisonId("a", "b\u0000c"));
  });

  it("删除仅移除指定组合并保留其余色槽", () => {
    expect(removeUsageComparison(base, "p1", "w1")).toEqual([base[1]]);
  });

  it("未初始化时自动选择首个候选，显式空数组保持空态", () => {
    expect(initialUsageComparisons(null, [{ providerId: "p1", windowKey: "w1" }])).toEqual([
      { provider_id: "p1", window_key: "w1", color_slot: 0 },
    ]);
    expect(initialUsageComparisons([], [{ providerId: "p1", windowKey: "w1" }])).toEqual([]);
    expect(initialUsageComparisons(null, [])).toEqual([]);
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
    expect(removeUsageComparison(base, "missing", "missing")).toEqual(base);
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
