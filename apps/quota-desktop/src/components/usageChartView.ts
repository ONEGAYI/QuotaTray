import type { HistoryPoint } from "../types";

export interface UsageSample {
  timestamp: number;
  value: number;
}

export interface UsageBridge {
  from: UsageSample;
  to: UsageSample;
  missingBuckets: number;
}

export interface SplitUsageSeries {
  segments: UsageSample[][];
  bridges: UsageBridge[];
  gaps: UsageBridge[];
}

export interface AxisScale {
  min: number;
  max: number;
  ticks: number[];
}

export interface ChartPoint {
  x: number;
  y: number;
  sample: UsageSample;
}

export type UsageMetricType = "absolute" | "percent";

export interface HistorySeries {
  windowKey: string;
  metric: UsageMetricType;
  unit: string;
  samples: UsageSample[];
}

export type UsageDomain = [number, number];

/** 点位提示卡片与数据点的视觉间隙（SVG 纵向单位）。 */
export const USAGE_TOOLTIP_GAP = 14;

/**
 * 点位提示卡片跟随数据点锚定：点在绘图区上半部时卡片放到点下方，
 * 下半部时翻到点上方——调用侧用 translateY(±100%) 切换锚边，
 * 因此无需估算卡片实际高度；水平沿用页面钳制区间避免左右溢出。
 * 返回值为相对图表容器的百分比坐标。
 */
export function usageTooltipPlacement(
  anchor: { x: number; y: number },
  chartWidth: number,
  chartHeight: number,
): { leftPct: number; topPct: number; below: boolean } {
  const below = anchor.y < chartHeight / 2;
  const topPct = below
    ? ((anchor.y + USAGE_TOOLTIP_GAP) / chartHeight) * 100
    : ((anchor.y - USAGE_TOOLTIP_GAP) / chartHeight) * 100;
  return {
    leftPct: Math.min(82, Math.max(10, (anchor.x / chartWidth) * 100)),
    topPct,
    below,
  };
}

function clampUsageDomain(view: UsageDomain, total: UsageDomain): UsageDomain {
  const span = Math.min(view[1] - view[0], total[1] - total[0]);
  if (view[0] < total[0]) return [total[0], total[0] + span];
  if (view[1] > total[1]) return [total[1] - span, total[1]];
  return view;
}

/**
 * 最近时间窗推进时：正在看实时边缘则保持缩放跨度并跟随；用户已平移到
 * 历史区间则保留绝对位置，只在其滑出保留期时做最小钳制。
 */
export function advanceUsageViewDomain(
  view: UsageDomain,
  previousTotal: UsageDomain,
  nextTotal: UsageDomain,
): UsageDomain {
  const atLiveEdge = Math.abs(view[1] - previousTotal[1]) <= 1;
  if (atLiveEdge) {
    const span = view[1] - view[0];
    return clampUsageDomain([nextTotal[1] - span, nextTotal[1]], nextTotal);
  }
  return clampUsageDomain(view, nextTotal);
}

export function historyPointValue(
  point: HistoryPoint,
): { metric: UsageMetricType; value: number; unit: string } | null {
  if (point.unit === "%") {
    if (point.remaining != null && Number.isFinite(point.remaining)) {
      return { metric: "percent", value: point.remaining, unit: "%" };
    }
    if (point.used != null && Number.isFinite(point.used)) {
      return { metric: "percent", value: 100 - point.used, unit: "%" };
    }
  }
  if (
    point.total != null
    && Number.isFinite(point.total)
    && point.total > 0
  ) {
    if (point.remaining != null && Number.isFinite(point.remaining)) {
      return { metric: "percent", value: (point.remaining / point.total) * 100, unit: "%" };
    }
    if (point.used != null && Number.isFinite(point.used)) {
      return { metric: "percent", value: 100 - (point.used / point.total) * 100, unit: "%" };
    }
  }
  if (point.remaining != null && Number.isFinite(point.remaining)) {
    return { metric: "absolute", value: point.remaining, unit: point.unit ?? "" };
  }
  if (point.used != null && Number.isFinite(point.used)) {
    return { metric: "absolute", value: point.used, unit: point.unit ?? "" };
  }
  return null;
}

/** 按窗口键分组、按时间桶保留最后一点，并丢弃无法绘制或语义漂移的旧点。 */
export function buildHistorySeries(
  points: HistoryPoint[],
  bucketMs: number,
): HistorySeries[] {
  const byWindow = new Map<string, HistoryPoint[]>();
  for (const point of points) {
    const group = byWindow.get(point.window_key) ?? [];
    group.push(point);
    byWindow.set(point.window_key, group);
  }

  const series: HistorySeries[] = [];
  for (const [windowKey, group] of byWindow) {
    const buckets = new Map<number, HistoryPoint>();
    for (const point of [...group].sort((a, b) => a.sampled_at - b.sampled_at)) {
      buckets.set(Math.floor(point.sampled_at / bucketMs), point);
    }
    const usable = [...buckets.values()]
      .map((point) => ({ point, value: historyPointValue(point) }))
      .filter((item): item is { point: HistoryPoint; value: NonNullable<ReturnType<typeof historyPointValue>> } => item.value != null);
    const latest = usable[usable.length - 1];
    if (!latest) continue;
    const matching = usable.filter((item) => item.value.metric === latest.value.metric);
    series.push({
      windowKey,
      metric: latest.value.metric,
      unit: latest.value.unit,
      samples: matching.map(({ point, value }) => ({
        timestamp: point.sampled_at,
        value: value.value,
      })),
    });
  }
  return series;
}

