import { Maximize2, MousePointer2, RotateCcw } from "lucide-react";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type PointerEvent as ReactPointerEvent,
  type WheelEvent,
} from "react";
import { useLang } from "../i18n";
import { useHistory } from "../queries";
import type { ProviderEntry } from "../types";
import { Button, SegmentedControl } from "./ui";
import {
  advanceUsageViewDomain,
  buildHistorySeries,
  buildLineGeometry,
  niceAbsoluteScale,
  shouldZoomUsageChart,
  splitUsageSeries,
  usageTooltipPlacement,
  USAGE_RANGES,
  type HistorySeries,
  type UsageBridge,
  type UsageMetricType,
  type UsageRange,
  type UsageSample,
} from "./usageChartView";

interface UsageScope extends HistorySeries {
  name: string;
  color: string;
  bucketMs: number;
}

type HoverState =
  | { kind: "sample"; sample: UsageSample; x: number; y: number }
  | { kind: "gap"; gap: UsageBridge; x: number; short: boolean }
  | null;

interface Props {
  providers: ProviderEntry[];
  providersLoading: boolean;
  providersError?: unknown;
  mobile?: boolean;
}

// 历史拉取始终取各范围档中的最大跨度，切档复用同一份缓存（queryKey 含 spanMs）
const HISTORY_FETCH_SPAN_MS = Math.max(
  ...Object.values(USAGE_RANGES).map((range) => range.spanMs),
);
// 首系列锚定品牌强调色令牌（明暗自适应），其余为固定辅助色板（见 DT-004）
const SERIES_COLORS = ["var(--qt-accent)", "#df6f9f", "#3d9b87", "#e49537", "#397bd8", "#a45fd4"];
const CHART = { width: 840, height: 410, left: 74, right: 766, top: 42, bottom: 338 };
const PLOT_WIDTH = CHART.right - CHART.left;
const PLOT_HEIGHT = CHART.bottom - CHART.top;

function clampDomain(
  min: number,
  max: number,
  totalMin: number,
  totalMax: number,
): [number, number] {
  const span = max - min;
  if (min < totalMin) return [totalMin, totalMin + span];
  if (max > totalMax) return [totalMax - span, totalMax];
  return [min, max];
}

function formatNumber(value: number, metric: UsageMetricType): string {
  if (metric === "percent") return `${Math.round(value)}%`;
  return value.toLocaleString(undefined, { maximumFractionDigits: 1 });
}

