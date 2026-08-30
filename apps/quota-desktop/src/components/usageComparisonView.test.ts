import { describe, expect, it } from "vitest";
import type { UsageComparisonSeries } from "../types";
import {
  addUsageComparison,
  detailComparisonIds,
  initialUsageComparisons,
  removeUsageComparison,
  usageComparisonConflict,
  usageTooltipDock,
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

  it("删除仅移除指定组合并保留其余色槽", () => {
    expect(removeUsageComparison(base, "p1", "w1")).toEqual([base[1]]);
  });

  it("未初始化时自动选择首个候选，显式空数组保持空态", () => {
    expect(initialUsageComparisons(null, [{ providerId: "p1", windowKey: "w1" }])).toEqual([
      { provider_id: "p1", window_key: "w1", color_slot: 0 },
    ]);
    expect(initialUsageComparisons([], [{ providerId: "p1", windowKey: "w1" }])).toEqual([]);
  });

  it("百分比可与一种绝对单位共存，不允许第二种绝对单位", () => {
    expect(usageComparisonConflict(["%", "CNY"], "%")).toBeNull();
    expect(usageComparisonConflict(["%", "CNY"], "CNY")).toBeNull();
    expect(usageComparisonConflict(["%", "CNY"], "USD")).toBe("CNY");
  });

  it("聚焦后详情只返回聚焦项，清除后返回全量", () => {
    expect(detailComparisonIds(["a", "b", "c"], "b")).toEqual(["b"]);
    expect(detailComparisonIds(["a", "b", "c"], null)).toEqual(["a", "b", "c"]);
  });

  it("气泡吸附到光标相反半区", () => {
    expect(usageTooltipDock(80, 400)).toBe("bottom");
    expect(usageTooltipDock(320, 400)).toBe("top");
  });
});
