import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  ClipboardCopy,
  Clock3,
  Ellipsis,
  Globe,
  KeyRound,
  Pause,
  Pencil,
  Play,
  RefreshCw,
  Trash2,
  WifiOff,
} from "lucide-react";
import { memo, useEffect, useState } from "react";
import { api } from "../api";
import {
  amountText,
  exactTime,
  kindLabel,
  relativeTime,
  resetCountdown,
  usedPercent,
  windowShortLabel,
} from "../display";
import { useLang } from "../i18n";
import { usePeakFlipTick, useProviderQuery } from "../queries";
import type { DragHandleProps } from "../useCardDragSort";
import type { NativeMeta, ProviderEntry, SnapshotEntry, UsageData } from "../types";
import { canCopyError, deriveProviderCardState, errorCopyText } from "./providerCardView";
import { isLightLogo, providerIconUrl, templateProviderIconUrl } from "./providerIcon";
import {
  pricingModelChoices,
  resolveProviderPricingView,
  withProviderModel,
} from "./providerPricing";
import { formatPrice } from "./pricingDraft";
import { Badge, Button, ConfirmDialog, DropdownMenu, IconButton, MenuItem, Tooltip } from "./ui";

interface Props {
  entry: ProviderEntry;
  intervalMinutes: number;
  thresholdPercent: number;
  snapshot?: SnapshotEntry;
  nativeMeta?: NativeMeta;
  onEdit: (entry: ProviderEntry, usageCurrency?: string) => void;
  /** 拖拽把手事件（列表级排序状态机下发；缺省则不渲染把手）。 */
  dragHandleProps?: DragHandleProps;
  /** 让位偏移（px）：拖拽会话期间由父级下发，undefined = 常态无位移。 */
  dragShift?: number;
  /** 本卡片是拖拽源（跟手/落位中）：视觉浮起强化。 */
  isDragSource?: boolean;
}

/** 主数值区取值：百分比优先，否则剩余额度。多窗口时 label 带窗口短标签。 */
function primaryValue(data: UsageData | undefined, lang: "zh" | "en", windowLabel?: string) {
  if (!data) return { value: "—", unit: "", label: lang === "zh" ? "暂无数据" : "No data" };
  const zh = lang === "zh";
  const percent = usedPercent(data);
  if (percent != null) {
    return {
      value: `${Math.round(percent)}%`,
      unit: "",
      label: windowLabel
        ? zh
          ? `已用 ${windowLabel}`
          : `Used ${windowLabel}`
        : zh
          ? "已用额度"
          : "Used",
    };
  }
  if (data.remaining != null) {
    return {
      value: amountText(data.remaining),
      unit: data.unit ?? "",
      label: windowLabel ?? (zh ? "可用余额" : "Available"),
    };
  }
  return { value: "—", unit: data.unit ?? "", label: zh ? "已获取" : "Fetched" };
}

function providerInitials(name: string) {
  const capitals = name.match(/[A-Z]/g);
  if (capitals && capitals.length >= 2) return capitals.slice(0, 2).join("");

  const words = name.trim().split(/\s+/).filter(Boolean);
  if (words.length >= 2) return words.slice(0, 2).map((word) => word[0]).join("").toUpperCase();
  return name.slice(0, 2).toUpperCase();
}

/** memo：拖拽让位重渲染只触达位移变化的卡片——依赖调用方保持
 *  onEdit/dragHandleProps 等引用稳定（App useCallback + hook 内缓存）。 */
