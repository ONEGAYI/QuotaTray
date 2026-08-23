// 供应商卡片：名称 / 类型徽标 / 用量数据 / 错误徽标 / 相对时间 / 手动操作。
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "../api";
import { dataSummary, relativeTime, usedPercent } from "../display";
import { useLang } from "../i18n";
import { useProviderQuery } from "../queries";
import { KEEP_LAST_GOOD_MS, type ProviderEntry, type QueryOutcome, type SnapshotEntry } from "../types";

interface Props {
  entry: ProviderEntry;
  intervalMinutes: number;
  thresholdPercent: number;
  /** 启动快照（spec §5：首屏先渲染上次成功结果，消除重启空窗） */
  snapshot?: SnapshotEntry;
  onEdit: (entry: ProviderEntry) => void;
}

/** 卡片展示视图（展示语义与 Rust tray.rs 的纯函数保持一致）。 */
function cardView(outcome: QueryOutcome | undefined) {
  if (!outcome) {
    return { badge: null as string | null, stale: false };
  }
  if (!outcome.ok && outcome.error) {
    // keep-last-good：瞬时失败且旧值在窗口内 → 保留旧值展示；
    // 超窗或确定性失败 → 错误立即透出
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

export function ProviderCard({ entry, intervalMinutes, thresholdPercent, snapshot, onEdit }: Props) {
  const qc = useQueryClient();
  const { t, lang } = useLang();
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
      return (
        <span className="rounded bg-slate-100 px-2 py-0.5 text-xs text-slate-500 dark:bg-slate-700 dark:text-slate-400">
          {t("card.disabled")}
        </span>
      );
    }
    switch (view.badge) {
      case "deterministic":
        return (
          <span className="rounded bg-red-100 px-2 py-0.5 text-xs text-red-700 dark:bg-red-950/60 dark:text-red-300">
            {t("card.deterministic")}
          </span>
        );
      case "invalid":
        return (
          <span className="rounded bg-red-100 px-2 py-0.5 text-xs text-red-700 dark:bg-red-950/60 dark:text-red-300">
            {t("card.invalid")}
          </span>
        );
      case "transient":
        return (
          <span className="rounded bg-slate-200 px-2 py-0.5 text-xs text-slate-600 dark:bg-slate-700 dark:text-slate-300">
            {view.stale ? t("card.staleKeep") : t("card.network")}
          </span>
        );
      default:
        return null;
    }
  })();

  return (
    <div className="rounded-lg border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-700 dark:bg-slate-800">
      <div className="flex items-center gap-2">
        <span className={`font-medium ${entry.enabled ? "" : "text-slate-400 dark:text-slate-500"}`}>
          {entry.name}
        </span>
        <span className="rounded bg-indigo-50 px-2 py-0.5 text-xs text-indigo-700 dark:bg-indigo-950/50 dark:text-indigo-300">
          {kindBadge}
        </span>
        {badgeEl}
        <span className="ml-auto flex items-center gap-1">
          <button
            title={t("card.refresh")}
            disabled={!entry.enabled}
            onClick={invalidate}
            className="rounded px-2 py-1 text-slate-500 hover:bg-slate-100 disabled:opacity-40 dark:text-slate-400 dark:hover:bg-slate-700"
          >
            ⟳
          </button>
          <button
            title={entry.enabled ? t("card.disable") : t("card.enable")}
            onClick={() => toggleEnabled.mutate()}
            className="rounded px-2 py-1 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-700"
          >
            {entry.enabled ? "⏸" : "▶"}
          </button>
          <button
            title={t("card.edit")}
            onClick={() => onEdit(entry)}
            className="rounded px-2 py-1 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-700"
          >
            ✎
          </button>
          <button
            title={t("card.remove")}
            onClick={() => {
              if (window.confirm(t("card.confirmRemove", { name: entry.name }))) remove.mutate();
            }}
            className="rounded px-2 py-1 text-slate-400 hover:bg-red-50 hover:text-red-600 dark:text-slate-500 dark:hover:bg-red-950/40 dark:hover:text-red-400"
          >
            ✕
          </button>
        </span>
      </div>

      <div className="mt-2 space-y-1">
        {!entry.enabled ? (
          <p className="text-sm text-slate-400 dark:text-slate-500">{t("card.disabledNote")}</p>
        ) : view.badge === "deterministic" ? (
          <p className="text-sm text-red-600 dark:text-red-400">{outcome?.error?.message}</p>
        ) : view.badge === "transient" && !view.stale ? (
          // 瞬时失败且旧值超窗/无旧值 → 错误立即透出
          <p className="text-sm text-slate-600 dark:text-slate-300">{outcome?.error?.message}</p>
        ) : view.badge === "invalid" ? (
          <p className="text-sm text-red-600 dark:text-red-400">
            {t("card.invalidPrefix")}
            {outcome?.data?.find((d) => d.is_valid === false)?.invalid_message ?? t("card.noReason")}
          </p>
        ) : outcome?.data == null && snapshot == null ? (
          <p className="text-sm text-slate-400 dark:text-slate-500">
            {query.isFetching ? t("card.querying") : t("card.noData")}
          </p>
        ) : (
          (outcome?.data ?? snapshot?.data ?? []).map((d, i) => {
            const rows = outcome?.data ?? snapshot?.data ?? [];
            const pct = usedPercent(d);
            const over = pct != null && pct >= thresholdPercent;
            return (
              <div key={i} className="flex items-baseline gap-2 text-sm">
                {rows.length > 1 && (
                  <span className="text-slate-500 dark:text-slate-400">
                    {d.plan_name ?? t("card.windowN", { n: i + 1 })}
                  </span>
                )}
                <span className={over ? "font-semibold text-red-600 dark:text-red-400" : "text-slate-800 dark:text-slate-200"}>
                  {dataSummary(d, lang)}
                  {over && " ⚠"}
                </span>
                {d.total != null && pct == null && (
                  <span className="text-xs text-slate-400 dark:text-slate-500">
                    {t("card.totalQuota", { total: d.total })}
                  </span>
                )}
              </div>
            );
          })
        )}
      </div>

      <div className="mt-2 flex items-center gap-3 text-xs text-slate-400 dark:text-slate-500">
        <span>{configured ? t("card.keyConfigured") : t("card.keyMissing")}</span>
        <span>·</span>
        {/* outcome 为空（首次查询未返回）时回落到启动快照时间 */}
        <span>{relativeTime(outcome?.at ?? snapshot?.at, lang)}</span>
        {outcome?.data == null && snapshot != null && !query.isFetching && (
          <span>{t("card.snapshotAt", { time: relativeTime(snapshot.at, lang) })}</span>
        )}
        {query.isFetching && <span className="animate-pulse">{t("card.refreshing")}</span>}
        {view.stale && outcome?.error && <span>· {outcome.error.message}</span>}
      </div>
    </div>
  );
}
