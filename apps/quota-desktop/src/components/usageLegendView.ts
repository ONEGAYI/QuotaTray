import { usageComparisonId } from "./usageComparisonView";

/** 药丸入口全平台统一渲染：有可比组合即显示；桌面悬停展开浮层，移动端点击打开聚焦模态窗 */
export function legendTriggerVisible(scopeCount: number): boolean {
  return scopeCount > 0;
}

export function toggleSeriesFocus(current: string | null, id: string): string | null {
  return current === id ? null : id;
}

export type LegendRemoveOutcome = { kind: "armed"; id: string } | { kind: "removed"; id: string };

export function pressLegendRemove(armedId: string | null, id: string): LegendRemoveOutcome {
  return armedId === id ? { kind: "removed", id } : { kind: "armed", id };
}

export interface LegendItemInput { provider_id: string; window_key: string; color_slot: number; }

export interface LegendItem {
  id: string;
  providerId: string;
  windowKey: string;
  colorSlot: number;
  /** 是否存在可绘制的可见曲线（可聚焦）；隐藏/失效条目为 false，仅保留展示与删除 */
  available: boolean;
  name: string;
}

/** 行列表以存储的 selection 全量驱动：单位冲突被隐藏、provider 已删或窗口失效的条目仍可定位与删除 */
export function buildLegendItems(
  selections: readonly LegendItemInput[],
  availableIds: ReadonlySet<string>,
  candidateNames: ReadonlyMap<string, string>,
): LegendItem[] {
  return selections.map((selection) => {
    const id = usageComparisonId(selection.provider_id, selection.window_key);
    return {
      id,
      providerId: selection.provider_id,
      windowKey: selection.window_key,
      colorSlot: selection.color_slot,
      available: availableIds.has(id),
      name: candidateNames.get(id) ?? `${selection.provider_id} · ${selection.window_key}`,
    };
  });
}

/** 聚焦平台（卡头药丸内下陷显示窗）的展示取数。 */
export interface FocusPlatformInfo {
  name: string;
  colorSlot: number;
  /** 最新样本值；组合暂不可绘或无样本时为 null（展示为 —） */
  value: number | null;
  metric: "percent" | "absolute";
}

/** 未聚焦或聚焦项不在行列表时返回 null；值取该组合时间线的最后一个样本。 */
export function focusPlatformInfo(
  focusedId: string | null,
  items: readonly LegendItem[],
  scopes: readonly { id: string; metric: FocusPlatformInfo["metric"]; samples: readonly { value: number }[] }[],
): FocusPlatformInfo | null {
  if (!focusedId) return null;
  const item = items.find((entry) => entry.id === focusedId);
  if (!item) return null;
  const scope = scopes.find((entry) => entry.id === focusedId);
  const last = scope ? scope.samples[scope.samples.length - 1] : undefined;
  return { name: item.name, colorSlot: item.colorSlot, value: last ? last.value : null, metric: scope?.metric ?? "percent" };
}
