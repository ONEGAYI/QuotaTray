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

/** 曲线值方向：样本 value 是剩余量还是已用量。百分比曲线恒为剩余量
 *  （used 倒置为 100−used）；绝对值曲线优先 remaining，仅配 used 的
 *  模板（used 独立可选、无 remaining/total）值为已用量——消耗方向相反，
 *  速率/峰值/净消耗均须按此方向补偿（issue #135）。 */
export type UsageQuantity = "remaining" | "used";

export interface HistorySeries {
  windowKey: string;
  metric: UsageMetricType;
  quantity: UsageQuantity;
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
): { metric: UsageMetricType; quantity: UsageQuantity; value: number; unit: string } | null {
  if (point.unit === "%") {
    if (point.remaining != null && Number.isFinite(point.remaining)) {
      return { metric: "percent", quantity: "remaining", value: point.remaining, unit: "%" };
    }
    if (point.used != null && Number.isFinite(point.used)) {
      return { metric: "percent", quantity: "remaining", value: 100 - point.used, unit: "%" };
    }
  }
  if (
    point.total != null
    && Number.isFinite(point.total)
    && point.total > 0
  ) {
    if (point.remaining != null && Number.isFinite(point.remaining)) {
      return { metric: "percent", quantity: "remaining", value: (point.remaining / point.total) * 100, unit: "%" };
    }
    if (point.used != null && Number.isFinite(point.used)) {
      return { metric: "percent", quantity: "remaining", value: 100 - (point.used / point.total) * 100, unit: "%" };
    }
  }
  if (point.remaining != null && Number.isFinite(point.remaining)) {
    return { metric: "absolute", quantity: "remaining", value: point.remaining, unit: point.unit ?? "" };
  }
  if (point.used != null && Number.isFinite(point.used)) {
    return { metric: "absolute", quantity: "used", value: point.used, unit: point.unit ?? "" };
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
    // 方向（quantity）与 metric 同按最新样本过滤：模板从仅配 used 改为
    // 提供 remaining 时旧样本是已用量语义，混排会使速率/净消耗方向错乱
    const matching = usable.filter((item) => item.value.metric === latest.value.metric && item.value.quantity === latest.value.quantity);
    series.push({
      windowKey,
      metric: latest.value.metric,
      quantity: latest.value.quantity,
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

/** 「定位线」卡头按钮一次点击的语义裁决。 */
export interface UsageMarkerToggleOutcome {
  /** 点击后放置模式是否开启 */
  mode: boolean;
  /** 点击后的定位线列表：满两条重定位时清空，其余路径原样保留 */
  markers: number[];
  /** 本次点击是否清掉了已有定位线（调用方据此决定是否落盘） */
  cleared: boolean;
}

/**
 * 定位线按钮三态（2026-09-16 所有者修订）：放置模式中点击=退出；未满两条时
 * 点击进入放置直接补位（下一次图表点击落 vacant 位）；满两条再点=两条全清、
 * 从头定位。清空不进入放置的诉求由卡头垃圾桶承担。
 */
export function pressUsageMarkerToggle(mode: boolean, markers: readonly number[]): UsageMarkerToggleOutcome {
  if (mode) return { mode: false, markers: [...markers], cleared: false };
  if (markers.length >= USAGE_MARKER_LIMIT) return { mode: true, markers: [], cleared: true };
  return { mode: true, markers: [...markers], cleared: false };
}

/**
 * 吸附最近真实样本时刻：距离 ≤ tolerance 才吸附（等距时取先遍历到的样本），
 * 无样本或超出容差时保留原始时刻——定位线对齐真实采样点，读数才干净。
 * 未吸附路径归整为整数毫秒：输入来自图表坐标换算（带小数），而
 * usage_marker_lines 持久化为 u64，浮点会使后端反序列化失败、保存回退，
 * 表现为空采集区域（不吸附）放不上线（2026-09-07 真机实证）。
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
  return best && Math.abs(best.timestamp - timestamp) <= toleranceMs ? best.timestamp : Math.round(timestamp);
}

/** 距给定时刻最近且不超过一个展示桶宽的真实样本（读数行与悬浮读数取值口径）。 */
export function nearestUsageSample(
  samples: UsageSample[],
  timestamp: number,
  bucketMs: number,
): UsageSample | null {
  const nearest = samples.reduce<UsageSample | null>(
    (best, sample) => !best || Math.abs(sample.timestamp - timestamp) < Math.abs(best.timestamp - timestamp) ? sample : best,
    null,
  );
  return nearest && Math.abs(nearest.timestamp - timestamp) <= bucketMs ? nearest : null;
}

/** 定位线测量的取值口径：聚焦曲线的展示桶样本、桶宽容差与曲线值方向。 */
export interface UsageMarkerScope {
  samples: UsageSample[];
  bucketMs: number;
  quantity: UsageQuantity;
}

/**
 * 定位线测量共用守卫：两条线按时间升序、时间差至少 1 分钟（毫秒级差异
 * 无测量意义，与 markerSpanText「至少 1 分钟」口径对齐），两端各取容差内
 * 最近样本，缺失即无可测值。nearest 语义下早线样本时刻恒不晚于晚线
 * 样本（两者都取自同一序列时不可能交叉），区间遍历按端点时刻夹取。
 */
function markerEndpoints(
  scope: UsageMarkerScope,
  markers: number[],
): { from: UsageSample; to: UsageSample; spanMs: number } | null {
  if (markers.length < 2) return null;
  const [early, late] = [...markers].sort((a, b) => a - b);
  const spanMs = late - early;
  if (spanMs < 60_000) return null;
  const from = nearestUsageSample(scope.samples, early, scope.bucketMs);
  const to = nearestUsageSample(scope.samples, late, scope.bucketMs);
  if (!from || !to) return null;
  return { from, to, spanMs };
}

/** 消耗方向的变化量（正 = 消耗）：剩余量下降为消耗，已用量上升为消耗——
 *  仅配 used 的模板曲线值是已用量，方向与剩余量相反（issue #135 同票修正）。 */
function consumptionDelta(
  quantity: UsageQuantity,
  previous: UsageSample,
  current: UsageSample,
): number {
  return quantity === "used" ? current.value - previous.value : previous.value - current.value;
}

/**
 * 定位线平均消耗速率（每小时）：两条线各按读数同口径取容差内最近样本，
 * 端点消耗方向变化量换算为正消耗（剩余量下降 / 已用量上升；负值表示
 * 区间内回升）。时间差按 marker 时刻计算，与读数行展示的时间差同源。
 * 样本缺失、时间差为零或不足 1 分钟返回 null（无可测值）。
 */
export function usageMarkerBurnRate(
  scope: UsageMarkerScope,
  markers: number[],
): number | null {
  const endpoints = markerEndpoints(scope, markers);
  if (!endpoints) return null;
  return consumptionDelta(scope.quantity, endpoints.from, endpoints.to) / (endpoints.spanMs / 3_600_000);
}

/** 定位线区间内最陡消耗段的每小时速率与段端样本。 */
export interface UsagePeakBurn {
  ratePerHour: number;
  from: UsageSample;
  to: UsageSample;
}

/**
 * 定位线区间内最陡消耗段（每小时）：两端按读数同口径取容差内最近
 * 样本，区间内相邻样本对按消耗方向变化量 ÷ 真实时长换算每小时斜率
 * （与 usageMarkerBurnRate 同口径：正 = 消耗，已用量曲线按已用量方向），
 * 取最大消耗段，斜率相同取较早段。只计算消耗方向——回升（充值/额度
 * 重置）是瞬间跳变而非连续过程，跨桶斜率无测量意义（2026-09-16
 * 所有者裁定不展示）。区间内不足两个样本、无消耗段、端点样本缺失、
 * markers 不足或时间差不足 1 分钟（与平均速率守卫对齐）时返回
 * null（无可测值）。
 */
export function usageMarkerPeakBurn(
  scope: UsageMarkerScope,
  markers: number[],
): UsagePeakBurn | null {
  const endpoints = markerEndpoints(scope, markers);
  if (!endpoints) return null;
  const rangeStart = Math.min(endpoints.from.timestamp, endpoints.to.timestamp);
  const rangeEnd = Math.max(endpoints.from.timestamp, endpoints.to.timestamp);
  const sorted = scope.samples
    .filter((sample) => sample.timestamp >= rangeStart && sample.timestamp <= rangeEnd)
    .sort((a, b) => a.timestamp - b.timestamp);
  if (sorted.length < 2) return null;

  let peak: UsagePeakBurn | null = null;
  for (let index = 1; index < sorted.length; index += 1) {
    const previous = sorted[index - 1];
    const current = sorted[index];
    const spanMs = current.timestamp - previous.timestamp;
    if (spanMs <= 0) continue;
    const ratePerHour = consumptionDelta(scope.quantity, previous, current) / (spanMs / 3_600_000);
    if (ratePerHour > 0 && (!peak || ratePerHour > peak.ratePerHour)) {
      peak = { ratePerHour, from: previous, to: current };
    }
  }
  return peak;
}

/**
 * 定位线区间净消耗：两条定位线所对应原始样本的端点净变化（消耗方向为
 * 正，负数表示区间净恢复——用「净消耗」而非「总消耗」，避免把净值误认
 * 作累计毛消耗，issue #135）。守卫与 usageMarkerBurnRate 一致；视觉
 * 平滑曲线不参与计算（本函数与分解都只消费原始展示桶样本）。
 */
export function usageMarkerNet(
  scope: UsageMarkerScope,
  markers: number[],
): number | null {
  const endpoints = markerEndpoints(scope, markers);
  if (!endpoints) return null;
  return consumptionDelta(scope.quantity, endpoints.from, endpoints.to);
}

/** 定位线区间净消耗分解：已观测消耗/恢复与断档上的带符号净变化。 */
export interface UsageNetBreakdown {
  /** 连续可观察段（相邻样本桶距 ≤2，与 splitUsageSeries 的 segments 边界
   *  一致——视觉实线段即已观测）内的累计消耗，非负 */
  observedBurn: number;
  /** 连续可观察段内的累计恢复（含额度重置跳升），非负 */
  observedRecovery: number;
  /** 虚线桥与断档（桶距 >2）上的带符号净变化（消耗方向为正）：
   *  断档期间发生的消耗与恢复组合无法推断，只如实记净值 */
  unobservedNet: number;
  /** 端点净消耗（与 usageMarkerNet 同源），冗余携带便于展示侧核对代数 */
  net: number;
}

/**
 * 定位线区间净消耗分解（issue #135）：连续可观察的相邻样本变化按曲线
 * 真实语义拆「已观测累计消耗」与「已观测累计恢复」；长断档（虚线桥及
 * 更大空档）不推断其中发生的消耗和恢复，差额记为带符号「未观测净变化」，
 * 保证 已观测消耗 − 已观测恢复 + 未观测净变化 = 端点净消耗 恒成立。
 * 只提供已用量的模板按已用量上升=消耗、下降=恢复（consumptionDelta）。
 * 守卫与 usageMarkerNet 一致。
 */
export function usageMarkerNetBreakdown(
  scope: UsageMarkerScope,
  markers: number[],
): UsageNetBreakdown | null {
  const endpoints = markerEndpoints(scope, markers);
  if (!endpoints) return null;
  const rangeStart = Math.min(endpoints.from.timestamp, endpoints.to.timestamp);
  const rangeEnd = Math.max(endpoints.from.timestamp, endpoints.to.timestamp);
  const sorted = scope.samples
    .filter((sample) => sample.timestamp >= rangeStart && sample.timestamp <= rangeEnd)
    .sort((a, b) => a.timestamp - b.timestamp);

  let observedBurn = 0;
  let observedRecovery = 0;
  let unobservedNet = 0;
  for (let index = 1; index < sorted.length; index += 1) {
    const previous = sorted[index - 1];
    const current = sorted[index];
    const bucketDistance = Math.max(1, Math.round((current.timestamp - previous.timestamp) / scope.bucketMs));
    const delta = consumptionDelta(scope.quantity, previous, current);
    if (bucketDistance <= 2) {
      if (delta > 0) observedBurn += delta;
      else if (delta < 0) observedRecovery += -delta;
    } else {
      unobservedNet += delta;
    }
  }
  return {
    observedBurn,
    observedRecovery,
    unobservedNet,
    net: consumptionDelta(scope.quantity, endpoints.from, endpoints.to),
  };
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
