import { QueryClient, QueryClientProvider, useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronLeft, ExternalLink, RefreshCw, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  useHoverPanelHidden,
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
  batchFailureForRow,
  hoverPanelMode,
  hoverPrimaryLabelKind,
  hoverRingView,
  hoverRows,
  hoverTargetHintKey,
  isCompactViewport,
  refreshSequentially,
  resolveHoverProvider,
  type HoverBatchFailure,
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

/** 列表行按钮的 DOM id：抽屉关闭后把焦点还给触发它的那一行。 */
function rowDomId(id: string): string {
  return `qt-hover-row-${id}`;
}

/** mutation 错误文案：去掉 `Error:` 前缀，横幅直接展示可读消息。 */
function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** 全量刷新失败：携带逐账户错误原文，抽屉按 id 取本账户消息。
 *  不能只带 id 列表——聚合文本里含有每个失败账户的错误，仅凭
 *  `ids.includes(rowId)` 无法阻止别家账户的文字进入当前详情。 */
class RefreshBatchError extends Error {
  constructor(message: string, readonly byId: Record<string, string>) {
    super(message);
    this.name = "RefreshBatchError";
  }
}

/** 抽屉滑出动画时长（与 index.css 的 .qt-hover-drawer 出场 transition 一致）。
 *  滑出期间保持遮罩与 pointer-events 锁定，避免点击穿透到下方列表。 */
const DRAWER_EXIT_MS = 340;

/** 倒计时刷新的时钟粒度：reset_at 按分钟显示，30s 一跳保证跨分钟边界
 *  最多迟到半分钟，同时不给常驻面板增加明显负担。 */
const CLOCK_TICK_MS = 30_000;

/** 面板存活期间的时钟锚点：倒计时与「N 分钟前」都依赖它。 */
function useClockTick(intervalMs = CLOCK_TICK_MS): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(timer);
  }, [intervalMs]);
  return now;
}

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
  nowMs,
}: {
  data: UsageData[];
  metric: ProviderEntry["primary_metric"];
  nowMs: number;
}) {
  const { lang, t } = useLang();
  return (
    <section className="qt-hover-usage-list">
      {data.map((item, index) => {
        const percent = remainingPercent(item);
        const reset = resetCountdown(item.reset_at, nowMs);
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
      {/* 显示目标恒有环：无数据时只画轨道，标记身份不依赖数值 */}
      {isTarget && (
        <svg className="qt-hover-avatar-ring" viewBox="0 0 38 38" aria-hidden="true">
          <rect className="qt-hover-avatar-track" x="2" y="2" width="34" height="34" rx="11" pathLength="100" />
          {ringFill != null && (
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
          )}
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
      // 文案随状态说明当前事实；aria-current 表达「这一项被选中」，
      // 不用 pressed（单选语义下不可取消，避免读屏理解为开关）
      aria-label={hint}
      aria-current={row.isTarget ? "true" : undefined}
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

/** 抽屉错误区：一次动作错误优先；批次错误只在与当前账户相关时展示，
 *  避免整批失败（其它账户）的文字占满这个账户的详情。 */
function DrawerError({
  row,
  actionError,
  batchFailure,
}: {
  row: HoverRow;
  actionError: string | null;
  batchFailure: HoverBatchFailure | null;
}) {
  const batchMessage = batchFailureForRow(batchFailure, row.id);
  const message = actionError ?? batchMessage ?? row.errorMessage;
  if (!message) return null;
  return (
    <p role="alert" className="qt-hover-error">
      {message}
    </p>
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
  // 单次操作错误（切换目标/模型、抽屉刷新）：只属于当前抽屉动作
  const [actionError, setActionError] = useState<string | null>(null);
  // 全量刷新批次的失败汇总：列表横幅展示，并记录失败条目供抽屉过滤
  const [batchFailure, setBatchFailure] = useState<HoverBatchFailure | null>(null);
  // 详情抽屉当前账户；面板每次被唤起/隐藏时清空（规格 #151：不记忆上次账户）
  const [openId, setOpenId] = useState<string | null>(null);
  // 抽屉滑出期间保留内容与遮罩：关闭瞬间若立刻撤掉，会看到内容先消失再滑出
  const [exiting, setExiting] = useState(false);
  // 退场期间继续渲染最后一次打开的账户（行被删除时也有内容可滑出）
  const [drawerSnapshot, setDrawerSnapshot] = useState<HoverRow | null>(null);
  const drawerRef = useRef<HTMLElement>(null);
  const drawerBackRef = useRef<HTMLButtonElement>(null);
  const exitTimerRef = useRef<number | null>(null);
  // 待归还焦点的行 id；undefined 表示无待处理（首次挂载不能被抢焦点）
  const focusReturnRef = useRef<string | null | undefined>(undefined);
  // 每帧的行快照：抽屉关闭时按 id 回查，供退场动画使用。
  // 用 ref 而非渲染期写入 state，避免上游给不稳定引用时形成渲染循环。
  const rowsRef = useRef<HoverRow[]>([]);
  // 最近一次成功选中的行：条目在展开期间被删除时，行已不在 rows 里，
  // 退场仍应滑出最后一份内容而不是瞬间消失。
  const lastRowRef = useRef<HoverRow | null>(null);
  // 压缩布局：后端在垂直空间不足时把窗口缩到压缩高度，前端随之裁剪区块
  const [compact, setCompact] = useState(() => isCompactViewport(window.innerHeight));
  // 面板常驻：时间派生（倒计时、「N 分钟前」）不能只靠数据引用变化驱动，
  // 否则数据被 React Query 结构共享时数字会一直停在旧值。
  const nowMs = useClockTick();
  // 峰谷标签锚点：常驻面板无重渲染时以翻转事件驱动重算（#15）
  const peakTick = usePeakFlipTick();

  // 唤起时补取状态整表与设置：面板窗口常驻且非激活显示，不会触发
  // React Query 的 focus 重取，只能靠事件补新鲜度（#151 双模式/显示目标）。
  useHoverPanelShown(() => {
    setOpenId(null);
    setExiting(false);
    setDrawerSnapshot(null);
    setActionError(null);
    setBatchFailure(null);
  });
  // 隐藏即清抽屉：下次 show 的首帧就是列表，不闪旧抽屉
  useHoverPanelHidden(() => {
    setOpenId(null);
    setExiting(false);
    setDrawerSnapshot(null);
    setActionError(null);
    setBatchFailure(null);
  });

  useEffect(() => {
    const onResize = () => setCompact(isCompactViewport(window.innerHeight));
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    document.body.classList.add("qt-hover-body");
    return () => document.body.classList.remove("qt-hover-body");
  }, []);

  // 抽屉退场定时器：卸载时清掉，避免对已卸载组件 setState
  useEffect(() => () => {
    if (exitTimerRef.current != null) window.clearTimeout(exitTimerRef.current);
  }, []);

  const closeDrawer = useCallback(() => {
    const closingId = openId;
    setOpenId(null);
    setActionError(null);
    if (!closingId) return;
    // 先留存这一行的最后状态：随后即使条目被禁用/删除，退场动画也有内容
    setDrawerSnapshot(
      rowsRef.current.find((row) => row.id === closingId)
        ?? (lastRowRef.current?.id === closingId ? lastRowRef.current : null),
    );
    focusReturnRef.current = closingId;
    setExiting(true);
    if (exitTimerRef.current != null) window.clearTimeout(exitTimerRef.current);
    exitTimerRef.current = window.setTimeout(() => {
      exitTimerRef.current = null;
      setExiting(false);
      setDrawerSnapshot(null);
      // 退场结束再归还焦点：滑出期间行仍被遮罩盖住，提前聚焦会让
      // 焦点环出现在模糊层下；行已被删除时回退到列表容器。
      const returnId = focusReturnRef.current;
      focusReturnRef.current = undefined;
      if (returnId === undefined || returnId === null) return;
      // preventScroll：行可能不在可视区，浏览器默认会 scrollIntoView，
      // 而抽屉此刻仍停在面板右外侧，祖先的可编程滚动会先被拽动再钳回
      const row = document.getElementById(rowDomId(returnId));
      if (row) row.focus({ preventScroll: true });
      else document.getElementById("qt-hover-list")?.focus({ preventScroll: true });
    }, DRAWER_EXIT_MS);
  }, [openId]);

  /** 打开抽屉；从退场动画中途重新打开时先取消退场，直接切到实时行。 */
  const openDrawer = useCallback((id: string) => {
    if (exitTimerRef.current != null) {
      window.clearTimeout(exitTimerRef.current);
      exitTimerRef.current = null;
    }
    focusReturnRef.current = undefined;
    setExiting(false);
    setDrawerSnapshot(null);
    setActionError(null);
    setOpenId(id);
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
        nowMs,
      }),
    [providers.data, states.data, snapshots.data, settings.data, lang, nowMs],
  );
  // 行集合提交后同步一份：关闭抽屉时据此留存退场内容（行已消失也有快照）。
  // 只在 effect 里写 ref，不触发渲染，也不违反并发渲染的纯函数约束。
  useEffect(() => {
    rowsRef.current = rows;
  }, [rows]);
  const selectedRow = openId ? rows.find((row) => row.id === openId) ?? null : null;
  useEffect(() => {
    if (selectedRow) lastRowRef.current = selectedRow;
  }, [selectedRow]);
  // 退场用快照渲染；正常打开时用实时行（数据更新能立即反映）
  const displayedDrawerRow = exiting ? drawerSnapshot : selectedRow;

  // 单账户模式沿用升级前的语义：条目 = 托盘显示目标
  const entry = resolveHoverProvider(
    providers.data ?? [],
    settings.data?.tray_icon_entry_id ?? null,
  );
  // 单次刷新失败的归属：只有失败请求就是当前抽屉账户时才进详情，
  // 否则挂到该账户名下的列表错误。单账户模式下详情即当前账户。
  const applyRefreshFailure = useCallback(
    (id: string, message: string) => {
      if (mode === "single" || openId === id) {
        setActionError(message);
        return;
      }
      const name = enabledProviders.find((provider) => provider.id === id)?.name ?? id;
      setBatchFailure({ message: `${name}: ${message}`, byId: { [id]: message }, global: null });
    },
    [mode, openId, enabledProviders],
  );

  const refreshOne = useMutation({
    mutationFn: (id: string) => api.queryProvider(id),
    // 新一次尝试立即清掉上次错误：成功后不能残留「刷新失败」
    onMutate: () => setActionError(null),
    onSuccess: (outcome, id) => {
      // 平台业务失败以 QueryOutcome.ok=false 正常返回，不走 onError
      if (outcome.ok) return;
      applyRefreshFailure(id, outcome.error?.message || t("card.noReason"));
    },
    // 命令 reject 与业务失败是两条分支：同样必须按请求身份路由，
    // 否则甲迟到的异常会写进此刻打开的乙的详情
    onError: (error, id) => applyRefreshFailure(id, errorText(error)),
  });

  const view = deriveProviderCardState({
    enabled: Boolean(entry),
    outcome: entry ? states.data?.[entry.id] : undefined,
    snapshot: entry ? snapshots.data?.[entry.id] : undefined,
    // 与升级前一致：首次取数/单条刷新在途时显示「查询中」而非「暂无数据」
    isFetching: states.isFetching || refreshOne.isPending,
  });

  // 头部刷新（多条目模式）：串行刷新全部启用条目，避免并发打满平台限流；
  // 单条失败不中断后续。平台查询失败以 QueryOutcome.ok=false 正常返回，
  // 必须按业务失败计入汇总，否则最常见的失败路径不会出现在横幅里。
  const refreshAll = useMutation({
    mutationFn: async (ids: string[]) => {
      const { failed } = await refreshSequentially(
        ids,
        (id) => api.queryProvider(id),
        (outcome) => (outcome.ok ? null : outcome.error?.message || t("card.noReason")),
      );
      if (failed.length > 0) {
        // 聚合文案按账户名展开（横幅可读），同时保留逐账户原文——
        // 抽屉只取自己那一条，不解析聚合字符串
        const nameOf = (id: string) =>
          enabledProviders.find((provider) => provider.id === id)?.name ?? id;
        const byId: Record<string, string> = {};
        for (const item of failed) byId[item.id] = item.error;
        throw new RefreshBatchError(
          failed.map((item) => `${nameOf(item.id)}: ${item.error}`).join("\n"),
          byId,
        );
      }
    },
    onError: (error) =>
      setBatchFailure({
        message: errorText(error),
        byId: error instanceof RefreshBatchError ? error.byId : {},
        global: error instanceof RefreshBatchError ? null : errorText(error),
      }),
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
      setActionError(errorText(error));
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
      setActionError(errorText(error));
    },
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      void qc.invalidateQueries({ queryKey: ["provider-states"] });
    },
  });

  const refreshBusy = refreshAll.isPending || refreshOne.isPending;
  const refresh = () => {
    setActionError(null);
    setBatchFailure(null);
    if (mode === "multi") {
      refreshAll.mutate(enabledProviders.map((provider) => provider.id));
    } else if (entry) {
      refreshOne.mutate(entry.id);
    }
  };

  // 抽屉的派生视图：与单账户视图同一套定价/模型/圆环逻辑
  const drawer = useMemo(() => {
    if (!displayedDrawerRow) return null;
    const target = displayedDrawerRow.entry;
    const drawerNativeId = target.kind.type === "native" ? target.kind.provider : undefined;
    const drawerMeta: NativeMeta | undefined = drawerNativeId
      ? nativeMetas.data?.find((meta) => meta.id === drawerNativeId)
      : undefined;
    const platformName = kindLabel(target.kind, drawerMeta?.name, lang);
    const pricingView = resolveProviderPricingView(
      target,
      drawerMeta,
      peakTick,
      displayedDrawerRow.data[0]?.unit,
    );
    const choices = pricingModelChoices(drawerMeta?.pricing ?? null, drawerMeta?.custom_models ?? []);
    const unitsPerCircle = settings.data?.ring_units_per_circle ?? 100;
    // 与行头像环同一取窗规则：跳过算不出度量的窗口，避免行上有环、抽屉空值
    const heroData = displayedDrawerRow.data.find(
      (item) => hoverRingView(item, unitsPerCircle, target.primary_metric) != null,
    ) ?? displayedDrawerRow.data[0];
    const heroWindow = displayedDrawerRow.data.length > 1
      ? windowShortLabel(heroData?.plan_name, 0, lang)
      : undefined;
    const heroLabelKind = hoverPrimaryLabelKind(heroData, target.primary_metric);
    const hero = primaryValue(
      heroData,
      lang,
      // 金额分支不带窗口后缀（「可用余额」，与主窗卡片一致）
      heroLabelKind === "percent" ? heroWindow : undefined,
      target.primary_metric,
    );
    return {
      entry: target,
      nativeMeta: drawerMeta,
      platformName,
      pricingView,
      choices,
      // 与单账户同一空值口径：算不出主数值时用 card.noData，而非「已获取」
      hero: heroLabelKind === "empty" ? { ...hero, label: t("card.noData") } : hero,
      heroReset: resetCountdown(heroData?.reset_at, nowMs),
      ring: hoverRingView(heroData, unitsPerCircle, target.primary_metric),
      modelValue: modelSelectValue(target, choices),
      hasDefault: choices.some((choice) => choice.value === "default"),
      explicitModel: target.pricing?.model
        ? choices.find(
            (choice) => choice.modelId?.toLowerCase() === target.pricing?.model?.toLowerCase(),
          )
        : undefined,
    };
  }, [displayedDrawerRow, nativeMetas.data, peakTick, lang, settings.data, nowMs, t]);

  // 抽屉是否真实可交互：openId 可能残留（条目被禁用/删除、模式切到 single）
  const drawerOpen = mode === "multi" && selectedRow != null && drawer != null;
  // 退场期间抽屉仍在 DOM 里滑出：列表继续模糊、遮罩继续拦截点击，
  // 直到 DRAWER_EXIT_MS 结束，避免关闭瞬间点到下方条目又开一个新抽屉。
  const drawerVisible = drawerOpen || (exiting && drawerSnapshot != null && drawer != null);

  useEffect(() => {
    // 抽屉实际不可见（条目被删除/禁用、模式切到 single、空表）：走正常关闭
    // 路径，保留快照完成滑出。这里用 drawerOpen 而非 selectedRow：模式切到
    // single 时行仍在，残留 openId 会在切回 multi 后自动重开抽屉。
    if (openId && !drawerOpen) closeDrawer();
  }, [openId, drawerOpen, closeDrawer]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // 抽屉打开时 Esc 先收起抽屉；抽屉已在退场途中时，Esc 直接隐藏面板
      // （不能再落回一个已经关闭的抽屉上变成空操作）
      if (drawerOpen) closeDrawer();
      else void api.hideHoverPanel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [drawerOpen, closeDrawer]);

  // 关闭态立即 inert：滑出动画期间不可聚焦/点击（visibility 延迟只是视觉）
  useEffect(() => {
    if (drawerRef.current) drawerRef.current.inert = !drawerOpen;
  }, [drawerOpen]);

  // 打开抽屉后把焦点交给返回键：键盘用户不必穿过被抽屉遮住的行。
  // preventScroll 必须显式关闭：此刻抽屉还在面板右外侧滑入，浏览器默认
  // 会把焦点元素滚进视野，新面板横向被拽动，动画中再被钳回——就是
  // 「条目区先向左跳、随后回弹」的来源。
  useEffect(() => {
    if (drawerOpen) drawerBackRef.current?.focus({ preventScroll: true });
  }, [drawerOpen]);

  // ---- 单账户视图派生（与升级前一致） ----
  const mainData = view.data[0];
  const primaryMetric = entry?.primary_metric;
  const heroWindow = view.data.length > 1
    ? windowShortLabel(mainData?.plan_name, 0, lang)
    : undefined;
  const primaryLabelKind = hoverPrimaryLabelKind(mainData, primaryMetric);
  const primary = primaryValue(
    mainData,
    lang,
    primaryLabelKind === "percent" ? heroWindow : undefined,
    primaryMetric,
  );
  // 升级前措辞逐字保留：金额「可用余额」不接窗口后缀，算不出时 card.noData
  const primaryLabel = primaryLabelKind === "amount"
    ? t("hover.availableBalance")
    : primaryLabelKind === "percent"
      ? heroWindow
        ? lang === "zh" ? `剩余 ${heroWindow}` : `Left ${heroWindow}`
        : t("hover.remainingQuota")
      : t("card.noData");
  const heroReset = resetCountdown(mainData?.reset_at, nowMs);
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
            <small><i />{headerAt ? relativeTime(headerAt, lang, nowMs) : t("card.neverSucceeded")}</small>
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

      {/* 批次失败横幅：绑定本次全量刷新，不随行导航消失。
          抽屉打开时只让位「抽屉确实承接了这条错误」的情形——即该失败属于
          当前账户或为无归属的全局失败。别家账户的失败抽屉不会显示，
          横幅必须留着，否则这条错误在抽屉开合之间彻底消失。 */}
      {mode === "multi"
        && batchFailure
        && !(drawerOpen && displayedDrawerRow
          && batchFailureForRow(batchFailure, displayedDrawerRow.id) != null) && (
        <p role="alert" className="qt-hover-error qt-hover-error-banner">{batchFailure.message}</p>
      )}

      {mode === "multi" ? (
        rows.length === 0 ? (
          emptyState
        ) : (
          <div className={`qt-hover-body-view${drawerVisible ? " is-drawer-open" : ""}`}>
            <div
              id="qt-hover-list"
              className="qt-hover-list"
              role="group"
              aria-label={t("hover.accounts")}
              aria-hidden={drawerVisible ? true : undefined}
              tabIndex={-1}
            >
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
                    id={rowDomId(row.id)}
                    type="button"
                    className={`qt-hover-row${row.isTarget ? " is-target" : ""}`}
                    aria-haspopup="dialog"
                    aria-expanded={drawerOpen && openId === row.id}
                    aria-controls={drawerOpen && openId === row.id ? "qt-hover-drawer" : undefined}
                    aria-current={row.isTarget ? "true" : undefined}
                    // 抽屉打开（含退场）时把被遮住的行移出 Tab 序列；
                    // 程序化 focus 仍可用，退场结束再归还焦点
                    tabIndex={drawerVisible ? -1 : undefined}
                    onClick={() => openDrawer(row.id)}
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
                      <span className="qt-hover-row-sub-wrap" data-tooltip={subtitle}>
                        <span className="qt-hover-row-sub">{subtitle}</span>
                      </span>
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

            {/* 独立遮罩承接「点击外部」：拖选文本到遮罩上时 click 目标是
                两者公共祖先，不会误判为外部点击。退场期间保留，防止
                点击穿透到被模糊的列表又把抽屉打开 */}
            {drawerVisible && (
              <div
                className="qt-hover-drawer-scrim"
                aria-hidden="true"
                onClick={() => {
                  if (drawerOpen) closeDrawer();
                }}
              />
            )}

            <aside
              ref={drawerRef}
              id="qt-hover-drawer"
              role="dialog"
              className={`qt-hover-drawer${drawerOpen ? " is-open" : ""}`}
              aria-label={t("hover.detail")}
              // 退场期间对读屏立即隐藏：内容仍在滑出，但不再是当前可操作区域
              aria-hidden={!drawerOpen}
            >
              {drawer && displayedDrawerRow && (
                <>
                <div className="qt-hover-drawer-head">
                  <button
                    ref={drawerBackRef}
                    type="button"
                    className="qt-hover-icon-btn"
                    aria-label={t("hover.back")}
                    onClick={closeDrawer}
                  >
                    <ChevronLeft size={16} />
                  </button>
                  <span className="qt-hover-drawer-title">{t("hover.detail")}</span>
                  <button
                    type="button"
                    className={`qt-hover-icon-btn ${refreshBusy ? "is-spinning" : ""}`}
                    aria-label={t("card.refresh")}
                    // 与头部全量刷新互斥：串行设计不允许同一账户并发两次查询
                    disabled={refreshBusy}
                    onClick={() => refreshOne.mutate(displayedDrawerRow.id)}
                  >
                    <RefreshCw size={16} />
                  </button>
                </div>
                <div className="qt-hover-drawer-body">
                  <div className="qt-hover-detail">
                    <div className="qt-hover-detail-head">
                      <AccountAvatar
                        entry={displayedDrawerRow.entry}
                        platformName={drawer.platformName}
                        ringFill={null}
                        isTarget={false}
                      />
                      <div className="qt-hover-detail-id">
                        <strong>{displayedDrawerRow.entry.name}</strong>
                        <small>
                          <i className={`qt-hover-status-dot is-${statusTone(displayedDrawerRow.kind)}`} />
                          <span
                            className="qt-hover-detail-sub-wrap"
                            data-tooltip={`${t(statusKey(displayedDrawerRow.kind))} · ${drawer.platformName}${drawer.pricingView?.modelLabel ? ` · ${drawer.pricingView.modelLabel}` : ""}`}
                          >
                            <span className="qt-hover-detail-sub">
                              {t(statusKey(displayedDrawerRow.kind))} · {drawer.platformName}
                              {drawer.pricingView?.modelLabel ? ` · ${drawer.pricingView.modelLabel}` : ""}
                            </span>
                          </span>
                        </small>
                      </div>
                      <TargetRing
                        row={displayedDrawerRow}
                        busy={switchTarget.isPending}
                        onSet={() => switchTarget.mutate(displayedDrawerRow.id)}
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
                            entry: displayedDrawerRow.entry,
                            choiceValue: event.target.value,
                            choices: drawer.choices,
                          })
                        }
                      >
                        {!drawer.explicitModel && displayedDrawerRow.entry.pricing?.model && (
                          <option value={`model:${displayedDrawerRow.entry.pricing.model}`}>
                            {displayedDrawerRow.entry.pricing.model}
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
                      <div className={`qt-hover-ring ${displayedDrawerRow.metrics.some((metric) => metric.alert) ? "is-alert" : ""}`}>
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

                    <UsageWindows data={displayedDrawerRow.data} metric={displayedDrawerRow.entry.primary_metric} nowMs={nowMs} />

                    {drawer.pricingView && <PricingBlock view={drawer.pricingView} />}

                    <DrawerError
                      row={displayedDrawerRow}
                      actionError={actionError}
                      batchFailure={batchFailure}
                    />
                  </div>
                </div>
                </>
              )}
            </aside>
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
                <span>{primaryLabel}</span>
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
              <UsageWindows data={visibleWindows} metric={primaryMetric} nowMs={nowMs} />
            )}

            {!compact && pricingView && <PricingBlock view={pricingView} />}

            {(view.errorMessage || actionError) && (
              <p role="alert" className="qt-hover-error">{actionError ?? view.errorMessage}</p>
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
