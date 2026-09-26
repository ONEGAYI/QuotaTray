import { QueryClient, QueryClientProvider, useMutation, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, RefreshCw, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { amountText, dataSummary, kindLabel, preferMetric, relativeTime, remainingPercent, resetCountdown, windowShortLabel } from "../display";
import { LangProvider, useLang } from "../i18n";
import {
  useNativeMetas,
  usePeakFlipTick,
  useProviderState,
  useProviderStateEvents,
  useProviders,
  useSettings,
  useSnapshots,
} from "../queries";
import { ThemeProvider } from "../theme";
import type { PrimaryMetric, ProviderEntry, Settings } from "../types";
import { hoverRingView, isCompactViewport, resolveHoverProvider } from "./hoverPanelView";
import { BrandMark } from "./BrandMark";
import { deriveProviderCardState } from "./providerCardView";
import {
  pricingModelChoices,
  resolveProviderPricingView,
  withProviderModel,
} from "./providerPricing";
import { formatPrice } from "./pricingDraft";

const hoverQueryClient = new QueryClient();

/** hero 主数值形状：label 为语义键（文案由调用处 i18n 渲染）。 */
type HeroValue = { label: "remaining" | "available" | "empty"; value: string; unit: string };

/** 悬停面板 hero 主数值（label 为语义键，文案由调用处 i18n 渲染）。
 *  主度量偏好分档经 display.preferMetric 骨架（T-24，#142；PR #146
 *  review 抽取）：amount 档金额优先（「可用余额」族）、auto/percent
 *  百分比优先（「剩余」族）；算不出时静默回退另一度量。
 *  与 ProviderCard 的 primaryValue 同款分档。 */
function primaryValue(
  data: ReturnType<typeof deriveProviderCardState>["data"][number] | undefined,
  metric: PrimaryMetric = "auto",
): HeroValue {
  if (!data) return { label: "empty", value: "—", unit: "" };
  const percentPart = (): HeroValue | null => {
    const percent = remainingPercent(data);
    if (percent == null) return null;
    return { label: "remaining", value: `${Math.round(percent)}%`, unit: "" };
  };
  const amountPart = (): HeroValue | null => {
    if (data.remaining == null) return null;
    return { label: "available", value: amountText(data.remaining), unit: data.unit ?? "" };
  };
  const fallback: HeroValue = { label: "empty", value: "—", unit: data.unit ?? "" };
  return preferMetric(metric, percentPart, amountPart) ?? fallback;
}

function statusKey(kind: ReturnType<typeof deriveProviderCardState>["kind"]) {
  switch (kind) {
    case "normal": return "card.normal" as const;
    case "snapshot": return "card.snapshot" as const;
    case "stale": return "card.staleKeep" as const;
    case "transient": return "card.network" as const;
    case "deterministic": return "card.deterministic" as const;
    case "invalid": return "card.invalid" as const;
    case "loading": return "card.querying" as const;
    default: return "card.noData" as const;
  }
}

function statusTone(kind: ReturnType<typeof deriveProviderCardState>["kind"]) {
  if (kind === "deterministic" || kind === "invalid") return "danger";
  if (kind === "stale" || kind === "transient") return "warning";
  if (kind === "normal" || kind === "snapshot") return "success";
  return "neutral";
}

function HoverPanelInner() {
  useProviderStateEvents();
  const qc = useQueryClient();
  const { lang, t } = useLang();
  const providers = useProviders();
  const settings = useSettings();
  const snapshots = useSnapshots();
  const nativeMetas = useNativeMetas();
  const [actionError, setActionError] = useState<string | null>(null);
  // 压缩布局：后端在垂直空间不足时把窗口缩到压缩高度，前端随之裁剪区块
  const [compact, setCompact] = useState(() => isCompactViewport(window.innerHeight));
  // 峰谷标签锚点：常驻面板无重渲染时以翻转事件驱动重算（#15）
  const peakTick = usePeakFlipTick();

  useEffect(() => {
    const onResize = () => setCompact(isCompactViewport(window.innerHeight));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    document.body.classList.add("qt-hover-body");
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") void api.hideHoverPanel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      document.body.classList.remove("qt-hover-body");
      window.removeEventListener("keydown", onKeyDown);
    };
  }, []);

  const enabledProviders = useMemo(
    () => (providers.data ?? []).filter((provider) => provider.enabled),
    [providers.data],
  );
  const entry = resolveHoverProvider(
    providers.data ?? [],
    settings.data?.tray_icon_entry_id ?? null,
  );
  const query = useProviderState(entry?.id ?? "__hover_empty__", Boolean(entry));
  const refreshProvider = useMutation({
    mutationFn: (id: string) => api.queryProvider(id),
    onError: (error) => setActionError(String(error)),
  });
  const view = deriveProviderCardState({
    enabled: Boolean(entry),
    outcome: query.data,
    snapshot: entry ? snapshots.data?.[entry.id] : undefined,
    isFetching: query.isFetching || refreshProvider.isPending,
  });
  const mainData = view.data[0];
  // 条目级主度量偏好（T-24）：hero/圆环/用量列表统一消费
  const primaryMetric = entry?.primary_metric;
  const primary = primaryValue(mainData, primaryMetric);
  // 多窗口时 hero 标签带窗口短标注（"剩余 5h"），单窗口保持通用文案
  const heroWindow = view.data.length > 1
    ? windowShortLabel(mainData?.plan_name, 0, lang)
    : null;
  const heroReset = resetCountdown(mainData?.reset_at);
  const ring = hoverRingView(mainData, settings.data?.ring_units_per_circle ?? 100, primaryMetric);
  const nativeProviderId = entry?.kind.type === "native" ? entry.kind.provider : undefined;
  const nativeMeta = nativeProviderId
    ? nativeMetas.data?.find((meta) => meta.id === nativeProviderId)
    : undefined;
  const platformName = entry ? kindLabel(entry.kind, nativeMeta?.name, lang) : "";
  const pricingView = entry
    ? resolveProviderPricingView(entry, nativeMeta, peakTick, mainData?.unit)
    : null;
  const modelChoices = pricingModelChoices(nativeMeta?.pricing ?? null, nativeMeta?.custom_models ?? []);
  const explicitModel = entry?.pricing?.model
    ? modelChoices.find((choice) => choice.modelId?.toLowerCase() === entry.pricing?.model?.toLowerCase())
    : undefined;
  const modelValue = entry?.pricing?.model
    ? explicitModel?.value ?? `model:${entry.pricing.model}`
    : "default";
  const hasDefault = modelChoices.some((choice) => choice.value === "default");

  const switchProvider = useMutation({
    // 图标源切换走后端 patch 合并（读现值→改单字段），不基于前端缓存
    // 全量提交——缓存陈旧时会把手未提交的设置整体抹回默认
    mutationFn: (id: string) => api.patchSettings({ tray_icon_entry_id: id }),
    onMutate: async (id) => {
      setActionError(null);
      await qc.cancelQueries({ queryKey: ["settings"] });
      const previous = qc.getQueryData<Settings>(["settings"]);
      if (previous) qc.setQueryData<Settings>(["settings"], { ...previous, tray_icon_entry_id: id });
      return { previous };
    },
    onError: (error, _id, context) => {
      if (context?.previous) qc.setQueryData(["settings"], context.previous);
      setActionError(String(error));
    },
    onSettled: () => void qc.invalidateQueries({ queryKey: ["settings"] }),
  });

  const switchModel = useMutation({
    mutationFn: async (choiceValue: string) => {
      if (!entry) return;
      const choice = modelChoices.find((item) => item.value === choiceValue);
      const modelId = choiceValue === "default"
        ? null
        : choice?.modelId ?? choiceValue.replace(/^model:/, "");
      await api.upsertProvider(withProviderModel(entry, modelId), null);
    },
    onMutate: async (choiceValue) => {
      setActionError(null);
      await qc.cancelQueries({ queryKey: ["providers"] });
      const previous = qc.getQueryData<ProviderEntry[]>(["providers"]);
      if (entry && previous) {
        const choice = modelChoices.find((item) => item.value === choiceValue);
        const modelId = choiceValue === "default"
          ? null
          : choice?.modelId ?? choiceValue.replace(/^model:/, "");
        qc.setQueryData<ProviderEntry[]>(
          ["providers"],
          previous.map((provider) => provider.id === entry.id ? withProviderModel(provider, modelId) : provider),
        );
      }
      return { previous };
    },
    onError: (error, _choice, context) => {
      if (context?.previous) qc.setQueryData(["providers"], context.previous);
      setActionError(String(error));
    },
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      if (entry) void qc.invalidateQueries({ queryKey: ["provider-state", entry.id] });
    },
  });

  const refresh = () => {
    setActionError(null);
    if (entry) refreshProvider.mutate(entry.id);
  };
  const visibleWindows = view.data.filter((item) => item.is_valid !== false).slice(0, 3);
  // T-22：阈值为剩余语义（默认 20），高亮方向为剩余 ≤ 阈值
  // （与后端 breach 同时机）；算不出剩余百分比视为永不触发。
  const overThreshold = view.data.some(
    (item) => (remainingPercent(item) ?? Infinity) <= (settings.data?.low_balance_remaining_percent ?? 20),
  );
  const renderedStatus = overThreshold ? t("settings.thresholdTitle") : t(statusKey(view.kind));
  const renderedTone = overThreshold ? "danger" : statusTone(view.kind);

  return (
    <div
      className="qt-hover-panel"
      onPointerEnter={() => void api.setHoverPanelPointerInside(true)}
      onPointerLeave={() => void api.setHoverPanelPointerInside(false)}
    >
      <header className="qt-hover-header">
        <div className="qt-hover-brand">
          <BrandMark className="qt-hover-mark" />
          <div>
            <strong>QuotaTray</strong>
            <small><i />{view.at ? relativeTime(view.at, lang) : t("card.neverSucceeded")}</small>
          </div>
        </div>
        <div className="qt-hover-actions">
          <button
            type="button"
            className={`qt-hover-icon-btn ${query.isFetching || refreshProvider.isPending ? "is-spinning" : ""}`}
            aria-label={t("card.refresh")}
            disabled={!entry || query.isFetching || refreshProvider.isPending}
            onClick={refresh}
          >
            <RefreshCw size={16} />
          </button>
          <button
            type="button"
            className="qt-hover-icon-btn"
            aria-label={t("hover.close")}
            onClick={() => void api.hideHoverPanel()}
          >
            <X size={16} />
          </button>
        </div>
      </header>

      {entry ? (
        <>
          <div className="qt-hover-selectors">
            <label>
              <span>{t("hover.account")}</span>
              <select
                value={entry.id}
                disabled={switchProvider.isPending}
                onChange={(event) => switchProvider.mutate(event.target.value)}
              >
                {enabledProviders.map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
              </select>
            </label>
            <label>
              <span>{t("hover.model")}</span>
              <select
                value={modelValue}
                disabled={switchModel.isPending || modelChoices.length === 0}
                onChange={(event) => switchModel.mutate(event.target.value)}
              >
                {!explicitModel && entry.pricing?.model && (
                  <option value={`model:${entry.pricing.model}`}>{entry.pricing.model}</option>
                )}
                {!hasDefault && <option value="default">{t("pricing.noModel")}</option>}
                {modelChoices.map((choice) => (
                  <option key={choice.value} value={choice.value}>
                    {choice.label}{choice.source === "custom" ? ` · ${t("pricing.libraryModel")}` : ""}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <main className="qt-hover-content">
            <section className="qt-hover-hero">
              <div>
                <span>{primary.label === "available" ? t("hover.availableBalance") : primary.label === "remaining" ? (heroWindow ? (lang === "zh" ? `剩余 ${heroWindow}` : `Left ${heroWindow}`) : t("hover.remainingQuota")) : t("card.noData")}</span>
                <strong>{primary.unit && <small>{primary.unit}</small>}{primary.value}</strong>
                {heroReset && <small className="qt-hover-reset" data-tooltip={t("card.resetIn", { time: heroReset })}>{heroReset}</small>}
              </div>
              <div className={`qt-hover-ring ${overThreshold ? "is-alert" : ""}`} aria-label={renderedStatus}>
                <svg viewBox="0 0 48 48" aria-hidden="true">
                  <circle cx="24" cy="24" r="18" pathLength="100" />
                  {ring && (
                    <circle
                      className="qt-hover-ring-value"
                      cx="24"
                      cy="24"
                      r="18"
                      pathLength="100"
                      strokeDasharray={`${ring.fillPercent} 100`}
                    />
                  )}
                </svg>
                <span>{ring?.center ?? "—"}</span>
              </div>
            </section>

            {!compact && (
              <section className="qt-hover-status-row">
                <span className={`qt-hover-status is-${renderedTone}`}><i />{renderedStatus}</span>
                <span>{platformName}{pricingView?.modelLabel ? ` · ${pricingView.modelLabel}` : ""}</span>
              </section>
            )}

            {!compact && visibleWindows.some((item) => remainingPercent(item) != null) && (
              <section className="qt-hover-usage-list">
                {visibleWindows.map((item, index) => {
                  // 进度条按剩余比例填充（PR #146 review）：与同行文案
                  // 「剩余 N%」、圆环三口径一致——剩余越多填充越多，
                  // 不再按已用填充（文案与视觉方向相反即 spec Problem 原句）。
                  const percent = remainingPercent(item);
                  const reset = resetCountdown(item.reset_at);
                  return (
                    <div className="qt-hover-usage" key={`${item.plan_name ?? "window"}-${index}`}>
                      <div>
                        <span>{item.plan_name ?? t("card.windowN", { n: index + 1 })}</span>
                        <b>{dataSummary(item, lang, primaryMetric)}</b>
                        {reset && <small className="qt-hover-usage-reset">{reset}</small>}
                      </div>
                      {percent != null && (
                        <div className="qt-hover-progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(percent)}>
                          <span style={{ width: `${Math.max(0, Math.min(100, percent))}%` }} />
                        </div>
                      )}
                    </div>
                  );
                })}
              </section>
            )}

            {!compact && pricingView && (
              <section className="qt-hover-pricing">
                <div className="qt-hover-pricing-head">
                  <span className={`qt-period-text ${pricingView.period === "peak" ? "is-peak" : "is-offpeak"}`}>
                    <i className={pricingView.period === "peak" ? "is-peak" : "is-offpeak"} />
                    {pricingView.plan === "subscription"
                      ? pricingView.period === "peak" ? t("card.subscriptionPeak") : t("card.subscriptionOffPeak")
                      : pricingView.period === "peak" ? t("card.periodPeak") : t("card.periodOffPeak")}
                  </span>
                </div>
                {pricingView.plan === "subscription" ? (
                  <p className="qt-hover-subscription">{t("pricing.subscriptionHint")}</p>
                ) : (
                  <>
                    <dl>
                      <div><dt>{t("pricing.hit")}</dt><dd>{formatPrice(pricingView.tier?.cache_hit_input)}</dd></div>
                      <div><dt>{t("pricing.miss")}</dt><dd>{formatPrice(pricingView.tier?.cache_miss_input)}</dd></div>
                      <div><dt>{t("pricing.out")}</dt><dd>{formatPrice(pricingView.tier?.output)}</dd></div>
                    </dl>
                    <p>{pricingView.currency ?? "—"} / {t("pricing.unitShort")}</p>
                  </>
                )}
              </section>
            )}

            {(view.errorMessage || actionError) && (
              <p className="qt-hover-error">{actionError ?? view.errorMessage}</p>
            )}
          </main>
        </>
      ) : (
        <main className="qt-hover-empty">
          <strong>{t("hover.noEnabled")}</strong>
          <span>{t("app.emptyHint")}</span>
        </main>
      )}

      {!compact && (
        <footer className="qt-hover-footer">
          <span>{entry?.name ?? "QuotaTray"}</span>
          <button type="button" onClick={() => void api.openMainWindow()}>
            <ExternalLink size={14} />{t("hover.openMain")}
          </button>
        </footer>
      )}
    </div>
  );
}

export default function HoverPanel() {
  return (
    <QueryClientProvider client={hoverQueryClient}>
      <LangProvider>
        <ThemeProvider>
          <HoverPanelInner />
        </ThemeProvider>
      </LangProvider>
    </QueryClientProvider>
  );
}
