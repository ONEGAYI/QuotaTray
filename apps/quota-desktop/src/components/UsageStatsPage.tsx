import { Maximize2, MousePointer2, Plus, RotateCcw, Settings2 } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent, type WheelEvent } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import { useHistories, useSettings } from "../queries";
import type { ProviderEntry, Settings, UsageComparisonSeries } from "../types";
import { UsageComparisonDialog, type UsageComparisonCandidate, type UsageComparisonDialogMode } from "./UsageComparisonDialog";
import { detailComparisonIds, initialUsageComparisons, partitionCompatibleUsageScopes, shouldShowFocusedGap, usageComparisonId, usageTooltipDock } from "./usageComparisonView";
import { Button, SegmentedControl } from "./ui";
import { advanceUsageViewDomain, buildHistorySeries, buildLineGeometry, isolatedUsageSamples, niceAbsoluteScale, shouldZoomUsageChart, splitUsageSeries, USAGE_RANGES, usageSmoothingRadius, type HistorySeries, type UsageDomain, type UsageRange, type UsageSample } from "./usageChartView";

interface UsageScope extends HistorySeries {
  id: string;
  providerId: string;
  providerName: string;
  name: string;
  colorSlot: number;
  bucketMs: number;
}

interface CursorState { timestamp: number; x: number; dock: "top" | "bottom"; }
interface Props { providers: ProviderEntry[]; providersLoading: boolean; providersError?: unknown; mobile?: boolean; }

const HISTORY_FETCH_SPAN_MS = Math.max(...Object.values(USAGE_RANGES).map((item) => item.spanMs));
const SERIES_COLORS = ["var(--qt-series-1)", "var(--qt-series-2)", "var(--qt-series-3)", "var(--qt-series-4)"];

