// 供应商卡片：名称 / 类型徽标 / 用量数据 / 错误徽标 / 相对时间 / 手动操作。
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "../api";
import { dataSummary, relativeTime, usedPercent } from "../display";
import { useProviderQuery } from "../queries";
import { KEEP_LAST_GOOD_MS, type ProviderEntry, type QueryOutcome } from "../types";

interface Props {
  entry: ProviderEntry;
  intervalMinutes: number;
  thresholdPercent: number;
  onEdit: (entry: ProviderEntry) => void;
}

/** 卡片展示视图（展示语义与 Rust tray.rs 的纯函数保持一致）。 */
function cardView(outcome: QueryOutcome | undefined) {
  if (!outcome) {
    return { badge: null as string | null, stale: false };
  }
  if (!outcome.ok && outcome.error) {
    const keepGood =
      outcome.error.kind === "transient" &&
      outcome.data != null &&
      outcome.at != null &&
      Date.now() - outcome.at <= KEEP_LAST_GOOD_MS;
    return { badge: outcome.error.kind, stale: keepGood };
  }
  const invalid = outcome.data?.find((d) => d.is_valid === false);
  if (invalid) {
    return { badge: "invalid", stale: false };
  }
  return { badge: null, stale: false };
}

export function ProviderCard({ entry, intervalMinutes, thresholdPercent, onEdit }: Props) {
  const qc = useQueryClient();
  const query = useProviderQuery(entry.id, entry.enabled, intervalMinutes);
  const outcome = query.data;
  const view = cardView(outcome);
  const configured = Boolean(entry.api_key_enc);

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ["provider", entry.id] });
  };

  const toggleEnabled = useMutation({
    mutationFn: () => api.upsertProvider({ ...entry, enabled: !entry.enabled }, null),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
    },
  });

  const remove = useMutation({
    mutationFn: () => api.removeProvider(entry.id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      void qc.invalidateQueries({ queryKey: ["provider", entry.id] });
    },
  });

  const kindBadge =
    entry.kind.type === "native" ? `native · ${entry.kind.provider}` : "template";

  const badgeEl = (() => {
    if (!entry.enabled) {
      return <span className="rounded bg-slate-100 px-2 py-0.5 text-xs text-slate-500">已停用</span>;
    }
    switch (view.badge) {
      case "deterministic":
        return <span className="rounded bg-red-100 px-2 py-0.5 text-xs text-red-700">确定性失败</span>;
      case "invalid":
        return <span className="rounded bg-red-100 px-2 py-0.5 text-xs text-red-700">已失效</span>;
      case "transient":
        return (
          <span className="rounded bg-slate-200 px-2 py-0.5 text-xs text-slate-600">
            {view.stale ? "暂不可达 · 保留旧值" : "网络波动"}
          </span>
        );
      default:
        return null;
    }
  })();

  return (
    <div className="rounded-lg border border-slate-200 bg-white p-4 shadow-sm">
      <div className="flex items-center gap-2">
        <span className={`font-medium ${entry.enabled ? "" : "text-slate-400"}`}>{entry.name}</span>
        <span className="rounded bg-indigo-50 px-2 py-0.5 text-xs text-indigo-700">{kindBadge}</span>
        {badgeEl}
        <span className="ml-auto flex items-center gap-1">
          <button
            title="手动刷新"
            disabled={!entry.enabled}
            onClick={invalidate}
            className="rounded px-2 py-1 text-slate-500 hover:bg-slate-100 disabled:opacity-40"
          >
            ⟳
          </button>
          <button
            title={entry.enabled ? "停用" : "启用"}
            onClick={() => toggleEnabled.mutate()}
            className="rounded px-2 py-1 text-slate-500 hover:bg-slate-100"
          >
            {entry.enabled ? "⏸" : "▶"}
          </button>
          <button
            title="编辑"
            onClick={() => onEdit(entry)}
            className="rounded px-2 py-1 text-slate-500 hover:bg-slate-100"
          >
            ✎
          </button>
          <button
            title="删除"
            onClick={() => {
              if (window.confirm(`确定删除「${entry.name}」？`)) remove.mutate();
            }}
            className="rounded px-2 py-1 text-slate-400 hover:bg-red-50 hover:text-red-600"
          >
            ✕
          </button>
        </span>
      </div>

      <div className="mt-2 space-y-1">
        {!entry.enabled ? (
          <p className="text-sm text-slate-400">条目已停用，不参与查询</p>
        ) : outcome == null ? (
          <p className="text-sm text-slate-400">{query.isFetching ? "查询中…" : "尚无数据"}</p>
        ) : view.badge === "deterministic" ? (
          <p className="text-sm text-red-600">{outcome.error?.message}</p>
        ) : view.badge === "invalid" ? (
          <p className="text-sm text-red-600">
            已失效：{outcome.data?.find((d) => d.is_valid === false)?.invalid_message ?? "未说明原因"}
          </p>
        ) : (
          (view.stale ? outcome.data ?? [] : outcome.data ?? []).map((d, i) => {
            const pct = usedPercent(d);
            const over = pct != null && pct >= thresholdPercent;
            return (
              <div key={i} className="flex items-baseline gap-2 text-sm">
                {outcome.data != null && outcome.data.length > 1 && (
                  <span className="text-slate-500">{d.plan_name ?? `窗口${i + 1}`}</span>
                )}
                <span className={over ? "font-semibold text-red-600" : "text-slate-800"}>
                  {dataSummary(d)}
                  {over && " ⚠"}
                </span>
                {d.total != null && pct == null && (
                  <span className="text-xs text-slate-400">总额度 {d.total}</span>
                )}
              </div>
            );
          })
        )}
      </div>

      <div className="mt-2 flex items-center gap-3 text-xs text-slate-400">
        <span>
          {configured ? "已配置 key" : "未配置 key"}
        </span>
        <span>·</span>
        <span>{relativeTime(outcome?.at)}</span>
        {query.isFetching && <span className="animate-pulse">刷新中…</span>}
        {view.stale && outcome?.error && <span>· {outcome.error.message}</span>}
      </div>
    </div>
  );
}