/** 普通滚轮与轴标签区归外层页面；仅绘图区 Ctrl+滚轮（含捏合）缩放。 */
export function shouldZoomUsageChart(
  event: { ctrlKey: boolean },
  insidePlot: boolean,
): boolean {
  return event.ctrlKey && insidePlot;
}

/**
 * 相邻点按展示桶判断连续性：最多缺 1 桶仍可拟合；缺 2–5 桶只画
 * 虚线端点桥；缺 6 桶以上完全断开。桥和断点都不会生成伪造样本。
 */
export function splitUsageSeries(
  samples: UsageSample[],
  bucketMs: number,
): SplitUsageSeries {
  if (samples.length === 0) return { segments: [], bridges: [], gaps: [] };

  const sorted = [...samples].sort((a, b) => a.timestamp - b.timestamp);
  const segments: UsageSample[][] = [[sorted[0]]];
  const bridges: UsageBridge[] = [];
  const gaps: UsageBridge[] = [];

  for (let index = 1; index < sorted.length; index += 1) {
    const from = sorted[index - 1];
    const to = sorted[index];
    const bucketDistance = Math.max(1, Math.round((to.timestamp - from.timestamp) / bucketMs));
    const missingBuckets = bucketDistance - 1;

    if (bucketDistance <= 2) {
      segments[segments.length - 1].push(to);
      continue;
    }

    const boundary = { from, to, missingBuckets };
    if (bucketDistance <= 6) bridges.push(boundary);
    else gaps.push(boundary);
    segments.push([to]);
  }

  return { segments, bridges, gaps };
}

const NICE_FACTORS = [1, 2, 2.5, 5, 10];

function niceStep(rawStep: number): number {
  if (!Number.isFinite(rawStep) || rawStep <= 0) return 25;
  const magnitude = 10 ** Math.floor(Math.log10(rawStep));
  const normalized = rawStep / magnitude;
  return (NICE_FACTORS.find((factor) => factor >= normalized) ?? 10) * magnitude;
}

/** 绝对值轴固定从零开始，按当前数据最大值生成易读的等距刻度。 */
export function niceAbsoluteScale(values: number[], intervalCount = 4): AxisScale {
  const finite = values.filter((value) => Number.isFinite(value) && value >= 0);
  const dataMax = finite.length > 0 ? Math.max(...finite) : 100;
  const step = niceStep(dataMax / intervalCount);
  const max = step * intervalCount;
  return {
    min: 0,
    max,
    ticks: Array.from({ length: intervalCount + 1 }, (_, index) => step * index),
  };
}

function formatCoordinate(value: number): string {
  const rounded = Math.round(value * 1_000) / 1_000;
  return Object.is(rounded, -0) ? "0" : String(rounded);
}

/**
 * 以单调三次 Hermite 曲线连接真实点。切线经过限幅，避免在相邻点范围外
 * 产生视觉过冲；缺失区间应在调用前由 splitUsageSeries 分段。
 */
export function buildLineGeometry(
  samples: UsageSample[],
  xOf: (timestamp: number) => number,
  yOf: (value: number) => number,
): { path: string; points: ChartPoint[] } {
  const points = samples.map((sample) => ({
    x: xOf(sample.timestamp),
    y: yOf(sample.value),
    sample,
  }));
  if (points.length < 2) return { path: "", points };

  const slopes = points.slice(1).map((point, index) => {
    const previous = points[index];
    const dx = point.x - previous.x;
    return dx === 0 ? 0 : (point.y - previous.y) / dx;
  });
  const tangents = points.map((_, index) => {
    if (index === 0) return slopes[0];
    if (index === points.length - 1) return slopes[slopes.length - 1];
    const before = slopes[index - 1];
    const after = slopes[index];
    if (before === 0 || after === 0 || Math.sign(before) !== Math.sign(after)) return 0;
    return (2 * before * after) / (before + after);
  });

  let path = `M ${formatCoordinate(points[0].x)} ${formatCoordinate(points[0].y)}`;
  for (let index = 0; index < points.length - 1; index += 1) {
    const from = points[index];
    const to = points[index + 1];
    const dx = to.x - from.x;
    const c1x = from.x + dx / 3;
    const c1y = from.y + tangents[index] * dx / 3;
    const c2x = to.x - dx / 3;
    const c2y = to.y - tangents[index + 1] * dx / 3;
    path += ` C ${formatCoordinate(c1x)} ${formatCoordinate(c1y)} ${formatCoordinate(c2x)} ${formatCoordinate(c2y)} ${formatCoordinate(to.x)} ${formatCoordinate(to.y)}`;
  }
  return { path, points };
}
