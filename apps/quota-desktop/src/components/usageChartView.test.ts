import { describe, expect, it } from "vitest";
import {
  addUsageMarker,
  advanceUsageViewDomain,
  buildLineGeometry,
  buildHistorySeries,
  historyPointValue,
  isolatedUsageSamples,
  moveUsageMarker,
  niceAbsoluteScale,
  shouldZoomUsageChart,
  snapUsageMarkerTimestamp,
  splitUsageSeries,
  USAGE_MARKER_LIMIT,
  USAGE_RANGES,
  USAGE_TOOLTIP_GAP,
  usageSmoothingRadius,
  usageTooltipPlacement,
  type UsageSample,
} from "./usageChartView";

const MINUTE = 60 * 1_000;
const HOUR = 60 * MINUTE;

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

  it("平台两端采用轻度局部平滑，但悬浮点仍保留真实值", () => {
    const geometry = buildLineGeometry(
      [point(0, 100), point(1, 80), point(2, 80), point(3, 80), point(4, 60)],
      (timestamp) => timestamp / HOUR * 100,
      (value) => value,
    );

    expect(geometry.points.map((item) => item.y)).toEqual([100, 80, 80, 80, 60]);
    expect(geometry.curvePoints[1].y).toBeGreaterThan(80);
    expect(geometry.curvePoints[2].y).toBeCloseTo(80);
    expect(geometry.curvePoints[3].y).toBeLessThan(80);
  });

  it("轻度平滑不跨越额度重置，重置段保持真实跳变", () => {
    const geometry = buildLineGeometry(
      [point(0, 80), point(1, 60), point(2, 100), point(3, 90)],
      (timestamp) => timestamp / HOUR * 100,
      (value) => 100 - value,
    );

    expect(geometry.path).toContain(" L 200 0");
    expect(geometry.curvePoints.map((item) => item.y)).toEqual([20, 40, 0, 10]);
  });

  it("仅 15 分钟及以下的细粒度桶启用轻度平滑，1 小时桶保留平台", () => {
    expect(usageSmoothingRadius(15 * MINUTE)).toBe(2);
    expect(usageSmoothingRadius(15 * MINUTE + 1)).toBe(0);
    expect(usageSmoothingRadius(HOUR)).toBe(0);

    const samples = [point(0, 100), point(1, 80), point(2, 80), point(3, 80), point(4, 60)];
    const geometry = buildLineGeometry(
      samples,
      (timestamp) => timestamp / HOUR * 100,
      (value) => value,
      { smoothingRadius: usageSmoothingRadius(HOUR) },
    );

    expect(geometry.curvePoints.map((item) => item.y)).toEqual([100, 80, 80, 80, 60]);
  });

  it("孤立单点从断线分段中单独提取用于圆点渲染", () => {
    const split = splitUsageSeries([point(0, 10), point(8, 20), point(9, 30)], HOUR);
    expect(isolatedUsageSamples(split)).toEqual([point(0, 10)]);
  });

  it("普通滚轮和非绘图区 Ctrl+滚轮留给页面，仅绘图区 Ctrl+滚轮缩放", () => {
    expect(shouldZoomUsageChart({ ctrlKey: false }, true)).toBe(false);
    expect(shouldZoomUsageChart({ ctrlKey: true }, false)).toBe(false);
    expect(shouldZoomUsageChart({ ctrlKey: true }, true)).toBe(true);
  });

  it("点位提示卡片锚定数据点：上半区翻到点下方，下半区翻到点上方，水平钳制在安全区", () => {
    const upper = usageTooltipPlacement({ x: 420, y: 82 }, 840, 410);
    expect(upper.below).toBe(true);
    expect(upper.topPct).toBeCloseTo(((82 + USAGE_TOOLTIP_GAP) / 410) * 100);
    expect(upper.leftPct).toBeCloseTo(50);

    const lower = usageTooltipPlacement({ x: 420, y: 306 }, 840, 410);
    expect(lower.below).toBe(false);
    expect(lower.topPct).toBeCloseTo(((306 - USAGE_TOOLTIP_GAP) / 410) * 100);

    expect(usageTooltipPlacement({ x: 40, y: 205 }, 840, 410).leftPct).toBe(10);
    expect(usageTooltipPlacement({ x: 900, y: 205 }, 840, 410).leftPct).toBe(82);
  });

  it("时间窗前进时跟随实时边缘，同时保留用户正在查看的历史区间", () => {
    const previousTotal: [number, number] = [0, 100];
    const nextTotal: [number, number] = [10, 110];

    expect(advanceUsageViewDomain([50, 100], previousTotal, nextTotal)).toEqual([60, 110]);
    expect(advanceUsageViewDomain([20, 60], previousTotal, nextTotal)).toEqual([20, 60]);
    expect(advanceUsageViewDomain([0, 30], previousTotal, nextTotal)).toEqual([10, 40]);
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
        samples: [point(0.5, 82), point(1, 76)],
      },
      {
        windowKey: "DeepSeek",
        metric: "absolute",
        unit: "CNY",
        samples: [point(0, 61.5)],
      },
    ]);
  });

  it("百分比历史倒置为剩余额度，绝对值继续展示 remaining", () => {
    expect(historyPointValue({
      window_key: "credits",
      sampled_at: 0,
      used: 25,
      total: 200,
      unit: "credits",
    })).toEqual({ metric: "percent", value: 87.5, unit: "%" });
    expect(historyPointValue({
      window_key: "percent",
      sampled_at: 0,
      used: 25,
      unit: "%",
    })).toEqual({ metric: "percent", value: 75, unit: "%" });
    expect(historyPointValue({
      window_key: "balance",
      sampled_at: 0,
      used: 4,
      remaining: 96,
      unit: "CNY",
    })).toEqual({ metric: "absolute", value: 96, unit: "CNY" });
  });

  it("视图范围档位锁定：24h 档 15 分钟桶、7d 档 1 小时桶，桶粒度整除跨度", () => {
    expect(Object.keys(USAGE_RANGES)).toEqual(["24h", "7d"]);
    expect(USAGE_RANGES["24h"]).toEqual({ spanMs: 24 * HOUR, bucketMs: 15 * MINUTE });
    expect(USAGE_RANGES["7d"]).toEqual({ spanMs: 7 * 24 * HOUR, bucketMs: HOUR });
    for (const config of Object.values(USAGE_RANGES)) {
      expect(config.spanMs % config.bucketMs).toBe(0);
    }
  });

  it("近 24 小时档按 15 分钟桶聚合：桶内保留最后一点，跨桶均保留", () => {
    const points = [
      { window_key: "Codex（5h）", sampled_at: 0, used: 10, remaining: 90, total: 100, unit: "%" },
      { window_key: "Codex（5h）", sampled_at: 10 * MINUTE, used: 16, remaining: 84, total: 100, unit: "%" },
      { window_key: "Codex（5h）", sampled_at: 15 * MINUTE, used: 24, remaining: 76, total: 100, unit: "%" },
    ];

    expect(buildHistorySeries(points, USAGE_RANGES["24h"].bucketMs)).toEqual([
      {
        windowKey: "Codex（5h）",
        metric: "percent",
        unit: "%",
        samples: [
          { timestamp: 10 * MINUTE, value: 84 },
          { timestamp: 15 * MINUTE, value: 76 },
        ],
      },
    ]);
  });

  it("15 分钟桶下空档阈值收紧：90 分钟仍虚线桥接，约 100 分钟起完全断开", () => {
    const series = splitUsageSeries(
      [
        { timestamp: 0, value: 10 },
        { timestamp: 60 * MINUTE, value: 14 },
        { timestamp: 150 * MINUTE, value: 20 },
        { timestamp: 160 * MINUTE, value: 24 },
        { timestamp: 260 * MINUTE, value: 30 },
      ],
      USAGE_RANGES["24h"].bucketMs,
    );

    expect(series.segments.map((segment) => segment.map((sample) => sample.timestamp)))
      .toEqual([[0], [60 * MINUTE], [150 * MINUTE, 160 * MINUTE], [260 * MINUTE]]);
    expect(series.bridges).toEqual([
      { from: { timestamp: 0, value: 10 }, to: { timestamp: 60 * MINUTE, value: 14 }, missingBuckets: 3 },
      { from: { timestamp: 60 * MINUTE, value: 14 }, to: { timestamp: 150 * MINUTE, value: 20 }, missingBuckets: 5 },
    ]);
    expect(series.gaps).toEqual([
      { from: { timestamp: 160 * MINUTE, value: 24 }, to: { timestamp: 260 * MINUTE, value: 30 }, missingBuckets: 6 },
    ]);
  });

  it("定位线放置：追加新时间戳，重复幂等，满两条后丢最旧", () => {
    expect(USAGE_MARKER_LIMIT).toBe(2);
    expect(addUsageMarker([], 100)).toEqual([100]);
    expect(addUsageMarker([100], 200)).toEqual([100, 200]);
    expect(addUsageMarker([100, 200], 200)).toEqual([100, 200]);
    expect(addUsageMarker([100, 200], 300)).toEqual([200, 300]);
  });

  it("定位线拖动微调：更新自身时刻，与另一条重合或未移动时原地不动", () => {
    expect(moveUsageMarker([100, 300], 300, 200)).toEqual([100, 200]);
    expect(moveUsageMarker([100, 300], 300, 100)).toEqual([100, 300]);
    expect(moveUsageMarker([100, 300], 300, 300)).toEqual([100, 300]);
  });

  it("定位线吸附：容差内吸附最近样本，容差外与空样本保留原始时刻", () => {
    const samples = [point(0, 10), point(2, 20)];
    expect(snapUsageMarkerTimestamp(0.6 * HOUR, samples, HOUR)).toBe(0);
    expect(snapUsageMarkerTimestamp(1.2 * HOUR, samples, HOUR)).toBe(2 * HOUR);
    // 等距时吸附先遍历到的样本（序列按时间升序输入，即较早的样本）
    expect(snapUsageMarkerTimestamp(HOUR, samples, HOUR)).toBe(0);
    expect(snapUsageMarkerTimestamp(4 * HOUR, samples, HOUR)).toBe(4 * HOUR);
    expect(snapUsageMarkerTimestamp(3 * HOUR, [], HOUR)).toBe(3 * HOUR);
  });
});
