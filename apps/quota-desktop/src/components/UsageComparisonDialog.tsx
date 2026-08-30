import { Plus, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useLang } from "../i18n";
import type { UsageComparisonSeries } from "../types";
import {
  MAX_USAGE_COMPARISONS,
  addUsageComparison,
  removeUsageComparison,
  usageComparisonConflict,
  usageComparisonId,
} from "./usageComparisonView";
import { Button, DialogShell } from "./ui";

export interface UsageComparisonCandidate {
  id: string;
  providerId: string;
  providerName: string;
  windowKey: string;
  windowName: string;
  metric: "absolute" | "percent";
  unit: string;
}

export type UsageComparisonDialogMode = "add" | "manage";

export function UsageComparisonDialog({
  mode,
  candidates,
  selected,
  onClose,
  onSave,
}: {
  mode: UsageComparisonDialogMode;
  candidates: UsageComparisonCandidate[];
  selected: UsageComparisonSeries[];
  onClose: () => void;
  onSave: (next: UsageComparisonSeries[]) => Promise<void>;
}) {
  const { t } = useLang();
  const [providerId, setProviderId] = useState("");
  const [candidateId, setCandidateId] = useState("");
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const selectedIds = useMemo(() => new Set(selected.map((item) => (
    usageComparisonId(item.provider_id, item.window_key)
  ))), [selected]);
  const selectedUnits = useMemo(() => selected.map((item) => (
    candidates.find((candidate) => candidate.id === usageComparisonId(item.provider_id, item.window_key))?.unit
  )).filter((unit): unit is string => Boolean(unit)), [candidates, selected]);
  const available = useMemo(() => candidates.filter((candidate) => !selectedIds.has(candidate.id)), [candidates, selectedIds]);
  const providers = useMemo(() => [...new Map(available.map((candidate) => [
    candidate.providerId,
    candidate.providerName,
  ])).entries()], [available]);
  const providerCandidates = useMemo(() => available.filter((candidate) => candidate.providerId === providerId), [available, providerId]);

  useEffect(() => {
    setError("");
    const nextProvider = providers.some(([id]) => id === providerId) ? providerId : providers[0]?.[0] ?? "";
    if (nextProvider !== providerId) setProviderId(nextProvider);
    const nextCandidates = available.filter((candidate) => candidate.providerId === nextProvider);
    if (!nextCandidates.some((candidate) => candidate.id === candidateId)) {
      setCandidateId(nextCandidates[0]?.id ?? "");
    }
  }, [mode, candidates, selected, providerId, candidateId, providers, available]);

  const candidate = candidates.find((item) => item.id === candidateId);
  const previewResult = candidate ? addUsageComparison(selected, candidate) : null;
  const previewColorSlot = previewResult?.ok ? previewResult.value[previewResult.value.length - 1].color_slot : 0;
  const atLimit = selected.length >= MAX_USAGE_COMPARISONS;
  const conflict = candidate ? usageComparisonConflict(selectedUnits, candidate.unit) : null;
  const title = mode === "add" ? t("usage.addCombinationTitle") : t("usage.manageCombinationTitle");
  const description = mode === "add" ? t("usage.addCombinationDesc") : t("usage.manageCombinationDesc");

  const save = async (next: UsageComparisonSeries[], id: string, closeAfter: boolean) => {
    setPendingId(id);
    setError("");
    try {
      await onSave(next);
      if (closeAfter) onClose();
    } catch (err) {
      setError(t("usage.saveFailed", { msg: String(err) }));
    } finally {
      setPendingId(null);
    }
  };

  const footer = mode === "add" ? (
    <>
      <span className="qt-usage-dialog-count">
        {t("usage.combinationCount", { count: selected.length })} · {t("usage.autoSaved")}
      </span>
      <Button
        variant="primary"
        icon={Plus}
        disabled={!candidate || atLimit || Boolean(conflict) || pendingId != null}
        onClick={() => {
          if (!candidate) return;
          const result = addUsageComparison(selected, candidate);
          if (result.ok) void save(result.value, candidate.id, true);
        }}
      >
        {t("usage.add")}
      </Button>
    </>
  ) : (
    <>
      <span className="qt-usage-dialog-count">
        {t("usage.combinationCount", { count: selected.length })} · {t("usage.autoSaved")}
      </span>
      <Button variant="primary" onClick={onClose}>{t("usage.done")}</Button>
    </>
  );

  return (
    <DialogShell
      title={title}
      description={description}
      onClose={onClose}
      closeLabel={t("common.close")}
      size="sm"
      className="qt-dialog-usage-comparison"
      backdropClassName="qt-usage-dialog-backdrop"
      closeOnBackdrop
      footer={footer}
    >
      {error && <div className="qt-inline-error" role="alert">{error}</div>}
      {mode === "add" ? (
        <div className="qt-usage-add-form">
          {atLimit ? (
            <div className="qt-inline-warning">{t("usage.limitReached")}</div>
          ) : available.length === 0 ? (
            <div className="qt-inline-warning">{t("usage.noCandidate")}</div>
          ) : (
            <>
              <label className="qt-field">
                <span>{t("usage.provider")}</span>
                <select className="qt-select" value={providerId} onChange={(event) => setProviderId(event.target.value)}>
                  {providers.map(([id, name]) => <option key={id} value={id}>{name}</option>)}
                </select>
              </label>
              <label className="qt-field">
                <span>{t("usage.modelOrWindow")}</span>
                <select className="qt-select" value={candidateId} onChange={(event) => setCandidateId(event.target.value)}>
                  {providerCandidates.map((item) => {
                    const itemConflict = usageComparisonConflict(selectedUnits, item.unit);
                    return (
                      <option key={item.id} value={item.id} disabled={Boolean(itemConflict)}>
                        {item.windowName}{itemConflict ? ` · ${t("usage.unitConflict", { unit: itemConflict })}` : ""}
                      </option>
                    );
                  })}
                </select>
              </label>
              {candidate && (
                <div className="qt-usage-add-preview">
                  <i style={{ background: `var(--qt-series-${previewColorSlot + 1})` }} />
                  <span>
                    <strong>{candidate.providerName} · {candidate.windowName}</strong>
                    <small>{candidate.metric === "percent" ? t("usage.remainingPercent") : t("usage.absoluteValue", { unit: candidate.unit })}</small>
                  </span>
                  <em>{candidate.unit}</em>
                </div>
              )}
              {conflict && <div className="qt-inline-warning">{t("usage.unitConflict", { unit: conflict })}</div>}
            </>
          )}
        </div>
      ) : selected.length === 0 ? (
        <div className="qt-usage-manage-empty">{t("usage.emptySelection")}</div>
      ) : (
        <div className="qt-usage-manage-list">
          {selected.map((item) => {
            const id = usageComparisonId(item.provider_id, item.window_key);
            const resolved = candidates.find((candidate) => candidate.id === id);
            return (
              <div className="qt-usage-manage-row" key={id}>
                <i style={{ background: `var(--qt-series-${item.color_slot + 1})` }} />
                <span>
                  <strong>{resolved ? `${resolved.providerName} · ${resolved.windowName}` : `${item.provider_id} · ${item.window_key}`}</strong>
                  <small>{resolved ? (resolved.metric === "percent" ? t("usage.remainingPercent") : t("usage.absoluteValue", { unit: resolved.unit })) : t("usage.unavailable")}</small>
                </span>
                <Button
                  variant="ghost"
                  icon={Trash2}
                  className="qt-usage-manage-remove"
                  disabled={pendingId != null}
                  onClick={() => void save(removeUsageComparison(selected, item.provider_id, item.window_key), id, false)}
                >
                  {t("usage.remove")}
                </Button>
              </div>
            );
          })}
        </div>
      )}
    </DialogShell>
  );
}