export const ProviderCard = memo(function ProviderCard({
  entry,
  intervalMinutes,
  thresholdPercent,
  snapshot,
  nativeMeta,
  onEdit,
  dragHandleProps,
  dragShift,
  isDragSource,
}: Props) {
  const qc = useQueryClient();
  const { t, lang } = useLang();
  const query = useProviderQuery(entry.id, entry.enabled, intervalMinutes);
  // 峰谷标签锚点：主窗开着跨过翻转点时以翻转事件驱动重算（#15）
  const peakTick = usePeakFlipTick();
  const [expanded, setExpanded] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  useEffect(() => {
    if (!feedback) return;
    const timer = window.setTimeout(() => setFeedback(null), 4_000);
    return () => window.clearTimeout(timer);
  }, [feedback]);
  const view = deriveProviderCardState({
    enabled: entry.enabled,
    outcome: query.data,
    snapshot,
    isFetching: query.isFetching,
  });
  const configured = Boolean(entry.api_key_enc);
  // CLI 凭据型（订阅四家）：凭据来自本机官方 CLI，不存在「配置 key」概念
  const cliCredential =
    entry.kind.type === "native" && nativeMeta?.uses_cli_credentials === true;
  const mainData = view.data[0];
  const multiWindow = view.data.length > 1;
  const primary = primaryValue(mainData, lang);
  const mainReset = resetCountdown(mainData?.reset_at);
  const pricingView = resolveProviderPricingView(entry, nativeMeta, peakTick, mainData?.unit);
  const modelChoices = pricingModelChoices(
    nativeMeta?.pricing ?? null,
    nativeMeta?.custom_models ?? [],
  );
  const platformName = kindLabel(entry.kind, nativeMeta?.name, lang);
  const platformIconUrl =
    entry.kind.type === "native"
      ? providerIconUrl(entry.kind.provider)
      : templateProviderIconUrl(entry.name);
  // 浅色品牌图（StepFun 纯白）需深底变体；无图标走首字母回退时不涉及
  const platformLightLogo =
    entry.kind.type === "native" && isLightLogo(entry.kind.provider);
  const explicitModelChoice = entry.pricing?.model
    ? modelChoices.find(
        (choice) => choice.modelId?.toLowerCase() === entry.pricing?.model?.toLowerCase(),
      )
    : undefined;
  const modelSelectValue = entry.pricing?.model
    ? explicitModelChoice?.value ?? `model:${entry.pricing.model}`
    : "default";
  const optionText = (choice: (typeof modelChoices)[number]) =>
    `${platformName} · ${choice.label}` +
    (choice.value === "default" ? t("pricing.presetDefault") : "") +
    (choice.source === "custom" ? ` · ${t("pricing.libraryModel")}` : "") +
    (choice.plan === "subscription" ? ` · ${t("pricing.subscriptionShort")}` : "");
  // 收起态 select 只显示截断文字，悬停以选中项全文作 title
  const selectedTitle = explicitModelChoice
    ? optionText(explicitModelChoice)
    : (entry.pricing?.model ?? undefined);
  const hasImplicitDefaultChoice = modelChoices.some((choice) => choice.value === "default");
  const showModelSelect = modelChoices.length > (hasImplicitDefaultChoice ? 1 : 0);
  const thresholdStates = view.data.map(
    (data) => (usedPercent(data) ?? -1) >= thresholdPercent,
  );
  const overThreshold = thresholdStates[0] ?? false;
  const anyOverThreshold = thresholdStates.some(Boolean);

  const invalidate = () => {
    setFeedback(null);
    void qc.invalidateQueries({ queryKey: ["provider", entry.id] });
  };

  const toggleEnabled = useMutation({
    mutationFn: () => api.upsertProvider({ ...entry, enabled: !entry.enabled }, null),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      setFeedback(entry.enabled ? t("card.disabled") : t("card.enable"));
    },
    onError: (error) => setFeedback(String(error)),
  });

  // 条目级查询代理开关（chatgpt.com 等被墙站点用；端口在设置中统一配置）
  const toggleProxy = useMutation({
    mutationFn: () => api.upsertProvider({ ...entry, use_proxy: !entry.use_proxy }, null),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      void qc.invalidateQueries({ queryKey: ["provider", entry.id] });
      setFeedback(entry.use_proxy ? t("card.proxyOffFeedback") : t("card.proxyOnFeedback"));
    },
    onError: (error) => setFeedback(String(error)),
  });

  const switchModel = useMutation({
    mutationFn: (choiceValue: string) => {
      const choice = modelChoices.find((item) => item.value === choiceValue);
      const modelId = choiceValue === "default"
        ? null
        : choice?.modelId ?? choiceValue.replace(/^model:/, "");
      return api.upsertProvider(withProviderModel(entry, modelId), null);
    },
    onSuccess: (_, choiceValue) => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      void qc.invalidateQueries({ queryKey: ["provider", entry.id] });
      const choice = modelChoices.find((item) => item.value === choiceValue);
      setFeedback(t("card.modelChanged", { model: choice?.label ?? choiceValue }));
    },
    onError: (error) => setFeedback(t("card.modelChangeError", { msg: String(error) })),
  });

  const remove = useMutation({
    mutationFn: () => api.removeProvider(entry.id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      void qc.invalidateQueries({ queryKey: ["provider", entry.id] });
      setConfirmOpen(false);
    },
  });

  const statusBadge = (() => {
    switch (view.kind) {
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
      case "disabled":
        return <Badge tone="neutral" dot>{t("card.disabled")}</Badge>;
      case "loading":
        return <Badge tone="neutral" dot>{t("card.querying")}</Badge>;
      default:
        return <Badge tone="neutral">{t("card.noData")}</Badge>;
    }
  })();

  const timestamp = view.at ? relativeTime(view.at, lang) : null;
  const timestampLabel = view.kind === "stale" || view.kind === "snapshot"
    ? t("card.lastSuccess", { time: timestamp ?? "—" })
    : timestamp ?? t("card.neverSucceeded");
  const displayedError = view.kind === "invalid"
    ? `${t("card.invalidPrefix")}${view.errorMessage ?? t("card.noReason")}`
    : view.errorMessage;
  const copyable = canCopyError(view);
  const copyErrorInfo = () => {
    // clipboard 在异常环境（非安全上下文）可能为 undefined，显式给出失败反馈
    const clipboard = navigator.clipboard;
    if (!clipboard) {
      setFeedback(t("card.copyFailed"));
      return;
    }
    clipboard
      .writeText(errorCopyText(view))
      .then(() => setFeedback(t("card.copied")))
      .catch(() => setFeedback(t("card.copyFailed")));
  };

  return (
    <article
      data-card-id={entry.id}
      className={`qt-provider-card ${expanded ? "is-expanded" : ""} ${
        !entry.enabled ? "is-disabled" : ""
      } ${view.kind === "stale" || view.kind === "transient" ? "is-warning" : ""} ${
        anyOverThreshold ? "has-balance-alert" : ""
      } ${isDragSource ? "is-drag-source" : ""}`}
      style={dragShift !== undefined ? { transform: `translateY(${dragShift}px)` } : undefined}
    >
      {dragHandleProps && (
        <button
          type="button"
          className="qt-drag-handle"
          aria-label={t("card.dragHandle")}
          title={t("card.dragHandleHint")}
          disabled={dragHandleProps.disabled}
          onPointerDown={dragHandleProps.onPointerDown}
          onKeyDown={dragHandleProps.onKeyDown}
        />
      )}
      <div className="qt-provider-primary">
        <div className="qt-provider-identity">
          <span className={`qt-provider-avatar${platformLightLogo ? " is-light-logo" : ""}`}>
            {platformIconUrl ? (
              <img src={platformIconUrl} alt="" aria-hidden="true" draggable={false} />
            ) : (
              providerInitials(platformName)
            )}
          </span>
          <div className="qt-provider-heading">
            <div className="qt-provider-name-row">
              <h2>{entry.name}</h2>
              {statusBadge}
            </div>
            <div className="qt-provider-route">
              <span
                className="qt-provider-route-label"
                title={pricingView?.modelLabel ? `${platformName} · ${pricingView.modelLabel}` : platformName}
              >
                {platformName}
                {pricingView?.modelLabel ? ` · ${pricingView.modelLabel}` : ""}
              </span>
              {showModelSelect && (
                <select
                  aria-label={t("card.switchModel")}
                  title={selectedTitle}
                  value={modelSelectValue}
                  disabled={switchModel.isPending}
                  onChange={(event) => {
                    setFeedback(null);
                    switchModel.mutate(event.target.value);
                  }}
                  className="qt-select qt-provider-model-select"
                >
                  {!explicitModelChoice && entry.pricing?.model && (
                    <option value={`model:${entry.pricing.model}`}>{entry.pricing.model}</option>
                  )}
                  {!hasImplicitDefaultChoice && (
                    <option value="default">{t("pricing.noModel")}</option>
                  )}
                  {modelChoices.map((choice) => (
                    <option key={choice.value} value={choice.value} title={optionText(choice)}>
                      {optionText(choice)}
                    </option>
                  ))}
                </select>
              )}
            </div>
          </div>
        </div>

        <div className={`qt-provider-balance ${multiWindow ? "is-multi" : ""}`}>
          {multiWindow ? (
            view.data.map((item, index) => {
              const itemValue = primaryValue(
                item,
                lang,
                windowShortLabel(item.plan_name, index, lang),
              );
              const itemReset = resetCountdown(item.reset_at);
              return (
                <div className="qt-balance-item" key={item.plan_name ?? index}>
                  <span>{itemValue.label}</span>
                  <strong className={thresholdStates[index] ? "is-alert" : ""}>
                    {itemReset && (
                      <small
                        className="qt-balance-reset"
                        title={t("card.resetIn", { time: itemReset })}
                      >
                        {itemReset}
                      </small>
                    )}
                    {itemValue.unit && <small>{itemValue.unit}</small>}
                    {itemValue.value}
                  </strong>
                </div>
              );
            })
          ) : (
            <>
              <span>{primary.label}</span>
              <strong className={overThreshold ? "is-alert" : ""}>
                {mainReset && (
                  <small className="qt-balance-reset" title={t("card.resetIn", { time: mainReset })}>
                    {mainReset}
                  </small>
                )}
                {primary.unit && <small>{primary.unit}</small>}
                {primary.value}
              </strong>
            </>
          )}
        </div>

        <div className="qt-provider-meta">
          <Tooltip text={view.at ? exactTime(view.at, lang) : timestampLabel}>
            <span className={view.kind === "stale" || view.kind === "transient" ? "is-warning" : ""}>
              {view.kind === "stale" || view.kind === "transient" ? (
                <WifiOff size={15} aria-hidden="true" />
              ) : timestamp ? (
                <CheckCircle2 size={15} aria-hidden="true" />
              ) : (
                <Clock3 size={15} aria-hidden="true" />
              )}
              {timestampLabel}
            </span>
          </Tooltip>
          <button
            type="button"
            className="qt-details-toggle"
            aria-expanded={expanded}
            onClick={() => setExpanded((value) => !value)}
          >
            {expanded ? t("card.collapse") : t("card.details")}
            {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
          </button>
        </div>
      </div>

      <div className="qt-provider-secondary">
        <div className="qt-provider-secondary-info">
          {!entry.enabled ? (
            <div className="qt-provider-security">
              <Pause size={15} aria-hidden="true" />
              {t("card.disabledNote")}
            </div>
          ) : pricingView ? (
            <>
              <div className="qt-pricing-context">
                <span className={`qt-period-dot ${pricingView.period === "peak" ? "is-peak" : "is-offpeak"}`} />
                <span className={`qt-period-text ${pricingView.period === "peak" ? "is-peak" : "is-offpeak"}`}>
                  {pricingView.plan === "subscription"
                    ? pricingView.period === "peak"
                      ? t("card.subscriptionPeak")
                      : t("card.subscriptionOffPeak")
                    : pricingView.period === "peak"
                    ? t("card.periodPeak")
                    : t("card.periodOffPeak")}
                </span>
                {pricingView.plan === "pay_as_you_go" && pricingView.currency && (
                  <span>· {pricingView.currency} / {t("pricing.unitShort")}</span>
                )}
              </div>
              {pricingView.plan === "subscription" ? (
                <div className="qt-subscription-note">
                  <Badge tone="accent">{t("pricing.subscriptionShort")}</Badge>
                  <span>{t("pricing.subscriptionHint")}</span>
                </div>
              ) : pricingView.tier && (
                <dl className="qt-provider-prices">
                  <div><dt>{t("pricing.hit")}</dt><dd>{formatPrice(pricingView.tier.cache_hit_input)}</dd></div>
                  <div><dt>{t("pricing.miss")}</dt><dd>{formatPrice(pricingView.tier.cache_miss_input)}</dd></div>
                  <div><dt>{t("pricing.out")}</dt><dd>{formatPrice(pricingView.tier.output)}</dd></div>
                </dl>
              )}
            </>
          ) : (
            <div className="qt-provider-security">
              <KeyRound size={15} aria-hidden="true" />
              {cliCredential
                ? t("card.cliCredential")
                : configured
                  ? t("card.keyConfigured")
                  : t("card.keyMissing")}
              <span>·</span>
              {t("card.refreshEvery", { minutes: intervalMinutes })}
            </div>
          )}
          {view.data.length === 1 && mainData?.total != null && usedPercent(mainData) == null && (
            <p className="qt-provider-total">
              {t("card.totalQuota", { total: mainData.total })}
            </p>
          )}
          {displayedError && (
            <p
              className={`qt-provider-error ${
                view.kind === "deterministic" || view.kind === "invalid" ? "is-danger" : ""
              }`}
            >
              {displayedError}
              {copyable && (
                <IconButton
                  icon={ClipboardCopy}
                  label={t("card.copyError")}
                  className="qt-provider-error-copy"
                  onClick={copyErrorInfo}
                />
              )}
            </p>
          )}
        </div>

        <div className="qt-provider-actions">
          <Button
            variant="ghost"
            icon={RefreshCw}
            disabled={!entry.enabled || toggleEnabled.isPending || toggleProxy.isPending}
            onClick={invalidate}
          >
            {view.kind === "transient" ? t("card.retry") : t("card.refresh")}
          </Button>
          <Button
            variant="ghost"
            icon={entry.enabled ? Pause : Play}
            disabled={toggleEnabled.isPending || toggleProxy.isPending}
            onClick={() => {
              setFeedback(null);
              toggleEnabled.mutate();
            }}
          >
            {entry.enabled ? t("card.disable") : t("card.enable")}
          </Button>
          <Button
            variant="ghost"
            icon={Globe}
            className={entry.use_proxy ? "is-active" : undefined}
            disabled={toggleProxy.isPending || toggleEnabled.isPending}
            onClick={() => {
              setFeedback(null);
              toggleProxy.mutate();
            }}
          >
            {entry.use_proxy ? t("card.proxyOn") : t("card.proxyOff")}
          </Button>
          <Button variant="ghost" icon={Pencil} onClick={() => onEdit(entry, mainData?.unit)}>
            {t("card.edit")}
          </Button>
          <div className="qt-provider-menu-anchor">
            <IconButton
              icon={Ellipsis}
              label={t("card.more")}
              aria-expanded={menuOpen}
              onClick={() => setMenuOpen((value) => !value)}
            />
            <DropdownMenu open={menuOpen} onClose={() => setMenuOpen(false)}>
              <MenuItem
                icon={Trash2}
                onClick={() => {
                  setFeedback(null);
                  setMenuOpen(false);
                  setConfirmOpen(true);
                }}
              >
                {t("card.remove")}
              </MenuItem>
            </DropdownMenu>
          </div>
        </div>
      </div>

      {(feedback || switchModel.isPending || query.isFetching) && (
        <div className="qt-provider-feedback" role="status" aria-live="polite">
          {switchModel.isPending
            ? t("card.modelChanging")
            : query.isFetching
              ? t("card.refreshing")
              : feedback}
        </div>
      )}

      <ConfirmDialog
        open={confirmOpen}
        title={t("card.removeTitle")}
        message={t("card.confirmRemove", { name: entry.name })}
        confirmLabel={t("card.remove")}
        cancelLabel={t("common.cancel")}
        pending={remove.isPending}
        onClose={() => setConfirmOpen(false)}
        onConfirm={() => remove.mutate()}
      />
    </article>
  );
});
