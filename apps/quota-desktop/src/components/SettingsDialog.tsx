import { useMutation, useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Check, ExternalLink, PackageCheck, SlidersHorizontal } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { relativeTime } from "../display";
import { useLang } from "../i18n";
import { useSettings, useUpdateState } from "../queries";
import type { Settings } from "../types";
import { resolveUpdateError, resolveUpdateStatus } from "./settingsView";
import { Button, DialogShell, SettingRow, Switch } from "./ui";

interface Props {
  open: boolean;
  onClose: () => void;
}

type Tab = "general" | "update";

export function SettingsDialog({ open, onClose }: Props) {
  const qc = useQueryClient();
  const { t, lang } = useLang();
  const settings = useSettings();
  const updateState = useUpdateState();
  const [tab, setTab] = useState<Tab>("general");
  const [draft, setDraft] = useState<Settings | null>(null);
  const [downloadedPath, setDownloadedPath] = useState<string | null>(null);

  useEffect(() => {
    if (open && settings.data) setDraft({ ...settings.data });
  }, [open, settings.data]);

  useEffect(() => {
    if (open) void qc.invalidateQueries({ queryKey: ["update-state"] });
  }, [open, qc]);

  const save = useMutation({
    mutationFn: (value: Settings) => api.saveSettings(value),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["settings"] });
      void qc.invalidateQueries({ queryKey: ["provider"] });
      onClose();
    },
  });

  const checkNow = useMutation({
    mutationFn: api.checkUpdateNow,
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["update-state"] }),
  });

  const download = useMutation({
    mutationFn: api.downloadUpdate,
    onSuccess: (path) => setDownloadedPath(path),
  });

  if (!open || !draft) return null;
  const update = updateState.data;
  const available = update?.available ?? null;
  const operationError = resolveUpdateError({
    checkError: checkNow.isError ? checkNow.error : null,
    downloadError: download.isError ? download.error : null,
    backendError: update?.last_error,
    hasAvailable: Boolean(available),
  });
  const updateStatus = resolveUpdateStatus({
    checking: checkNow.isPending,
    hasAvailable: Boolean(available),
    error: operationError,
  });

  return (
    <DialogShell
      title={t("settings.title")}
      description={t("settings.description")}
      onClose={onClose}
      closeLabel={t("titlebar.close")}
      size="md"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" disabled={save.isPending} onClick={() => save.mutate(draft)}>
            {save.isPending ? t("common.saving") : t("settings.save")}
          </Button>
        </>
      }
    >
      <div className="qt-settings-layout">
        <nav className="qt-settings-nav" aria-label={t("settings.title")}>
          <button
            type="button"
            aria-selected={tab === "general"}
            onClick={() => setTab("general")}
          >
            <SlidersHorizontal size={16} aria-hidden="true" />
            {t("settings.tabGeneral")}
          </button>
          <button
            type="button"
            aria-selected={tab === "update"}
            onClick={() => setTab("update")}
          >
            <PackageCheck size={16} aria-hidden="true" />
            {t("settings.tabUpdate")}
          </button>
        </nav>

        <div className="qt-settings-content">
          {tab === "general" ? (
            <>
              <SettingRow title={t("settings.intervalTitle")} description={t("settings.intervalHint")}>
                <div className="qt-number-control">
                  <input
                    className="qt-input"
                    type="number"
                    min={1}
                    max={1440}
                    step={1}
                    value={draft.refresh_interval_minutes}
                    onChange={(event) =>
                      setDraft({ ...draft, refresh_interval_minutes: Number(event.target.value) })
                    }
                  />
                  <span>{t("settings.minuteUnit")}</span>
                </div>
              </SettingRow>
              <SettingRow title={t("settings.thresholdTitle")} description={t("settings.thresholdHint")}>
                <div className="qt-number-control">
                  <input
                    className="qt-input"
                    type="number"
                    min={0}
                    max={100}
                    step={1}
                    value={draft.low_balance_threshold_percent}
                    onChange={(event) =>
                      setDraft({ ...draft, low_balance_threshold_percent: Number(event.target.value) })
                    }
                  />
                  <span>%</span>
                </div>
              </SettingRow>
              <SettingRow title={t("settings.autostart")} description={t("settings.autostartHint")}>
                <Switch
                  label={t("settings.autostart")}
                  checked={draft.autostart}
                  onChange={(autostart) => setDraft({ ...draft, autostart })}
                />
              </SettingRow>
              <SettingRow title={t("settings.ringUnits")} description={t("settings.ringUnitsHint")}>
                <input
                  className="qt-input"
                  type="number"
                  min={1}
                  step="any"
                  value={draft.ring_units_per_circle}
                  onChange={(event) =>
                    setDraft({ ...draft, ring_units_per_circle: Number(event.target.value) })
                  }
                />
              </SettingRow>
            </>
          ) : (
            <>
              <div className={`qt-update-status ${
                updateStatus === "error"
                  ? "has-error"
                  : updateStatus === "available"
                    ? "has-update"
                    : "is-current"
              }`}>
                <span className="qt-update-status-icon">
                  {updateStatus === "error"
                    ? <AlertTriangle size={17} aria-hidden="true" />
                    : <Check size={17} aria-hidden="true" />}
                </span>
                <div>
                  <h3>
                    {updateStatus === "checking"
                      ? t("settings.checking")
                      : updateStatus === "available" && available
                        ? t("settings.updateAvailable", { version: available.version })
                        : updateStatus === "error"
                          ? t("settings.updateCheckFailed")
                          : t("settings.upToDate")}
                  </h3>
                  <p>
                    QuotaTray {update?.current_version ?? "…"} · {update?.last_check
                      ? relativeTime(update.last_check, lang)
                      : t("settings.neverChecked")}
                  </p>
                </div>
                <Button
                  disabled={checkNow.isPending}
                  onClick={() => {
                    setDownloadedPath(null);
                    download.reset();
                    checkNow.mutate();
                  }}
                >
                  {t("settings.checkNow")}
                </Button>
              </div>
              <SettingRow title={t("settings.updateEnabledTitle")} description={t("settings.updateEnabledHint")}>
                <Switch
                  label={t("settings.updateEnabledTitle")}
                  checked={draft.update_check_enabled}
                  onChange={(update_check_enabled) => setDraft({ ...draft, update_check_enabled })}
                />
              </SettingRow>
              <SettingRow title={t("settings.updateTimeTitle")} description={t("settings.updateTimeHint")}>
                <input
                  className="qt-input"
                  type="text"
                  value={draft.update_check_time}
                  onChange={(event) => setDraft({ ...draft, update_check_time: event.target.value })}
                />
              </SettingRow>
              {available?.downloadable && (
                <Button
                  variant="primary"
                  disabled={download.isPending}
                  onClick={() => {
                    setDownloadedPath(null);
                    checkNow.reset();
                    download.mutate();
                  }}
                >
                  {download.isPending ? t("settings.downloading") : t("settings.download")}
                </Button>
              )}
              {available && !available.downloadable && (
                <a
                  className="qt-settings-manual-link"
                  href={available.html_url}
                  target="_blank"
                  rel="noreferrer"
                >
                  <ExternalLink size={14} aria-hidden="true" />
                  {t("settings.manualUrl", { url: available.html_url })}
                </a>
              )}
              {downloadedPath && <p className="qt-settings-success">{t("settings.downloaded", { path: downloadedPath })}</p>}
              {operationError && <p className="qt-inline-error">{operationError}</p>}
            </>
          )}
          {save.isError && <p className="qt-inline-error">{String(save.error)}</p>}
        </div>
      </div>
    </DialogShell>
  );
}
