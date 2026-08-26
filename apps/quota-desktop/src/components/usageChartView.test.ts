import { describe, expect, it } from "vitest";
import {
  buildLineGeometry,
  buildHistorySeries,
  historyPointValue,
  niceAbsoluteScale,
  shouldZoomUsageChart,
  splitUsageSeries,
  type UsageSample,
} from "./usageChartView";

const HOUR = 60 * 60 * 1_000;

function point(hour: number, value: number): UsageSample {
  return { timestamp: hour * HOUR, value };
}

describe("使用统计图表纯逻辑", () => {
  it("连续数据拟合为同一段，短缺失用虚线桥接，长缺失完全断开", () => {
    const series = splitUsageSeries(
      [point(0, 10), point(1, 14), point(4, 22), point(12, 38), point(13, 42)],
      HOUR,
    );

    expect(series.segments.map((segment) => segment.map((sample) => sample.timestamp / HOUR)))
      .toEqual([[0, 1], [4], [12, 13]]);
    expect(series.bridges).toEqual([
      { from: point(1, 14), to: point(4, 22), missingBuckets: 2 },
    ]);
    expect(series.gaps).toEqual([
      { from: point(4, 22), to: point(12, 38), missingBuckets: 7 },
    ]);
  });

  it("绝对值轴从零开始并生成覆盖最大值的整洁刻度", () => {
    expect(niceAbsoluteScale([12, 88, 131], 4)).toEqual({
      min: 0,
      max: 200,
      ticks: [0, 50, 100, 150, 200],
    });
    expect(niceAbsoluteScale([], 4)).toEqual({
      min: 0,
      max: 100,
      ticks: [0, 25, 50, 75, 100],
    });
  });

  it("连续段输出单调三次曲线路径，单点段保留为可悬停数据点", () => {
    const geometry = buildLineGeometry(
      [point(0, 10), point(1, 20), point(2, 15)],
      (timestamp) => timestamp / HOUR * 100,
      (value) => 100 - value,
    );

    expect(geometry.path).toMatch(/^M 0 90 C /);
    expect(geometry.path).not.toContain("NaN");
    expect(geometry.points).toEqual([
      { x: 0, y: 90, sample: point(0, 10) },
      { x: 100, y: 80, sample: point(1, 20) },
      { x: 200, y: 85, sample: point(2, 15) },
    ]);

    expect(buildLineGeometry([point(3, 7)], () => 18, () => 24).path).toBe("");
  });

  it("普通滚轮留给页面滚动，仅 Ctrl+滚轮进入图表缩放", () => {
    expect(shouldZoomUsageChart({ ctrlKey: false })).toBe(false);
    expect(shouldZoomUsageChart({ ctrlKey: true })).toBe(true);
  });

  it("真实历史点按 Scope 与小时桶分组，桶内保留最后一点", () => {
    const points = [
      { window_key: "Codex（5h）", sampled_at: 0, used: 10, remaining: 90, total: 100, unit: "%" },
      { window_key: "Codex（5h）", sampled_at: HOUR / 2, used: 18, remaining: 82, total: 100, unit: "%" },
      { window_key: "Codex（5h）", sampled_at: HOUR, used: 24, remaining: 76, total: 100, unit: "%" },
      { window_key: "DeepSeek", sampled_at: 0, remaining: 61.5, unit: "CNY" },
      { window_key: "empty", sampled_at: 0 },
    ];

    expect(buildHistorySeries(points, HOUR)).toEqual([
      {
        windowKey: "Codex（5h）",
        metric: "percent",
        unit: "%",
        samples: [point(0.5, 18), point(1, 24)],
      },
      {
        windowKey: "DeepSeek",
        metric: "absolute",
        unit: "CNY",
        samples: [point(0, 61.5)],
      },
    ]);
  });

  it("历史值语义与现有卡片一致：百分比优先，否则展示剩余额度", () => {
    expect(historyPointValue({
      window_key: "credits",
      sampled_at: 0,
      used: 25,
      total: 200,
      unit: "credits",
    })).toEqual({ metric: "percent", value: 12.5, unit: "%" });
    expect(historyPointValue({
      window_key: "balance",
      sampled_at: 0,
      used: 4,
      remaining: 96,
      unit: "CNY",
    })).toEqual({ metric: "absolute", value: 96, unit: "CNY" });
  });
});
