import { preferMetric, remainingPercent, resetCountdown, windowShortLabel } from "../display";
import type {
  PrimaryMetric,
  ProviderEntry,
  QueryOutcome,
  Settings,
  SnapshotEntry,
  UsageData,
} from "../types";
import { deriveProviderCardState, primaryValue, type ProviderCardKind } from "./providerCardView";

export interface HoverRingView {
  fillPercent: number;
  center: string;
}

/** 压缩布局阈值：窗口逻辑高度低于此值视为压缩视口——后端在垂直空间
 * 不足时把窗口缩到压缩高度（hover_panel.rs 的 PANEL_HEIGHT_COMPACT），
 * 前端据此裁掉峰谷/用量列表/页脚，只留头部+选择器+余额圆环。 */
export const COMPACT_VIEWPORT_THRESHOLD = 400;

export function isCompactViewport(innerHeight: number): boolean {
  return Number.isFinite(innerHeight) && innerHeight < COMPACT_VIEWPORT_THRESHOLD;
}

export function resolveHoverProvider(
  providers: ProviderEntry[],
  trayIconEntryId: string | null,
): ProviderEntry | null {
  const selected = trayIconEntryId
    ? providers.find((provider) => provider.id === trayIconEntryId && provider.enabled)
    : undefined;
  return selected ?? providers.find((provider) => provider.enabled) ?? null;
}

/** 悬停面板圆环视图（与托盘 ring.rs 圆环语义成对）。
 *  主度量偏好分档经 display.preferMetric 骨架（T-24，#142；PR #146
 *  review 抽取）：amount 档余额环优先（走每圈单位分层机制）、
 *  auto/percent 百分比环优先；指定度量算不出时静默回退另一度量。 */
export function hoverRingView(
  data: UsageData | undefined,
  unitsPerCircle: number,
  metric: PrimaryMetric = "auto",
): HoverRingView | null {
  if (!data || data.is_valid === false) return null;

  // T-22 收敛：百分比剩余换算统一走 display.remainingPercent（与 core
  // remaining_percent 镜像），不再局部做 100−used 反向换算；clamp 保留
  // 圆环视觉防护（越界数据不出界）。
  const percentView = (): HoverRingView | null => {
    const percent = remainingPercent(data);
    if (percent == null || !Number.isFinite(percent)) return null;
    const remaining = Math.max(0, Math.min(100, percent));
    return { fillPercent: remaining, center: `${Math.round(remaining)}%` };
  };

  const balanceView = (): HoverRingView | null => {
    if (data.remaining == null || !Number.isFinite(data.remaining)) return null;
    const remaining = Math.max(0, data.remaining);
    const perRing = Number.isFinite(unitsPerCircle) && unitsPerCircle > 0 ? unitsPerCircle : 100;
    const full = Math.floor(remaining / perRing);
    const fraction = (remaining % perRing) / perRing;
    const layerCount = full + (fraction > 0 ? 1 : 0);
    const fillPercent = layerCount > 4 ? 100 : fraction > 0 ? fraction * 100 : remaining > 0 ? 100 : 0;
    return { fillPercent, center: compactAmount(remaining) };
  };

  return preferMetric(metric, percentView, balanceView);
}

function compactAmount(value: number): string {
  const rounded = Math.round(value);
  if (rounded < 10_000) return String(rounded);
  if (rounded < 1_000_000) {
    const thousands = value / 1_000;
    return thousands >= 10 ? `${Math.round(thousands)}k` : `${Math.floor(thousands * 10) / 10}k`;
  }
  if (rounded < 1_000_000_000) {
    const millions = value / 1_000_000;
    return millions >= 10 ? `${Math.round(millions)}M` : `${Math.floor(millions * 10) / 10}M`;
  }
  const billions = value / 1_000_000_000;
  return billions >= 10 ? `${Math.round(billions)}B` : `${Math.floor(billions * 10) / 10}B`;
}

/** 悬停面板内容模式（规格 #151）：`multi` 全部账户列表 + 详情抽屉，
 *  `single` 单账户视图（升级前的行为）。未知值回退 multi，与后端
 *  settings.sanitize 同向。 */
export type HoverPanelMode = "multi" | "single";

export function hoverPanelMode(mode: string | null | undefined): HoverPanelMode {
  return mode === "single" ? "single" : "multi";
}

/** 列表行的单个额度窗口指标（label 已含「剩余」族措辞）。 */
export interface HoverRowMetric {
  key: string;
  label: string;
  value: string;
  unit: string;
  reset: string | null;
  /** 剩余百分比 ≤ 低余额阈值时为真（警示色）。 */
  alert: boolean;
}

/** 多条目列表的一行：账户身份 + 全部窗口指标 + 显示目标标记。 */
export interface HoverRow {
  id: string;
  entry: ProviderEntry;
  kind: ProviderCardKind;
  data: UsageData[];
  at: number | null;
  errorMessage: string | null;
  metrics: HoverRowMetric[];
  /** 该行是否为托盘圆环的显示目标（tray_icon_entry_id 或回退项）。 */
  isTarget: boolean;
  /** 目标环弧长：与托盘圆环同一派生规则；无数据为 null。 */
  ringFill: number | null;
}