function formatAxisNumber(value: number): string {
  if (Math.abs(value) >= 1_000) return `${Math.round(value / 1_000)}k`;
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

function scopeName(windowKey: string, index: number, lang: "zh" | "en"): string {
  const bracketed = windowKey.match(/（([^）]+)）|\(([^)]+)\)/)?.slice(1).find(Boolean);
  if (bracketed) {
    if (bracketed.toLowerCase() === "week") return lang === "zh" ? "周限" : "Weekly";
    return bracketed;
  }
  if (/^w\d+(?:#\d+)?$/.test(windowKey)) {
    return lang === "zh" ? `窗口 ${index + 1}` : `Window ${index + 1}`;
  }
  return windowKey;
}

export function UsageStatsPage({ providers, providersLoading, providersError, mobile = false }: Props) {
  const { lang, t } = useLang();
  const [rangeNow, setRangeNow] = useState(() => Date.now());
  const [range, setRange] = useState<UsageRange>("7d");
  const rangeConfig = USAGE_RANGES[range];
  const totalDomain = useMemo<[number, number]>(
    () => [rangeNow - rangeConfig.spanMs, rangeNow],
    [rangeNow, rangeConfig],
  );
  const [providerId, setProviderId] = useState("");
  const [scopeId, setScopeId] = useState("");
  const [viewDomain, setViewDomain] = useState<[number, number]>(totalDomain);
  const [hover, setHover] = useState<HoverState>(null);
  const [chartHovered, setChartHovered] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const svgRef = useRef<SVGSVGElement>(null);
  const previousTotalRef = useRef(totalDomain);
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    domain: [number, number];
  } | null>(null);

  useEffect(() => {
    const timer = window.setInterval(() => setRangeNow(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const previousTotal = previousTotalRef.current;
    if (previousTotal[0] === totalDomain[0] && previousTotal[1] === totalDomain[1]) return;
    setViewDomain((view) => advanceUsageViewDomain(view, previousTotal, totalDomain));
    previousTotalRef.current = totalDomain;
  }, [totalDomain]);

  useEffect(() => {
    if (providers.length === 0) {
      if (providerId) setProviderId("");
      return;
    }
    if (!providers.some((provider) => provider.id === providerId)) {
      setProviderId(providers[0].id);
      setScopeId("");
      setViewDomain(totalDomain);
    }
  }, [providerId, providers, totalDomain]);

  const provider = providers.find((item) => item.id === providerId);
  const history = useHistory(providerId, HISTORY_FETCH_SPAN_MS);
  useEffect(() => {
    const newest = (history.data ?? []).reduce(
      (latest, point) => Math.max(latest, point.sampled_at),
      0,
    );
    if (newest > rangeNow) setRangeNow(newest);
  }, [history.data, rangeNow]);
  const scopes = useMemo<UsageScope[]>(
    () => buildHistorySeries(
      (history.data ?? []).filter((point) => (
        point.sampled_at >= totalDomain[0] && point.sampled_at <= totalDomain[1]
      )),
      rangeConfig.bucketMs,
    ).map((series, index) => ({
      ...series,
      name: scopeName(series.windowKey, index, lang),
      color: SERIES_COLORS[index % SERIES_COLORS.length],
      bucketMs: rangeConfig.bucketMs,
    })),
    [history.data, lang, rangeConfig, totalDomain],
  );

  useEffect(() => {
    if (scopes.length === 0) {
      if (scopeId) setScopeId("");
      return;
    }
    if (!scopes.some((scope) => scope.windowKey === scopeId)) {
      setScopeId(scopes[0].windowKey);
      setViewDomain(totalDomain);
      setHover(null);
    }
  }, [scopeId, scopes, totalDomain]);

  const scope = scopes.find((item) => item.windowKey === scopeId);
  const xOf = (timestamp: number) =>
    CHART.left + ((timestamp - viewDomain[0]) / (viewDomain[1] - viewDomain[0])) * PLOT_WIDTH;
  const visibleValues = (scope?.samples ?? [])
    .filter((sample) => sample.timestamp >= viewDomain[0] && sample.timestamp <= viewDomain[1])
    .map((sample) => sample.value);
  const absoluteScale = niceAbsoluteScale(visibleValues);
  const yOf = (value: number) => {
    const scaleMax = scope?.metric === "percent" ? 100 : absoluteScale.max;
    return CHART.bottom - (value / scaleMax) * PLOT_HEIGHT;
  };
  const split = useMemo(
    () => scope ? splitUsageSeries(scope.samples, scope.bucketMs) : { segments: [], bridges: [], gaps: [] },
    [scope],
  );
  const geometries = split.segments.map((segment) => buildLineGeometry(segment, xOf, yOf));
  const percentTicks = [0, 25, 50, 75, 100];
  const activeTicks = scope?.metric === "percent" ? percentTicks : absoluteScale.ticks;
  const xTicks = Array.from({ length: 7 }, (_, index) =>
    viewDomain[0] + ((viewDomain[1] - viewDomain[0]) * index) / 6,
  );
  const current = scope?.samples[scope.samples.length - 1];
  const dateFormatter = new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
  const axisDateFormatter = new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
  });

  const selectProvider = (nextProviderId: string) => {
    dragRef.current = null;
    setIsDragging(false);
    setProviderId(nextProviderId);
    setScopeId("");
    setViewDomain(totalDomain);
    setHover(null);
  };

  const selectScope = (nextScopeId: string) => {
    dragRef.current = null;
    setIsDragging(false);
    setScopeId(nextScopeId);
    setViewDomain(totalDomain);
    setHover(null);
  };

  const selectRange = (nextRange: UsageRange) => {
    if (nextRange === range) return;
    dragRef.current = null;
    setIsDragging(false);
    setRange(nextRange);
    setViewDomain([rangeNow - USAGE_RANGES[nextRange].spanMs, rangeNow]);
    setHover(null);
  };

  const svgPoint = (clientX: number, clientY: number) => {
    const bounds = svgRef.current?.getBoundingClientRect();
    if (!bounds) return { x: 0, y: 0 };
    return {
      x: ((clientX - bounds.left) / bounds.width) * CHART.width,
      y: ((clientY - bounds.top) / bounds.height) * CHART.height,
    };
  };

  const updateHover = (x: number) => {
    if (!scope || x < CHART.left || x > CHART.right) {
      setHover(null);
      return;
    }
    const visible = scope.samples
      .map((sample) => ({ sample, x: xOf(sample.timestamp) }))
      .filter((item) => item.x >= CHART.left - 18 && item.x <= CHART.right + 18);
    const nearest = visible.reduce<(typeof visible)[number] | null>((best, item) => {
      if (!best || Math.abs(item.x - x) < Math.abs(best.x - x)) return item;
      return best;
    }, null);
    if (nearest && Math.abs(nearest.x - x) <= 18) {
      setHover({
        kind: "sample",
        sample: nearest.sample,
        x: nearest.x,
        y: yOf(nearest.sample.value),
      });
      return;
    }
    const timestamp = viewDomain[0] + ((x - CHART.left) / PLOT_WIDTH) * (viewDomain[1] - viewDomain[0]);
    const longGap = split.gaps.find((gap) => timestamp > gap.from.timestamp && timestamp < gap.to.timestamp);
    if (longGap) {
      setHover({ kind: "gap", gap: longGap, x, short: false });
      return;
    }
    const shortGap = split.bridges.find((gap) => timestamp > gap.from.timestamp && timestamp < gap.to.timestamp);
    setHover(shortGap ? { kind: "gap", gap: shortGap, x, short: true } : null);
  };

  const onPointerDown = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (!scope) return;
    const point = svgPoint(event.clientX, event.clientY);
    if (point.x < CHART.left || point.x > CHART.right || point.y < CHART.top || point.y > CHART.bottom) return;
    dragRef.current = { pointerId: event.pointerId, startX: point.x, domain: viewDomain };
    setIsDragging(true);
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: ReactPointerEvent<SVGSVGElement>) => {
    const point = svgPoint(event.clientX, event.clientY);
    const drag = dragRef.current;
    if (drag?.pointerId === event.pointerId) {
      const span = drag.domain[1] - drag.domain[0];
      const shift = -((point.x - drag.startX) / PLOT_WIDTH) * span;
      setViewDomain(clampDomain(
        drag.domain[0] + shift,
        drag.domain[1] + shift,
        totalDomain[0],
        totalDomain[1],
      ));
      setHover(null);
      return;
    }
    updateHover(point.x);
  };

  const endPointer = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    dragRef.current = null;
    setIsDragging(false);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const onWheel = (event: WheelEvent<HTMLDivElement>) => {
    if (!scope) return;
    const point = svgPoint(event.clientX, event.clientY);
    const insidePlot = point.x >= CHART.left && point.x <= CHART.right
      && point.y >= CHART.top && point.y <= CHART.bottom;
    if (!shouldZoomUsageChart(event, insidePlot)) return;
    event.preventDefault();
    event.stopPropagation();
    const totalSpan = totalDomain[1] - totalDomain[0];
    const currentSpan = viewDomain[1] - viewDomain[0];
    const minSpan = Math.max(scope.bucketMs * 4, totalSpan * 0.08);
    const nextSpan = Math.min(totalSpan, Math.max(minSpan, currentSpan * (event.deltaY > 0 ? 1.18 : 0.84)));
    const anchorRatio = (point.x - CHART.left) / PLOT_WIDTH;
    const anchorTime = viewDomain[0] + currentSpan * anchorRatio;
    const nextMin = anchorTime - nextSpan * anchorRatio;
    setViewDomain(clampDomain(nextMin, nextMin + nextSpan, totalDomain[0], totalDomain[1]));
  };

  const resetView = () => {
    setViewDomain(totalDomain);
    setHover(null);
  };

  const tooltipPos = hover?.kind === "sample"
    ? usageTooltipPlacement({ x: hover.x, y: hover.y }, CHART.width, CHART.height)
    : null;
  const tooltipClass = [
    "qt-usage-tooltip",
    hover?.kind === "gap" ? "is-gap" : "",
    tooltipPos ? (tooltipPos.below ? "is-below" : "is-above") : "",
  ].filter(Boolean).join(" ");
  const tooltipStyle: CSSProperties = tooltipPos
    ? { left: `${tooltipPos.leftPct}%`, top: `${tooltipPos.topPct}%` }
    : hover
      ? { left: `${Math.min(82, Math.max(10, (hover.x / CHART.width) * 100))}%` }
      : {};
  const pageState = providersLoading
    ? { kind: "loading" as const, message: t("usage.loadingProviders") }
    : providersError
      ? { kind: "error" as const, message: t("usage.providersError", { msg: String(providersError) }) }
      : providers.length === 0
        ? { kind: "empty" as const, message: t("usage.noProviders") }
        : history.isLoading
          ? { kind: "loading" as const, message: t("usage.loadingHistory") }
          : history.isError
            ? { kind: "error" as const, message: t("usage.historyError", { msg: String(history.error) }) }
            : !scope || !current
              ? { kind: "empty" as const, message: t("usage.emptyHistory") }
              : null;

  return (
    <section className="qt-usage-page" aria-label={t("usage.title")}>
      <div className="qt-usage-toolbar">
        <div className="qt-usage-selectors">
          <label className="qt-usage-selector">
            <span>{t("usage.provider")}</span>
            <select
              value={providerId}
              disabled={providers.length === 0}
              onChange={(event) => selectProvider(event.target.value)}
            >
              {providers.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
            </select>
          </label>
          <span className="qt-usage-selector-arrow" aria-hidden="true">→</span>
          <label className="qt-usage-selector">
            <span>{t("usage.scope")}</span>
            <select
              value={scopeId}
              disabled={scopes.length === 0}
              onChange={(event) => selectScope(event.target.value)}
            >
              {scopes.map((item) => <option key={item.windowKey} value={item.windowKey}>{item.name}</option>)}
            </select>
          </label>
        </div>
        <div className="qt-usage-range-switch">
          <SegmentedControl
            value={range}
            onChange={selectRange}
            compact
            options={[
              { value: "24h", label: t("usage.range24h") },
              { value: "7d", label: t("usage.range7d") },
            ]}
          />
        </div>
      </div>

      {pageState ? (
        <div className={`qt-usage-state is-${pageState.kind}`}>
          <strong>{pageState.kind === "empty" ? t("usage.emptyTitle") : t("usage.statusTitle")}</strong>
          <span>{pageState.message}</span>
        </div>
      ) : scope && current && provider ? (
        <article
          className="qt-usage-chart-card"
          style={{ "--qt-series-color": scope.color } as CSSProperties}
        >
          <header className="qt-usage-chart-head">
            <div>
              <p className="qt-usage-eyebrow">
                <span style={{ background: scope.color }} />
                {provider.name} · {scope.name}
              </p>
              <div className="qt-usage-current">
                <strong>{formatNumber(current.value, scope.metric)}</strong>
                <span>{scope.metric === "percent" ? t("usage.remainingQuota") : scope.unit}</span>
              </div>
              <p className="qt-usage-updated">{t("usage.lastSample", { time: dateFormatter.format(current.timestamp) })}</p>
            </div>
            <div className="qt-usage-chart-actions">
              <span className={`qt-usage-axis-chip ${scope.metric === "absolute" ? "is-active" : ""}`}>
                {t("usage.leftAxis")}
              </span>
              <span className={`qt-usage-axis-chip ${scope.metric === "percent" ? "is-active" : ""}`}>
                {t("usage.rightAxis")}
              </span>
              <Button icon={RotateCcw} className="qt-usage-reset" onClick={resetView}>
                {t("usage.resetView")}
              </Button>
            </div>
          </header>

          <div
            className={`qt-usage-chart-wrap ${chartHovered ? "is-hovered" : ""} ${isDragging ? "is-dragging" : ""}`}
            onWheelCapture={onWheel}
          >
            <svg
              ref={svgRef}
              className="qt-usage-chart"
              viewBox={`0 0 ${CHART.width} ${CHART.height}`}
              role="img"
              aria-label={t("usage.chartLabel", { provider: provider.name, scope: scope.name })}
              onPointerEnter={() => setChartHovered(true)}
              onPointerLeave={() => {
                setChartHovered(false);
                if (!mobile && !isDragging) setHover(null);
              }}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={endPointer}
              onPointerCancel={endPointer}
              onDoubleClick={resetView}
            >
              <defs>
                <clipPath id="qt-usage-plot-clip">
                  <rect x={CHART.left} y={CHART.top} width={PLOT_WIDTH} height={PLOT_HEIGHT} rx="2" />
                </clipPath>
                <linearGradient id="qt-usage-gap-fade" x1="0" x2="1">
                  <stop offset="0" stopColor="var(--qt-surface-soft)" stopOpacity="0" />
                  <stop offset=".16" stopColor="var(--qt-surface-soft)" stopOpacity=".72" />
                  <stop offset=".84" stopColor="var(--qt-surface-soft)" stopOpacity=".72" />
                  <stop offset="1" stopColor="var(--qt-surface-soft)" stopOpacity="0" />
                </linearGradient>
                <pattern id="qt-usage-gap-pattern" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
                  <line x1="0" y1="0" x2="0" y2="8" className="qt-usage-gap-stripe" />
                </pattern>
              </defs>

              {activeTicks.map((tick) => {
                const max = scope.metric === "percent" ? 100 : absoluteScale.max;
                const y = CHART.bottom - (tick / max) * PLOT_HEIGHT;
                return <line key={`h-${tick}`} className="qt-usage-grid-line" x1={CHART.left} x2={CHART.right} y1={y} y2={y} />;
              })}
              {xTicks.map((timestamp) => {
                const x = xOf(timestamp);
                return <line key={`v-${timestamp}`} className="qt-usage-grid-line" x1={x} x2={x} y1={CHART.top} y2={CHART.bottom} />;
              })}

              <g className={`qt-usage-axis qt-usage-axis-left ${scope.metric === "absolute" ? "is-active" : ""}`}>
                <text className="qt-usage-axis-title" x={CHART.left} y="20">{t("usage.absoluteAxisTitle")}</text>
                {(scope.metric === "absolute" ? absoluteScale.ticks : percentTicks).map((tick) => {
                  const max = scope.metric === "absolute" ? absoluteScale.max : 100;
                  const y = CHART.bottom - (tick / max) * PLOT_HEIGHT;
                  return (
                    <g key={`left-${tick}`}>
                      <line x1={CHART.left - 7} x2={CHART.left} y1={y} y2={y} />
                      {scope.metric === "absolute" && <text x={CHART.left - 12} y={y + 4} textAnchor="end">{formatAxisNumber(tick)}</text>}
                    </g>
                  );
                })}
              </g>
              <g className={`qt-usage-axis qt-usage-axis-right ${scope.metric === "percent" ? "is-active" : ""}`}>
                <text className="qt-usage-axis-title" x={CHART.right} y="20" textAnchor="end">{t("usage.remainingPercentAxisTitle")}</text>
                {percentTicks.map((tick) => {
                  const y = CHART.bottom - (tick / 100) * PLOT_HEIGHT;
                  return (
                    <g key={`right-${tick}`}>
                      <line x1={CHART.right} x2={CHART.right + 7} y1={y} y2={y} />
                      {scope.metric === "percent" && <text x={CHART.right + 12} y={y + 4} textAnchor="start">{tick}%</text>}
                    </g>
                  );
                })}
              </g>
              <g className="qt-usage-axis qt-usage-axis-bottom is-active">
                {xTicks.map((timestamp, index) => {
                  const x = xOf(timestamp);
                  return (
                    <g key={`x-${timestamp}`}>
                      <line x1={x} x2={x} y1={CHART.bottom} y2={CHART.bottom + 7} />
                      <text x={x} y={CHART.bottom + 27} textAnchor={index === 0 ? "start" : index === xTicks.length - 1 ? "end" : "middle"}>
                        {axisDateFormatter.format(timestamp)}
                      </text>
                    </g>
                  );
                })}
              </g>

              <g clipPath="url(#qt-usage-plot-clip)">
                {split.gaps.map((gap) => {
                  const from = xOf(gap.from.timestamp);
                  const to = xOf(gap.to.timestamp);
                  return (
                    <g key={`gap-${gap.from.timestamp}`}>
                      <rect className="qt-usage-gap-fill" x={from} y={CHART.top} width={to - from} height={PLOT_HEIGHT} fill="url(#qt-usage-gap-fade)" />
                      <rect x={from} y={CHART.top} width={to - from} height={PLOT_HEIGHT} fill="url(#qt-usage-gap-pattern)" opacity=".34" />
                    </g>
                  );
                })}
                {split.bridges.map((bridge) => (
                  <line
                    key={`bridge-${bridge.from.timestamp}`}
                    className="qt-usage-bridge"
                    x1={xOf(bridge.from.timestamp)}
                    y1={yOf(bridge.from.value)}
                    x2={xOf(bridge.to.timestamp)}
                    y2={yOf(bridge.to.value)}
                  />
                ))}
                {geometries.map((geometry, index) => geometry.path && (
                  <path key={`line-${index}`} className="qt-usage-series" d={geometry.path} />
                ))}
                {split.segments.flatMap((segment) => segment.length === 1 ? segment : []).map((sample) => (
                  <circle key={`solo-${sample.timestamp}`} className="qt-usage-endpoint" cx={xOf(sample.timestamp)} cy={yOf(sample.value)} r="4" />
                ))}
                {[...split.bridges, ...split.gaps].flatMap((gap) => [gap.from, gap.to]).map((sample, index) => (
                  <circle key={`edge-${sample.timestamp}-${index}`} className="qt-usage-gap-edge" cx={xOf(sample.timestamp)} cy={yOf(sample.value)} r="4.5" />
                ))}
                {hover?.kind === "sample" && (
                  <>
                    <line className="qt-usage-crosshair" x1={hover.x} x2={hover.x} y1={CHART.top} y2={CHART.bottom} />
                    <circle className="qt-usage-hover-halo" cx={hover.x} cy={hover.y} r="10" />
                    <circle className="qt-usage-hover-dot" cx={hover.x} cy={hover.y} r="4.5" />
                  </>
                )}
              </g>
            </svg>

            {hover && (
              <div className={tooltipClass} style={tooltipStyle}>
                {hover.kind === "sample" ? (
                  <>
                    <span className="qt-usage-tooltip-time">{dateFormatter.format(hover.sample.timestamp)}</span>
                    <strong>{formatNumber(hover.sample.value, scope.metric)}</strong>
                    <span>{provider.name} · {scope.name}</span>
                  </>
                ) : (
                  <>
                    <span className="qt-usage-tooltip-time">{hover.short ? t("usage.shortGap") : t("usage.noData")}</span>
                    <strong>{t("usage.missingBuckets", { count: hover.gap.missingBuckets })}</strong>
                    <span>{dateFormatter.format(hover.gap.from.timestamp)} – {dateFormatter.format(hover.gap.to.timestamp)}</span>
                    <em>{t("usage.noDataHint")}</em>
                  </>
                )}
              </div>
            )}
          </div>

          <footer className="qt-usage-chart-footer">
            <span><MousePointer2 size={14} aria-hidden="true" />{t("usage.dragHint")}</span>
            <span><Maximize2 size={14} aria-hidden="true" />{t("usage.zoomHint")}</span>
            <span className="qt-usage-legend-gap"><i />{t("usage.gapLegend")}</span>
          </footer>
        </article>
      ) : null}
    </section>
  );
}
