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

export type UsageRange = "24h" | "7d";

/** 定位线上限：超出时丢弃最早写入的条目（数组头部），保持「测量两点间隔」
 *  语义；与 Rust 侧 settings.rs 的 MAX_USAGE_MARKER_LINES 同值，两端同步修改。 */
export const USAGE_MARKER_LIMIT = 2;

export interface UsageRangeConfig {
  spanMs: number;
  bucketMs: number;
}

/**
 * 使用统计视图范围档位：桶粒度对齐 CLI history 的范围口径
 * （docs/specs/history-spec.md §7：24h=15 分钟桶 / 7d=1 小时桶），
 * 桶内取最后一点。历史拉取始终取各档最大 span，前端再按所选范围裁剪。
 */
export const USAGE_RANGES: Record<UsageRange, UsageRangeConfig> = {
  "24h": { spanMs: 24 * 60 * 60 * 1_000, bucketMs: 15 * 60 * 1_000 },
  "7d": { spanMs: 7 * 24 * 60 * 60 * 1_000, bucketMs: 60 * 60 * 1_000 },
};

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

/** 单点分段无法形成 SVG path，需以真实端点圆单独渲染。 */
export function isolatedUsageSamples(split: SplitUsageSeries): UsageSample[] {
  return split.segments.flatMap((segment) => segment.length === 1 ? segment : []);
}

/**
 * 放置定位线：与既有时间戳重复时幂等返回原数组（时间差为 0 无意义）；
 * 已满上限时丢弃最早写入的条目——两次测量意图以最新一次为准（两条拖动
 * 交叉后数组序不保证时间序，超限丢弃的是数组头部）。
 */
export function addUsageMarker(existing: number[], timestamp: number): number[] {
  if (existing.includes(timestamp)) return existing;
  const next = [...existing, timestamp];
  return next.length > USAGE_MARKER_LIMIT ? next.slice(next.length - USAGE_MARKER_LIMIT) : next;
}

/** 拖动微调定位线：目标时刻与另一条重合或未移动时原地不动（幂等返回原数组）。 */
export function moveUsageMarker(existing: number[], from: number, to: number): number[] {
  if (from === to || existing.includes(to)) return existing;
  return existing.map((timestamp) => (timestamp === from ? to : timestamp));
}

/**
 * 吸附最近真实样本时刻：距离 ≤ tolerance 才吸附（等距时取先遍历到的样本），
 * 无样本或超出容差时保留原始时刻——定位线对齐真实采样点，读数才干净。
 */
export function snapUsageMarkerTimestamp(
  timestamp: number,
  samples: UsageSample[],
  toleranceMs: number,
): number {
  let best: UsageSample | null = null;
  for (const sample of samples) {
    if (!best || Math.abs(sample.timestamp - timestamp) < Math.abs(best.timestamp - timestamp)) {
      best = sample;
    }
  }
  return best && Math.abs(best.timestamp - timestamp) <= toleranceMs ? best.timestamp : timestamp;
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

const LIGHT_SMOOTHING_RADIUS = 2;
const MAX_SMOOTHING_BUCKET_MS = USAGE_RANGES["24h"].bucketMs;
const MIN_RESET_JUMP_PX = 12;
const RESET_RANGE_FRACTION = 0.25;

/** 轻度平滑只用于 15 分钟及以下的细粒度展示桶。 */
export function usageSmoothingRadius(bucketMs: number): number {
  return Number.isFinite(bucketMs) && bucketMs > 0 && bucketMs <= MAX_SMOOTHING_BUCKET_MS
    ? LIGHT_SMOOTHING_RADIUS
    : 0;
}

function splitCurvePointsAtResets(points: ChartPoint[]): ChartPoint[][] {
  const ys = points.map((point) => point.y);
  const visualRange = Math.max(...ys) - Math.min(...ys);
  const resetThreshold = Math.max(MIN_RESET_JUMP_PX, visualRange * RESET_RANGE_FRACTION);
  const segments: ChartPoint[][] = [[points[0]]];

  for (let index = 1; index < points.length; index += 1) {
    const previous = points[index - 1];
    const current = points[index];
    const valueIncreased = current.sample.value > previous.sample.value;
    const visualJump = Math.abs(current.y - previous.y);
    if (valueIncreased && visualJump >= resetThreshold) {
      segments.push([current]);
    } else {
      segments[segments.length - 1].push(current);
    }
  }
  return segments;
}

function lightSmoothCurveSegment(points: ChartPoint[], radius: number): ChartPoint[] {
  if (points.length <= 2) return points;
  const sigma = radius / 1.6;
  return points.map((point, index) => {
    if (index === 0 || index === points.length - 1) return point;
    const from = Math.max(0, index - radius);
    const to = Math.min(points.length - 1, index + radius);
    let weightedY = 0;
    let totalWeight = 0;
    for (let cursor = from; cursor <= to; cursor += 1) {
      const distance = cursor - index;
      const weight = Math.exp(-(distance * distance) / (2 * sigma * sigma));
      weightedY += points[cursor].y * weight;
      totalWeight += weight;
    }
    return { ...point, y: weightedY / totalWeight };
  });
}

function appendMonotoneCurve(path: string, points: ChartPoint[]): string {
  if (points.length < 2) return path;
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
  return path;
}

/**
 * 启用时对真实点做轻度局部平滑，再以单调三次 Hermite 曲线连接；调用方按
 * 展示桶粒度传入半径 2 或 0。悬浮点仍保留原始坐标；检测到显著正向跳增时
 * 按额度重置分段，以直线保留真实跳变。缺失区间应在调用前由
 * splitUsageSeries 分段。
 */
export function buildLineGeometry(
  samples: UsageSample[],
  xOf: (timestamp: number) => number,
  yOf: (value: number) => number,
  options: { smoothingRadius?: number } = {},
): { path: string; points: ChartPoint[]; curvePoints: ChartPoint[] } {
  const points = samples.map((sample) => ({
    x: xOf(sample.timestamp),
    y: yOf(sample.value),
    sample,
  }));
  if (points.length < 2) return { path: "", points, curvePoints: points };

  const smoothingRadius = Math.max(0, Math.floor(options.smoothingRadius ?? LIGHT_SMOOTHING_RADIUS));
  const smoothedSegments = smoothingRadius > 0
    ? splitCurvePointsAtResets(points).map((segment) => lightSmoothCurveSegment(segment, smoothingRadius))
    : [points];
  const curvePoints = smoothedSegments.flat();
  let path = "";
  for (const [index, segment] of smoothedSegments.entries()) {
    const first = segment[0];
    path += index === 0
      ? `M ${formatCoordinate(first.x)} ${formatCoordinate(first.y)}`
      : ` L ${formatCoordinate(first.x)} ${formatCoordinate(first.y)}`;
    path = appendMonotoneCurve(path, segment);
  }
  return { path, points, curvePoints };
}