export interface HoverRowsInput {
  providers: ProviderEntry[];
  states: Record<string, QueryOutcome> | undefined;
  snapshots: Record<string, SnapshotEntry> | undefined;
  settings: Settings | undefined;
  lang: "zh" | "en";
  nowMs?: number;
}

/** 多条目列表行派生（纯函数）：仅启用条目、保持存储顺序；每个窗口一行
 *  指标；显示目标按 resolveHoverProvider 的同一语义判定。
 *  状态派生与主页卡片同源（deriveProviderCardState），未查询过的条目
 *  回退到启动快照。 */
export function hoverRows({
  providers,
  states,
  snapshots,
  settings,
  lang,
  nowMs,
}: HoverRowsInput): HoverRow[] {
  const target = resolveHoverProvider(providers, settings?.tray_icon_entry_id ?? null);
  const unitsPerCircle = settings?.ring_units_per_circle ?? 100;
  const threshold = settings?.low_balance_remaining_percent ?? 20;

  return providers
    .filter((provider) => provider.enabled)
    .map((entry) => {
      const view = deriveProviderCardState({
        enabled: true,
        outcome: states?.[entry.id],
        snapshot: snapshots?.[entry.id],
        nowMs,
      });
      const data = view.data.filter((item) => item.is_valid !== false);
      const metrics: HoverRowMetric[] = data.map((item, index) => {
        const windowLabel =
          data.length > 1 ? windowShortLabel(item.plan_name, index, lang) : undefined;
        const value = primaryValue(item, lang, windowLabel, entry.primary_metric);
        const percent = remainingPercent(item);
        return {
          // windowsFrom 模板会让同一行多个窗口复用 plan_name，索引必须入 key
          key: `${entry.id}:${index}:${item.plan_name ?? ""}`,
          label: value.label,
          value: value.value,
          unit: value.unit,
          reset: resetCountdown(item.reset_at),
          alert: (percent ?? Number.POSITIVE_INFINITY) <= threshold,
        };
      });
      // 与托盘 ring.rs 的 data_ring_input 同规则：跳过算不出弧长的窗口，
      // 取首个可用窗口（只看 data[0] 会在首窗口缺字段时误报无环）。
      let ringFill: number | null = null;
      for (const item of data) {
        const ring = hoverRingView(item, unitsPerCircle, entry.primary_metric);
        if (ring) {
          ringFill = ring.fillPercent;
          break;
        }
      }
      return {
        id: entry.id,
        entry,
        kind: view.kind,
        data,
        at: view.at,
        errorMessage: view.errorMessage,
        metrics,
        isTarget: target?.id === entry.id,
        ringFill,
      };
    });
}

/** on/off 环的悬停提示文案键（按目标状态区分）。 */
export function hoverTargetHintKey(isTarget: boolean): "hover.targetOn" | "hover.targetOff" {
  return isTarget ? "hover.targetOn" : "hover.targetOff";
}

/** 全量刷新失败汇总：`ids` 为失败条目，空数组表示整批/未知归属的失败。 */
export interface HoverBatchFailure {
  message: string;
  ids: string[];
}

/** 批次失败中属于该行的文案；不属于该行时为 null——别家账户的错误不该
 *  出现在当前账户的详情里。空 ids（整批失败）对所有行都算相关。 */
export function batchFailureForRow(
  failure: HoverBatchFailure | null | undefined,
  rowId: string,
): string | null {
  if (!failure) return null;
  if (failure.ids.length === 0) return failure.message;
  return failure.ids.includes(rowId) ? failure.message : null;
}

/** 单账户 hero 标签档位（升级前语义）：percent →「剩余[ 窗口]」、
 *  amount →「可用余额」、算不出 →「暂无数据」。与 primaryValue 的取值
 *  分档同一 preferMetric 规则，仅用于保持 single 模式文案逐字一致。 */
export function hoverPrimaryLabelKind(
  data: UsageData | undefined,
  metric: PrimaryMetric = "auto",
): "percent" | "amount" | "empty" {
  if (!data) return "empty";
  const kind = preferMetric<"percent" | "amount">(
    metric,
    () => (remainingPercent(data) == null ? null : "percent"),
    () => (data.remaining == null ? null : "amount"),
  );
  return kind ?? "empty";
}

/** 串行刷新一批条目：单条失败不中断后续（平台限流下仍尽量刷新全部），
 *  返回成功与失败清单供调用方汇总报错。
 *  `run` 可能以 QueryOutcome 的 `ok=false` 表达平台失败（命令本身成功），
 *  由调用方通过 `failure` 判定为失败并给出消息。 */
export async function refreshSequentially<T>(
  ids: string[],
  run: (id: string) => Promise<T>,
  failure?: (result: T) => string | null,
): Promise<{ ok: string[]; failed: { id: string; error: string }[] }> {
  const ok: string[] = [];
  const failed: { id: string; error: string }[] = [];
  for (const id of ids) {
    try {
      const result = await run(id);
      const failureMessage = failure?.(result) ?? null;
      // 非 null 即失败：空消息也代表失败，不能被当成成功
      if (failureMessage != null) failed.push({ id, error: failureMessage });
      else ok.push(id);
    } catch (error) {
      failed.push({
        id,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }
  return { ok, failed };
}
