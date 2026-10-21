import { QueryClient, QueryClientProvider, useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, ExternalLink, RefreshCw, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import {
  dataSummary,
  kindLabel,
  relativeTime,
  remainingPercent,
  resetCountdown,
  windowShortLabel,
} from "../display";
import { LangProvider, useLang } from "../i18n";
import {
  useHoverPanelShown,
  useNativeMetas,
  usePeakFlipTick,
  useProviderStateEvents,
  useProviderStates,
  useProviders,
  useRefreshNow,
  useSettings,
  useSnapshots,
} from "../queries";
import { ThemeProvider } from "../theme";
import type { NativeMeta, ProviderEntry, Settings, UsageData } from "../types";
import {
  hoverPanelMode,
  hoverRingView,
  hoverRows,
  hoverTargetHintKey,
  isCompactViewport,
  resolveHoverProvider,
  type HoverRow,
} from "./hoverPanelView";
import { BrandMark } from "./BrandMark";
import {
  deriveProviderCardState,
  primaryValue,
  type ProviderCardKind,
} from "./providerCardView";
import {
  isLightLogo,
  providerIconUrl,
  providerInitials,
  templateProviderIconUrl,
} from "./providerIcon";
import {
  pricingModelChoices,
  resolveProviderPricingView,
  withProviderModel,
  type PricingModelChoice,
  type ProviderPricingView,
} from "./providerPricing";
import { formatPrice } from "./pricingDraft";
import { Badge } from "./ui";

const hoverQueryClient = new QueryClient();

function statusKey(kind: ProviderCardKind) {
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

function statusTone(kind: ProviderCardKind) {
  if (kind === "deterministic" || kind === "invalid") return "danger";
  if (kind === "stale" || kind === "transient") return "warning";
  if (kind === "normal" || kind === "snapshot") return "success";
  return "neutral";
}

/** 状态徽章（与主页卡片同款映射）。 */
function StatusBadge({ kind }: { kind: ProviderCardKind }) {
  const { t } = useLang();
  switch (kind) {
    case "normal":
      return <Badge tone="success" dot>{t("card.normal")}</Badge>;
    case "snapshot":
      return <Badge tone="neutral" dot>{t("card.snapshot")}</Badge>;
    case "stale":
      return <Badge tone="warning" dot>{t("card.staleKeep")}</Badge>;
    case "transient":
      return <Badge tone="warning" dot>{t("card.network")}</Badge>;
    case "deterministic":
      return <Badge tone="danger" dot>{t("card.deterministic")}</Badge>;
    case "invalid":
      return <Badge tone="danger" dot>{t("card.invalid")}</Badge>;
    case "loading":
      return <Badge tone="neutral" dot>{t("card.querying")}</Badge>;
    default:
      return <Badge tone="neutral">{t("card.noData")}</Badge>;
  }
}

/** 模型下拉当前值：显式模型命中目录则用目录值，否则保留自定义 model: 形态。 */
function modelSelectValue(entry: ProviderEntry, choices: PricingModelChoice[]): string {
  if (!entry.pricing?.model) return "default";
  const explicit = choices.find(
    (choice) => choice.modelId?.toLowerCase() === entry.pricing?.model?.toLowerCase(),
  );
  return explicit?.value ?? `model:${entry.pricing.model}`;
}

function choiceModelId(choiceValue: string, choices: PricingModelChoice[]): string | null {
  if (choiceValue === "default") return null;
  const choice = choices.find((item) => item.value === choiceValue);
  return choice?.modelId ?? choiceValue.replace(/^model:/, "");
}

/** 峰谷价卡：单账户视图与详情抽屉共用。 */
function PricingBlock({ view }: { view: ProviderPricingView }) {
  const { t } = useLang();
  return (
    <section className="qt-hover-pricing">
      <div className="qt-hover-pricing-head">
        <span className={`qt-period-text ${view.period === "peak" ? "is-peak" : "is-offpeak"}`}>
          <i className={view.period === "peak" ? "is-peak" : "is-offpeak"} />
          {view.plan === "subscription"
            ? view.period === "peak" ? t("card.subscriptionPeak") : t("card.subscriptionOffPeak")
            : view.period === "peak" ? t("card.periodPeak") : t("card.periodOffPeak")}
        </span>
      </div>
      {view.plan === "subscription" ? (
        <p className="qt-hover-subscription">{t("pricing.subscriptionHint")}</p>
      ) : (
        <>
          <dl>
            <div><dt>{t("pricing.hit")}</dt><dd>{formatPrice(view.tier?.cache_hit_input)}</dd></div>
            <div><dt>{t("pricing.miss")}</dt><dd>{formatPrice(view.tier?.cache_miss_input)}</dd></div>
            <div><dt>{t("pricing.out")}</dt><dd>{formatPrice(view.tier?.output)}</dd></div>
          </dl>
          <p>{view.currency ?? "—"} / {t("pricing.unitShort")}</p>
        </>
      )}
    </section>
  );
}

/** 额度窗口列表（单账户视图与详情抽屉共用）：进度条按剩余比例填充，
 *  与同行「剩余 N%」、圆环三口径一致。 */
function UsageWindows({
  data,
  metric,
}: {
  data: UsageData[];
  metric: ProviderEntry["primary_metric"];
}) {
  const { lang, t } = useLang();
  return (
    <section className="qt-hover-usage-list">
      {data.map((item, index) => {
        const percent = remainingPercent(item);
        const reset = resetCountdown(item.reset_at);
        return (
          <div className="qt-hover-usage" key={`${item.plan_name ?? "window"}-${index}`}>
            <div>
              <span>{item.plan_name ?? t("card.windowN", { n: index + 1 })}</span>
              <b>{dataSummary(item, lang, metric)}</b>
              {reset && <small className="qt-hover-usage-reset">{reset}</small>}
            </div>
            {percent != null && (
              <div
                className="qt-hover-progress"
                role="progressbar"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(percent)}
              >
                <span style={{ width: `${Math.max(0, Math.min(100, percent))}%` }} />
              </div>
            )}
          </div>
        );
      })}
    </section>
  );
}

/** 账户身份图标：预置平台用官方品牌图，模板/脚本按名称匹配或首字母。 */
function AccountAvatar({
  entry,
  platformName,
  ringFill,
  isTarget,
}: {
  entry: ProviderEntry;
  platformName: string;
  ringFill: number | null;
  isTarget: boolean;
}) {
  const nativeId = entry.kind.type === "native" ? entry.kind.provider : null;
  const iconUrl = nativeId ? providerIconUrl(nativeId) : templateProviderIconUrl(entry.name);
  const lightLogo = nativeId ? isLightLogo(nativeId) : false;
  return (
    <span className={`qt-hover-avatar${isTarget ? " is-target" : ""}`}>
      {isTarget && ringFill != null && (
        <svg className="qt-hover-avatar-ring" viewBox="0 0 38 38" aria-hidden="true">
          <rect className="qt-hover-avatar-track" x="2" y="2" width="34" height="34" rx="11" pathLength="100" />
          <rect
            className="qt-hover-avatar-value"
            x="2"
            y="2"
            width="34"
            height="34"
            rx="11"
            pathLength="100"
            strokeDasharray={`${Math.max(0, Math.min(100, ringFill))} 100`}
          />
        </svg>
      )}
      <span className={`qt-hover-avatar-inner${lightLogo ? " is-light-logo" : ""}`}>
        {iconUrl ? (
          <img src={iconUrl} alt="" aria-hidden="true" draggable={false} />
        ) : (
          providerInitials(platformName)
        )}
      </span>
    </span>
  );
}

/** 显示目标环：圈内 on/off、圈外该账户真实进度；off 态整环灰。 */
function TargetRing({
  row,
  busy,
  onSet,
}: {
  row: HoverRow;
  busy: boolean;
  onSet: () => void;
}) {
  const { t } = useLang();
  const hint = t(hoverTargetHintKey(row.isTarget));
  return (
    <button
      type="button"
      className={`qt-hover-target-ring${row.isTarget ? " is-on" : ""}`}
      data-tooltip={hint}
      aria-label={hint}
      aria-pressed={row.isTarget}
      disabled={busy}
      onClick={() => {
        if (row.isTarget) return;
        onSet();
      }}
    >
      <svg viewBox="0 0 36 36" aria-hidden="true">
        <circle className="qt-hover-target-track" cx="18" cy="18" r="15" pathLength="100" />
        <circle
          className="qt-hover-target-value"
          cx="18"
          cy="18"
          r="15"
          pathLength="100"
          strokeDasharray={`${Math.max(0, Math.min(100, row.ringFill ?? 0))} 100`}
        />
      </svg>
      <span>{row.isTarget ? "on" : "off"}</span>
    </button>
  );
}

function HoverPanelInner() {
  useProviderStateEvents();
  useRefreshNow();
  const qc = useQueryClient();
  const { lang, t } = useLang();
  const providers = useProviders();
  const settings = useSettings();
  const snapshots = useSnapshots();
  const states = useProviderStates();
  const nativeMetas = useNativeMetas();
  const [actionError, setActionError] = useState<string | null>(null);
  // 详情抽屉当前账户；面板每次被唤起时清空（useHoverPanelShown）
  const [openId, setOpenId] = useState<string | null>(null);
  // 压缩布局：后端在垂直空间不足时把窗口缩到压缩高度，前端随之裁剪区块
  const [compact, setCompact] = useState(() => isCompactViewport(window.innerHeight));
  // 峰谷标签锚点：常驻面板无重渲染时以翻转事件驱动重算（#15）
  const peakTick = usePeakFlipTick();

  useHoverPanelShown(() => {
    setOpenId(null);
    // 面板隐藏期间事件不推送，重新唤起时补一次整表状态
    void qc.invalidateQueries({ queryKey: ["provider-states"] });
  });

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

  const mode = hoverPanelMode(settings.data?.hover_panel_mode);
  const enabledProviders = useMemo(
    () => (providers.data ?? []).filter((provider) => provider.enabled),
    [providers.data],
  );
  const rows = useMemo(
    () =>
      hoverRows({
        providers: providers.data ?? [],
        states: states.data,
        snapshots: snapshots.data,
        settings: settings.data,
        lang,
      }),
    [providers.data, states.data, snapshots.data, settings.data, lang],
  );
  const selectedRow = openId ? rows.find((row) => row.id === openId) ?? null : null;

  // 单账户模式沿用升级前的语义：条目 = 托盘显示目标
  const entry = resolveHoverProvider(
    providers.data ?? [],
    settings.data?.tray_icon_entry_id ?? null,
  );
  const view = deriveProviderCardState({
    enabled: Boolean(entry),
    outcome: entry ? states.data?.[entry.id] : undefined,
    snapshot: entry ? snapshots.data?.[entry.id] : undefined,
  });

  const refreshOne = useMutation({
    mutationFn: (id: string) => api.queryProvider(id),
    onError: (error) => setActionError(String(error)),
  });
  // 头部刷新（多条目模式）：串行刷新全部启用条目，避免并发打满平台限流
  const refreshAll = useMutation({
    mutationFn: async (ids: string[]) => {
      for (const id of ids) {
        await api.queryProvider(id);
      }
    },
    onError: (error) => setActionError(String(error)),
  });

  const switchTarget = useMutation({
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
    mutationFn: async ({
      entry: target,
      choiceValue,
      choices,
    }: {
      entry: ProviderEntry;
      choiceValue: string;
      choices: PricingModelChoice[];
    }) => {
      await api.upsertProvider(withProviderModel(target, choiceModelId(choiceValue, choices)), null);
    },
    onMutate: async ({ entry: target, choiceValue, choices }) => {
      setActionError(null);
      await qc.cancelQueries({ queryKey: ["providers"] });
      const previous = qc.getQueryData<ProviderEntry[]>(["providers"]);
      const modelId = choiceModelId(choiceValue, choices);
      if (previous) {
        qc.setQueryData<ProviderEntry[]>(
          ["providers"],
          previous.map((provider) =>
            provider.id === target.id ? withProviderModel(provider, modelId) : provider,
          ),
        );
      }
      return { previous };
    },
    onError: (error, _vars, context) => {
      if (context?.previous) qc.setQueryData(["providers"], context.previous);
      setActionError(String(error));
    },
    onSettled: (_data, _error, vars) => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      void qc.invalidateQueries({ queryKey: ["provider-state", vars.entry.id] });
      void qc.invalidateQueries({ queryKey: ["provider-states"] });
    },
  });

  const refreshBusy = refreshAll.isPending || refreshOne.isPending;
  const refresh = () => {
    setActionError(null);
    if (mode === "multi") {
      refreshAll.mutate(enabledProviders.map((provider) => provider.id));
    } else if (entry) {
      refreshOne.mutate(entry.id);
    }
  };

  // 抽屉的派生视图：与单账户视图同一套定价/模型/圆环逻辑
  const drawer = useMemo(() => {
    if (!selectedRow) return null;
    const target = selectedRow.entry;
    const drawerNativeId = target.kind.type === "native" ? target.kind.provider : undefined;
    const drawerMeta: NativeMeta | undefined = drawerNativeId
      ? nativeMetas.data?.find((meta) => meta.id === drawerNativeId)
      : undefined;
    const platformName = kindLabel(target.kind, drawerMeta?.name, lang);
    const pricingView = resolveProviderPricingView(
      target,
      drawerMeta,
      peakTick,
      selectedRow.data[0]?.unit,
    );
    const choices = pricingModelChoices(drawerMeta?.pricing ?? null, drawerMeta?.custom_models ?? []);
    const heroData = selectedRow.data[0];
    const heroWindow = selectedRow.data.length > 1
      ? windowShortLabel(heroData?.plan_name, 0, lang)
      : undefined;
    return {
      entry: target,
      nativeMeta: drawerMeta,
      platformName,
      pricingView,
      choices,
      hero: primaryValue(heroData, lang, heroWindow, target.primary_metric),
      heroReset: resetCountdown(heroData?.reset_at),
      ring: hoverRingView(
        heroData,
        settings.data?.ring_units_per_circle ?? 100,
        target.primary_metric,
      ),
      modelValue: modelSelectValue(target, choices),
      hasDefault: choices.some((choice) => choice.value === "default"),
      explicitModel: target.pricing?.model
        ? choices.find(
            (choice) => choice.modelId?.toLowerCase() === target.pricing?.model?.toLowerCase(),
          )
        : undefined,
    };
  }, [selectedRow, nativeMetas.data, peakTick, lang, settings.data]);

  // ---- 单账户视图派生（与升级前一致） ----
  const mainData = view.data[0];
  const primaryMetric = entry?.primary_metric;
  const heroWindow = view.data.length > 1
    ? windowShortLabel(mainData?.plan_name, 0, lang)
    : undefined;
  const primary = primaryValue(mainData, lang, heroWindow, primaryMetric);
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
    ? modelChoices.find(
        (choice) => choice.modelId?.toLowerCase() === entry.pricing?.model?.toLowerCase(),
      )
    : undefined;
  const modelValue = entry ? modelSelectValue(entry, modelChoices) : "default";
  const hasDefault = modelChoices.some((choice) => choice.value === "default");

  // 多条目模式取各行的最近成功时刻；单账户模式沿用该条目时刻
  const headerAt = mode === "multi"
    ? rows.reduce<number | null>((latest, row) => (row.at != null && (latest == null || row.at > latest) ? row.at : latest), null)
    : view.at;

  const visibleWindows = view.data.filter((item) => item.is_valid !== false).slice(0, 3);
  // T-22：阈值为剩余语义（默认 20），高亮方向为剩余 ≤ 阈值
  // （与后端 breach 同时机）；算不出剩余百分比视为永不触发。
  const overThreshold = view.data.some(
    (item) => (remainingPercent(item) ?? Infinity) <= (settings.data?.low_balance_remaining_percent ?? 20),
  );
  const renderedStatus = overThreshold ? t("settings.thresholdTitle") : t(statusKey(view.kind));
  const renderedTone = overThreshold ? "danger" : statusTone(view.kind);

  const emptyState = (
    <main className="qt-hover-empty">
      <strong>{t("hover.noEnabled")}</strong>
      <span>{t("app.emptyHint")}</span>
    </main>
  );

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
            <small><i />{headerAt ? relativeTime(headerAt, lang) : t("card.neverSucceeded")}</small>
          </div>
        </div>
        <div className="qt-hover-actions">
          <button
            type="button"
            className={`qt-hover-icon-btn ${refreshBusy ? "is-spinning" : ""}`}
            aria-label={t("card.refresh")}
            disabled={refreshBusy || (mode === "multi" ? enabledProviders.length === 0 : !entry)}
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

      {mode === "multi" ? (
        rows.length === 0 ? (
          emptyState
        ) : (
          <div className="qt-hover-body-view">
            <div className="qt-hover-list" role="listbox" aria-label={t("hover.accounts")}>
              {rows.map((row) => {
                const rowNativeId = row.entry.kind.type === "native" ? row.entry.kind.provider : undefined;
                const rowMeta = rowNativeId
                  ? nativeMetas.data?.find((meta) => meta.id === rowNativeId)
                  : undefined;
                const rowPlatform = kindLabel(row.entry.kind, rowMeta?.name, lang);
                const rowModel = resolveProviderPricingView(
                  row.entry,
                  rowMeta,
                  peakTick,
                  row.data[0]?.unit,
                )?.modelLabel;
                const subtitle = [rowPlatform, rowModel].filter(Boolean).join(" · ");
                return (
                  <button
                    key={row.id}
                    type="button"
                    role="option"
                    aria-selected={row.isTarget}
                    className={`qt-hover-row${row.isTarget ? " is-target" : ""}`}
                    onClick={() => setOpenId(row.id)}
                  >
                    <AccountAvatar
                      entry={row.entry}
                      platformName={rowPlatform}
                      ringFill={row.ringFill}
                      isTarget={row.isTarget}
                    />
                    <span className="qt-hover-row-id">
                      <span className="qt-hover-row-name">
                        <span className="qt-hover-row-title">{row.entry.name}</span>
                        <StatusBadge kind={row.kind} />
                      </span>
                      <span className="qt-hover-row-sub" data-tooltip={subtitle}>{subtitle}</span>
                    </span>
                    <span className="qt-hover-row-metrics">
                      {row.metrics.length === 0 ? (
                        <span className="qt-hover-metric is-empty">{t("card.noData")}</span>
                      ) : (
                        row.metrics.map((metric) => (
                          <span className="qt-hover-metric" key={metric.key}>
                            <em>{metric.label}</em>
                            <b className={metric.alert ? "is-alert" : ""}>
                              {metric.unit && <small>{metric.unit}</small>}
                              {metric.value}
                            </b>
                            {metric.reset && <small>{metric.reset}</small>}
                          </span>
                        ))
                      )}
                    </span>
                  </button>
                );
              })}
            </div>

            {selectedRow && drawer && (
              <aside className="qt-hover-drawer is-open" aria-label={t("hover.detail")}>
                <div className="qt-hover-drawer-head">
                  <button
                    type="button"
                    className="qt-hover-icon-btn"
                    aria-label={t("hover.back")}
                    onClick={() => setOpenId(null)}
                  >
                    <ChevronLeft size={16} />
                  </button>
                  <span className="qt-hover-drawer-title">{t("hover.detail")}</span>
                  <button
                    type="button"
                    className={`qt-hover-icon-btn ${refreshOne.isPending ? "is-spinning" : ""}`}
                    aria-label={t("card.refresh")}
                    disabled={refreshOne.isPending}
                    onClick={() => refreshOne.mutate(selectedRow.id)}
                  >
                    <RefreshCw size={16} />
                  </button>
                </div>
                <div className="qt-hover-drawer-body">
                  <div className="qt-hover-detail">
                    <div className="qt-hover-detail-head">
                      <AccountAvatar
                        entry={selectedRow.entry}
                        platformName={drawer.platformName}
                        ringFill={null}
                        isTarget={false}
                      />
                      <div className="qt-hover-detail-id">
                        <strong>{selectedRow.entry.name}</strong>
                        <small>
                          <i className={`qt-hover-status-dot is-${statusTone(selectedRow.kind)}`} />
                          <span className="qt-hover-detail-sub" data-tooltip={`${t(statusKey(selectedRow.kind))} · ${drawer.platformName}${drawer.pricingView?.modelLabel ? ` · ${drawer.pricingView.modelLabel}` : ""}`}>
                            {t(statusKey(selectedRow.kind))} · {drawer.platformName}
                            {drawer.pricingView?.modelLabel ? ` · ${drawer.pricingView.modelLabel}` : ""}
                          </span>
                        </small>
                      </div>
                      <TargetRing
                        row={selectedRow}
                        busy={switchTarget.isPending}
                        onSet={() => switchTarget.mutate(selectedRow.id)}
                      />
                    </div>

                    <label className="qt-hover-model-row">
                      <span>{t("hover.model")}</span>
                      <select
                        className="qt-hover-model-select"
                        value={drawer.modelValue}
                        disabled={switchModel.isPending || drawer.choices.length === 0}
                        onChange={(event) =>
                          switchModel.mutate({
                            entry: selectedRow.entry,
                            choiceValue: event.target.value,
                            choices: drawer.choices,
                          })
                        }
                      >
                        {!drawer.explicitModel && selectedRow.entry.pricing?.model && (
                          <option value={`model:${selectedRow.entry.pricing.model}`}>
                            {selectedRow.entry.pricing.model}
                          </option>
                        )}
                        {!drawer.hasDefault && <option value="default">{t("pricing.noModel")}</option>}
                        {drawer.choices.map((choice) => (
                          <option key={choice.value} value={choice.value}>
                            {choice.label}
                            {choice.source === "custom" ? ` · ${t("pricing.libraryModel")}` : ""}
                          </option>
                        ))}
                      </select>
                    </label>

                    <section className="qt-hover-hero">
                      <div>
                        <span>{drawer.hero.label}</span>
                        <strong>
                          {drawer.hero.unit && <small>{drawer.hero.unit}</small>}
                          {drawer.hero.value}
                        </strong>
                        {drawer.heroReset && (
                          <small className="qt-hover-reset" data-tooltip={t("card.resetIn", { time: drawer.heroReset })}>
                            {drawer.heroReset}
                          </small>
                        )}
                      </div>
                      <div className={`qt-hover-ring ${selectedRow.metrics.some((metric) => metric.alert) ? "is-alert" : ""}`}>
                        <svg viewBox="0 0 48 48" aria-hidden="true">
                          <circle cx="24" cy="24" r="18" pathLength="100" />
                          {drawer.ring && (
                            <circle
                              className="qt-hover-ring-value"
                              cx="24"
                              cy="24"
                              r="18"
                              pathLength="100"
                              strokeDasharray={`${drawer.ring.fillPercent} 100`}
                            />
                          )}
                        </svg>
                        <span>{drawer.ring?.center ?? "—"}</span>
                      </div>
                    </section>

                    <UsageWindows data={selectedRow.data} metric={selectedRow.entry.primary_metric} />

                    {drawer.pricingView && <PricingBlock view={drawer.pricingView} />}

                    {(selectedRow.errorMessage || actionError) && (
                      <p className="qt-hover-error">{actionError ?? selectedRow.errorMessage}</p>
                    )}
                  </div>
                </div>
              </aside>
            )}
          </div>
        )
      ) : entry ? (
        <>
          <div className="qt-hover-selectors">
            <label>
              <span>{t("hover.account")}</span>
              <select
                value={entry.id}
                disabled={switchTarget.isPending}
                onChange={(event) => switchTarget.mutate(event.target.value)}
              >
                {enabledProviders.map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
              </select>
            </label>
            <label>
              <span>{t("hover.model")}</span>
              <select
                value={modelValue}
                disabled={switchModel.isPending || modelChoices.length === 0}
                onChange={(event) =>
                  switchModel.mutate({
                    entry,
                    choiceValue: event.target.value,
                    choices: modelChoices,
                  })
                }
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
                <span>{primary.label}</span>
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
              <UsageWindows data={visibleWindows} metric={primaryMetric} />
            )}

            {!compact && pricingView && <PricingBlock view={pricingView} />}

            {(view.errorMessage || actionError) && (
              <p className="qt-hover-error">{actionError ?? view.errorMessage}</p>
            )}
          </main>
        </>
      ) : (
        emptyState
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