function scopeName(windowKey: string, index: number, lang: "zh" | "en"): string {
  const bracketed = windowKey.match(/（([^）]+)）|\(([^)]+)\)/)?.slice(1).find(Boolean);
  if (bracketed) {
    if (bracketed.toLowerCase() === "week") return lang === "zh" ? "周限" : "Weekly";
    return bracketed;
  }
  if (/^w\d+(?:#\d+)?$/.test(windowKey)) return lang === "zh" ? `窗口 ${index + 1}` : `Window ${index + 1}`;
  return windowKey;
}

function formatNumber(value: number, metric: HistorySeries["metric"]): string {
  return metric === "percent" ? `${Math.round(value)}%` : value.toLocaleString(undefined, { maximumFractionDigits: 1 });
}

function formatAxisNumber(value: number): string {
  if (Math.abs(value) >= 1_000) return `${Math.round(value / 1_000)}k`;
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

function nearestSample(scope: UsageScope, timestamp: number): UsageSample | null {
  const nearest = scope.samples.reduce<UsageSample | null>((best, sample) => !best || Math.abs(sample.timestamp - timestamp) < Math.abs(best.timestamp - timestamp) ? sample : best, null);
  return nearest && Math.abs(nearest.timestamp - timestamp) <= scope.bucketMs ? nearest : null;
}

function clampDomain(min: number, max: number, totalMin: number, totalMax: number): UsageDomain {
  const span = max - min;
  if (min < totalMin) return [totalMin, totalMin + span];
  if (max > totalMax) return [totalMax - span, totalMax];
  return [min, max];
}

export function UsageStatsPage({ providers, providersLoading, providersError, mobile = false }: Props) {
  const { lang, t } = useLang();
  const qc = useQueryClient();
  const settings = useSettings();
  const providerIds = useMemo(() => providers.map((provider) => provider.id), [providers]);
  const histories = useHistories(providerIds, HISTORY_FETCH_SPAN_MS);
  const [rangeNow, setRangeNow] = useState(() => Date.now());
  const [range, setRange] = useState<UsageRange>("7d");
  const [viewDomain, setViewDomain] = useState<UsageDomain>(() => [rangeNow - USAGE_RANGES["7d"].spanMs, rangeNow]);
  const [cursor, setCursor] = useState<CursorState | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [dialogMode, setDialogMode] = useState<UsageComparisonDialogMode | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const svgRef = useRef<SVGSVGElement>(null);
  const previousTotalRef = useRef<UsageDomain>(viewDomain);
  const autoInitRef = useRef(false);
  const dragRef = useRef<{ pointerId: number; startX: number; domain: UsageDomain } | null>(null);
  const rangeConfig = USAGE_RANGES[range];
  const totalDomain = useMemo<UsageDomain>(() => [rangeNow - rangeConfig.spanMs, rangeNow], [rangeNow, rangeConfig.spanMs]);
  const chart = mobile ? { width: 360, height: 300, left: 42, right: 326, top: 28, bottom: 258 } : { width: 840, height: 410, left: 74, right: 766, top: 42, bottom: 338 };
  const plotWidth = chart.right - chart.left;
  const plotHeight = chart.bottom - chart.top;

  useEffect(() => { const timer = window.setInterval(() => setRangeNow(Date.now()), 60_000); return () => window.clearInterval(timer); }, []);
  useEffect(() => {
    const newest = histories.flatMap((query) => query.data ?? []).reduce((max, point) => Math.max(max, point.sampled_at), 0);
    if (newest > rangeNow) setRangeNow(newest);
  }, [histories, rangeNow]);
  useEffect(() => {
    const previous = previousTotalRef.current;
    if (previous[0] !== totalDomain[0] || previous[1] !== totalDomain[1]) {
      setViewDomain((view) => advanceUsageViewDomain(view, previous, totalDomain));
      previousTotalRef.current = totalDomain;
    }
  }, [totalDomain]);

  const candidates = useMemo<UsageComparisonCandidate[]>(() => providers.flatMap((provider, providerIndex) => {
    const points = histories[providerIndex]?.data ?? [];
    return buildHistorySeries(points, USAGE_RANGES["7d"].bucketMs).map((series, index) => ({
      id: usageComparisonId(provider.id, series.windowKey), providerId: provider.id, providerName: provider.name,
      windowKey: series.windowKey, windowName: scopeName(series.windowKey, index, lang), metric: series.metric, unit: series.unit,
    }));
  }), [histories, lang, providers]);

  const storedSelection = settings.data?.usage_comparison_series;
  const effectiveSelection = useMemo<UsageComparisonSeries[]>(() => {
    return initialUsageComparisons(storedSelection ?? null, candidates);
  }, [candidates, storedSelection]);
  const saveSelection = useCallback(async (next: UsageComparisonSeries[]) => {
    await api.patchSettings({ usage_comparison_series: next });
    qc.setQueryData<Settings | undefined>(["settings"], (current) => current ? { ...current, usage_comparison_series: next } : current);
  }, [qc]);
  useEffect(() => {
    if (settings.data?.usage_comparison_series !== null) {
      autoInitRef.current = false;
      return;
    }
    if (candidates.length === 0 || autoInitRef.current) return;
    autoInitRef.current = true;
    void saveSelection([{ provider_id: candidates[0].providerId, window_key: candidates[0].windowKey, color_slot: 0 }]).catch(() => { autoInitRef.current = false; });
  }, [candidates, saveSelection, settings.data?.usage_comparison_series]);

  const scopePartition = useMemo(() => {
    const builtScopes = effectiveSelection.flatMap((selection) => {
      const providerIndex = providers.findIndex((provider) => provider.id === selection.provider_id);
      if (providerIndex < 0) return [];
      const provider = providers[providerIndex];
      const points = (histories[providerIndex]?.data ?? []).filter((point) => point.sampled_at >= totalDomain[0] && point.sampled_at <= totalDomain[1]);
      const built = buildHistorySeries(points, rangeConfig.bucketMs);
      const seriesIndex = built.findIndex((item) => item.windowKey === selection.window_key);
      if (seriesIndex < 0) return [];
      const series = built[seriesIndex];
      const stableName = candidates.find((candidate) => candidate.id === usageComparisonId(provider.id, series.windowKey))?.windowName;
      return [{ ...series, id: usageComparisonId(provider.id, series.windowKey), providerId: provider.id, providerName: provider.name, name: stableName ?? scopeName(series.windowKey, seriesIndex, lang), colorSlot: selection.color_slot, bucketMs: rangeConfig.bucketMs }];
    });
    return partitionCompatibleUsageScopes(builtScopes);
  }, [candidates, effectiveSelection, histories, lang, providers, rangeConfig.bucketMs, totalDomain]);
  const scopes: UsageScope[] = scopePartition.visible;
  useEffect(() => { if (focusedId && !scopes.some((scope) => scope.id === focusedId)) setFocusedId(null); }, [focusedId, scopes]);

  const absoluteScale = niceAbsoluteScale(scopes.filter((scope) => scope.metric === "absolute").flatMap((scope) => scope.samples.map((sample) => sample.value)));
  const percentPresent = scopes.some((scope) => scope.metric === "percent");
  const absolutePresent = scopes.some((scope) => scope.metric === "absolute");
  const xOf = (timestamp: number) => chart.left + ((timestamp - viewDomain[0]) / (viewDomain[1] - viewDomain[0])) * plotWidth;
  const yOf = (scope: UsageScope, value: number) => chart.bottom - (value / (scope.metric === "percent" ? 100 : absoluteScale.max)) * plotHeight;
  const xTickCount = mobile ? 3 : 7;
  const xTicks = Array.from({ length: xTickCount }, (_, index) => viewDomain[0] + ((viewDomain[1] - viewDomain[0]) * index) / (xTickCount - 1));
  const yTicks = [0, 25, 50, 75, 100];
  const detailIds = detailComparisonIds(scopes.map((scope) => scope.id), focusedId);
  const detailScopes = scopes.filter((scope) => detailIds.includes(scope.id));
  const smoothingRadius = usageSmoothingRadius(rangeConfig.bucketMs);
  const splitScopes = useMemo(() => scopes.map((scope) => ({
    scope,
    split: splitUsageSeries(scope.samples, scope.bucketMs),
  })), [scopes]);
  const focusedHasLongGap = splitScopes.some(({ scope, split }) => (
    shouldShowFocusedGap(focusedId, scope.id) && split.gaps.length > 0
  ));
  const dateFormatter = new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  const axisFormatter = new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", { month: "numeric", day: "numeric", hour: "2-digit" });

  const svgPoint = (clientX: number, clientY: number) => { const bounds = svgRef.current?.getBoundingClientRect(); return bounds ? { x: ((clientX - bounds.left) / bounds.width) * chart.width, y: ((clientY - bounds.top) / bounds.height) * chart.height } : { x: 0, y: 0 }; };
  const updateCursor = (point: { x: number; y: number }) => {
    if (point.x < chart.left || point.x > chart.right || point.y < chart.top || point.y > chart.bottom) return;
    setCursor({ timestamp: viewDomain[0] + ((point.x - chart.left) / plotWidth) * (viewDomain[1] - viewDomain[0]), x: point.x, dock: usageTooltipDock(point.y, chart.height) });
  };
  const onPointerDown = (event: ReactPointerEvent<SVGSVGElement>) => {
    const point = svgPoint(event.clientX, event.clientY);
    if (mobile) {
      if (point.x < chart.left || point.x > chart.right || point.y < chart.top || point.y > chart.bottom) setCursor(null);
      else updateCursor(point);
    } else { dragRef.current = { pointerId: event.pointerId, startX: point.x, domain: viewDomain }; setIsDragging(true); }
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const onPointerMove = (event: ReactPointerEvent<SVGSVGElement>) => {
    const point = svgPoint(event.clientX, event.clientY); const drag = dragRef.current;
    if (!mobile && drag?.pointerId === event.pointerId) { const span = drag.domain[1] - drag.domain[0]; const shift = -((point.x - drag.startX) / plotWidth) * span; setViewDomain(clampDomain(drag.domain[0] + shift, drag.domain[1] + shift, totalDomain[0], totalDomain[1])); setCursor(null); }
    else if (!mobile || event.buttons > 0) updateCursor(point);
  };
  const endPointer = (event: ReactPointerEvent<SVGSVGElement>) => { dragRef.current = null; setIsDragging(false); if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId); };
  const onWheel = (event: WheelEvent<HTMLDivElement>) => {
    if (mobile || scopes.length === 0) return; const point = svgPoint(event.clientX, event.clientY); const inside = point.x >= chart.left && point.x <= chart.right && point.y >= chart.top && point.y <= chart.bottom;
    if (!shouldZoomUsageChart(event, inside)) return; event.preventDefault(); const totalSpan = totalDomain[1] - totalDomain[0]; const currentSpan = viewDomain[1] - viewDomain[0]; const minSpan = Math.max(rangeConfig.bucketMs * 4, totalSpan * .08); const nextSpan = Math.min(totalSpan, Math.max(minSpan, currentSpan * (event.deltaY > 0 ? 1.18 : .84))); const ratio = (point.x - chart.left) / plotWidth; const anchor = viewDomain[0] + currentSpan * ratio; const nextMin = anchor - nextSpan * ratio; setViewDomain(clampDomain(nextMin, nextMin + nextSpan, totalDomain[0], totalDomain[1])); setCursor(null);
  };
  const resetView = () => { setViewDomain(totalDomain); setCursor(null); };
  const selectRange = (next: UsageRange) => { setRange(next); setViewDomain([rangeNow - USAGE_RANGES[next].spanMs, rangeNow]); setCursor(null); };
  const pageState = providersLoading || settings.isLoading ? { kind: "loading", message: t("usage.loadingProviders") } : providersError ? { kind: "error", message: t("usage.providersError", { msg: String(providersError) }) } : settings.isError ? { kind: "error", message: t("usage.settingsError", { msg: String(settings.error) }) } : providers.length === 0 ? { kind: "empty", message: t("usage.noProviders") } : effectiveSelection.length === 0 ? { kind: "empty", message: t("usage.emptySelection") } : scopes.length === 0 && histories.some((query) => query.isLoading) ? { kind: "loading", message: t("usage.loadingHistory") } : scopes.length === 0 ? { kind: "empty", message: t("usage.emptyHistory") } : null;
  const partialErrors = histories.filter((query) => query.isError).length;
  const cursorRows = cursor ? detailScopes.map((scope) => ({ scope, sample: nearestSample(scope, cursor.timestamp) })) : [];

  return <section className="qt-usage-page" aria-label={t("usage.title")}>
    <div className="qt-usage-toolbar"><div className="qt-usage-comparison-actions"><Button icon={Plus} onClick={() => setDialogMode("add")}>{t("usage.addCombination")} <span>{t("usage.combinationCount", { count: effectiveSelection.length })}</span></Button><Button icon={Settings2} variant="ghost" onClick={() => setDialogMode("manage")}>{t("usage.manageCombinations")}</Button></div><div className="qt-usage-range-switch"><SegmentedControl value={range} onChange={selectRange} compact options={[{ value: "24h", label: t("usage.range24h") }, { value: "7d", label: t("usage.range7d") }]} /></div></div>
    {partialErrors > 0 && <div className="qt-inline-warning qt-usage-partial-warning">{t("usage.historyError", { msg: String(partialErrors) })}</div>}
    {scopePartition.hidden.length > 0 && <div className="qt-inline-warning qt-usage-partial-warning">{t("usage.unitConflictHidden", { count: scopePartition.hidden.length, unit: scopePartition.absoluteUnit ?? "—" })}</div>}
    {!mobile && scopes.length > 0 && <div className="qt-usage-series-summary">{scopes.map((scope) => { const current = scope.samples[scope.samples.length - 1]; return <button key={scope.id} type="button" aria-pressed={focusedId === scope.id} style={{ "--qt-series-color": SERIES_COLORS[scope.colorSlot] } as CSSProperties} onClick={() => setFocusedId((value) => value === scope.id ? null : scope.id)}><i /><span>{scope.providerName} · {scope.name}</span><strong>{current ? formatNumber(current.value, scope.metric) : "—"}</strong></button>; })}</div>}
    {mobile && scopes.length > 0 && <div className="qt-usage-mobile-focus">{scopes.map((scope) => <button key={scope.id} type="button" aria-pressed={focusedId === scope.id} style={{ "--qt-series-color": SERIES_COLORS[scope.colorSlot] } as CSSProperties} onClick={() => setFocusedId((value) => value === scope.id ? null : scope.id)}>{scope.providerName} · {scope.name}</button>)}</div>}
    {pageState ? <div className={`qt-usage-state is-${pageState.kind}`}><strong>{pageState.kind === "empty" ? t("usage.emptyTitle") : t("usage.statusTitle")}</strong><span>{pageState.message}</span></div> : <article className="qt-usage-chart-card">
      <header className="qt-usage-chart-head"><div><p className="qt-usage-eyebrow">{t("usage.title")}</p><p className="qt-usage-updated">{t("usage.comparisonChartLabel", { count: scopes.length })}</p></div><Button icon={RotateCcw} className="qt-usage-reset" onClick={() => { if (focusedId) setFocusedId(null); else resetView(); }}>{focusedId ? t("usage.resetFocus") : t("usage.resetView")}</Button></header>
      <div className={`qt-usage-chart-wrap ${isDragging ? "is-dragging" : ""}`} onWheelCapture={onWheel}>
        <svg ref={svgRef} className="qt-usage-chart" viewBox={`0 0 ${chart.width} ${chart.height}`} role="img" aria-label={t("usage.comparisonChartLabel", { count: scopes.length })} onPointerLeave={() => { if (!mobile && !isDragging) setCursor(null); }} onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={endPointer} onPointerCancel={endPointer} onDoubleClick={() => { if (!mobile) resetView(); }}>
          <defs>
            <clipPath id="qt-usage-plot-clip"><rect x={chart.left} y={chart.top} width={plotWidth} height={plotHeight} rx="2" /></clipPath>
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
          {yTicks.map((tick) => <line key={tick} className="qt-usage-grid-line" x1={chart.left} x2={chart.right} y1={chart.bottom - tick / 100 * plotHeight} y2={chart.bottom - tick / 100 * plotHeight} />)}{xTicks.map((tick) => <line key={tick} className="qt-usage-grid-line" x1={xOf(tick)} x2={xOf(tick)} y1={chart.top} y2={chart.bottom} />)}
          <g className={`qt-usage-axis qt-usage-axis-left ${absolutePresent ? "is-active" : ""}`}>{absolutePresent && yTicks.map((tick) => { const y = chart.bottom - tick / 100 * plotHeight; return <g key={tick}><line x1={chart.left - 7} x2={chart.left} y1={y} y2={y} /><text x={chart.left - 11} y={y + 4} textAnchor="end">{formatAxisNumber(absoluteScale.max * tick / 100)}</text></g>; })}</g>
          <g className={`qt-usage-axis qt-usage-axis-right ${percentPresent ? "is-active" : ""}`}>{percentPresent && yTicks.map((tick) => { const y = chart.bottom - tick / 100 * plotHeight; return <g key={tick}><line x1={chart.right} x2={chart.right + 7} y1={y} y2={y} /><text x={chart.right + 11} y={y + 4}>{tick}%</text></g>; })}</g>
          <g className="qt-usage-axis qt-usage-axis-bottom is-active">{xTicks.map((tick, index) => <text key={tick} x={xOf(tick)} y={chart.bottom + 24} textAnchor={index === 0 ? "start" : index === xTicks.length - 1 ? "end" : "middle"}>{axisFormatter.format(tick)}</text>)}</g>
          <g clipPath="url(#qt-usage-plot-clip)">{splitScopes.map(({ scope, split }) => { const muted = focusedId && focusedId !== scope.id; const showLongGaps = shouldShowFocusedGap(focusedId, scope.id); return <g key={scope.id} style={{ "--qt-series-color": SERIES_COLORS[scope.colorSlot] } as CSSProperties} className={muted ? "is-muted" : focusedId === scope.id ? "is-focused" : ""}>{showLongGaps && split.gaps.map((gap) => { const from = xOf(gap.from.timestamp); const to = xOf(gap.to.timestamp); return <g key={`gap-${gap.from.timestamp}`}><rect className="qt-usage-gap-fill" x={from} y={chart.top} width={to - from} height={plotHeight} fill="url(#qt-usage-gap-fade)" /><rect x={from} y={chart.top} width={to - from} height={plotHeight} fill="url(#qt-usage-gap-pattern)" opacity=".34" /><circle className="qt-usage-gap-edge" cx={from} cy={yOf(scope, gap.from.value)} r="4.5" /><circle className="qt-usage-gap-edge" cx={to} cy={yOf(scope, gap.to.value)} r="4.5" /></g>; })}{split.bridges.map((bridge) => <line key={bridge.from.timestamp} className="qt-usage-bridge" x1={xOf(bridge.from.timestamp)} y1={yOf(scope, bridge.from.value)} x2={xOf(bridge.to.timestamp)} y2={yOf(scope, bridge.to.value)} />)}{split.segments.map((segment, index) => { const geometry = buildLineGeometry(segment, xOf, (value) => yOf(scope, value), { smoothingRadius }); return geometry.path ? <path key={index} className="qt-usage-series" d={geometry.path} /> : null; })}{isolatedUsageSamples(split).map((sample) => <circle key={`solo-${sample.timestamp}`} className="qt-usage-endpoint" cx={xOf(sample.timestamp)} cy={yOf(scope, sample.value)} r="4" />)}{cursor && (() => { const sample = nearestSample(scope, cursor.timestamp); return sample && detailIds.includes(scope.id) ? <circle className="qt-usage-hover-dot" cx={cursor.x} cy={yOf(scope, sample.value)} r="4" /> : null; })()}</g>; })}{cursor && <line className="qt-usage-crosshair" x1={cursor.x} x2={cursor.x} y1={chart.top} y2={chart.bottom} />}</g>
        </svg>
        {!mobile && cursor && <div className={`qt-usage-tooltip is-docked-${cursor.dock}`} style={{ left: `${Math.min(82, Math.max(12, cursor.x / chart.width * 100))}%` }}><span className="qt-usage-tooltip-time">{dateFormatter.format(cursor.timestamp)}</span>{cursorRows.map(({ scope, sample }) => <div key={scope.id} className="qt-usage-tooltip-row" style={{ "--qt-series-color": SERIES_COLORS[scope.colorSlot] } as CSSProperties}><i /><span>{scope.providerName} · {scope.name}</span><strong>{sample ? formatNumber(sample.value, scope.metric) : "—"}</strong></div>)}</div>}
      </div>
      {mobile && cursor && <div className="qt-usage-mobile-readout" aria-live="polite"><span>{dateFormatter.format(cursor.timestamp)}</span>{cursorRows.map(({ scope, sample }) => <div key={scope.id} style={{ "--qt-series-color": SERIES_COLORS[scope.colorSlot] } as CSSProperties}><i /><span>{scope.providerName} · {scope.name}</span><strong>{sample ? formatNumber(sample.value, scope.metric) : "—"}</strong></div>)}</div>}
      {!mobile && <footer className="qt-usage-chart-footer"><span><MousePointer2 size={14} aria-hidden="true" />{t("usage.dragHint")}</span><span><Maximize2 size={14} aria-hidden="true" />{t("usage.zoomHint")}</span><span>{t("usage.focusHint")}</span>{focusedHasLongGap && <span className="qt-usage-legend-gap"><i />{t("usage.gapLegend")}</span>}</footer>}
    </article>}
    {dialogMode && <UsageComparisonDialog mode={dialogMode} candidates={candidates} selected={effectiveSelection} onClose={() => setDialogMode(null)} onSave={saveSelection} />}
  </section>;
}
