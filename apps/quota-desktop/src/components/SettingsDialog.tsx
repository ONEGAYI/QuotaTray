import { useMutation, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import {
  confirm as confirmDialog,
  open as openDialog,
  save as saveDialog,
} from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  AlertTriangle,
  Check,
  Database,
  ExternalLink,
  FileDown,
  FileUp,
  PackageCheck,
  SlidersHorizontal,
  Trash2,
} from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { relativeTime } from "../display";
import { useLang } from "../i18n";
import { useSettings, useUpdateState } from "../queries";
import type { DownloadProgress, Settings } from "../types";
import {
  downloadPercent,
  formatDownloadProgress,
  resolveUpdateAction,
  resolveUpdateError,
  resolveUpdateErrorDetail,
  resolveUpdateStatus,
  runtimeLabel,
} from "./settingsView";
import {
  defaultTransferFileName,
  ensureTransferExtension,
  transferErrorMessage,
} from "./configTransferView";
import { ClearConfigDialog } from "./ClearConfigDialog";
import { Button, DialogShell, SettingRow, Switch } from "./ui";

interface Props {
  open: boolean;
  onClose: () => void;
}

type Tab = "general" | "update" | "data";
type TransferFeedback = { kind: "success" | "error"; text: string };

export function SettingsDialog({ open, onClose }: Props) {
  const qc = useQueryClient();
  const { t, lang } = useLang();
  const settings = useSettings();
  const updateState = useUpdateState();
  const [tab, setTab] = useState<Tab>("general");
  const [draft, setDraft] = useState<Settings | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null);
  const [transferFeedback, setTransferFeedback] = useState<TransferFeedback | null>(null);
  const [clearOpen, setClearOpen] = useState(false);

  const portableRun = updateState.data?.portable ?? false;
  useEffect(() => {
    if (open && settings.data) {
      // 便携形态钳制自启动为关：后端硬门禁拒绝开启，draft 与 UI/持久值
      // 保持一致（settings.json 手改为 true 的存量也在此归位）
      const next = { ...settings.data, autostart: portableRun ? false : settings.data.autostart };
      setDraft(next);
    }
  }, [open, settings.data, portableRun]);

  useEffect(() => {
    if (open) void qc.invalidateQueries({ queryKey: ["update-state"] });
  }, [open, qc]);

  useEffect(() => {
    const unlisten = listen<DownloadProgress>("update-download-progress", (event) => {
      setDownloadProgress(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

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
    // 已下载路径由后端状态表记录，失效缓存即刷新出「立即安装」入口
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["update-state"] }),
  });

  const install = useMutation({
    mutationFn: api.installUpdate,
    // 文件丢失等失败场景后端已清记录：刷新状态让按钮回到「下载安装包」
    onError: () => void qc.invalidateQueries({ queryKey: ["update-state"] }),
  });

  const exportConfiguration = useMutation({
    mutationFn: api.exportConfiguration,
    onSuccess: (_, path) => {
      setTransferFeedback({ kind: "success", text: t("settings.exportSuccess", { path }) });
    },
    onError: (error) => {
      setTransferFeedback({ kind: "error", text: transferErrorMessage(error) });
    },
  });

  const importConfiguration = useMutation({
    mutationFn: api.importConfiguration,
    onSuccess: (count) => {
      setTransferFeedback({
        kind: "success",
        text: t("settings.importSuccess", { count: String(count) }),
      });
      void qc.invalidateQueries({ queryKey: ["providers"] });
      void qc.invalidateQueries({ queryKey: ["provider"] });
      void qc.invalidateQueries({ queryKey: ["snapshots"] });
      void qc.invalidateQueries({ queryKey: ["native-metas"] });
    },
    onError: (error) => {
      setTransferFeedback({ kind: "error", text: transferErrorMessage(error) });
    },
  });

  const beginExport = async () => {
    setTransferFeedback(null);
    const confirmed = await confirmDialog(t("settings.exportConfirm"), {
      title: t("settings.transferTitle"),
      kind: "warning",
      okLabel: t("settings.exportConfirmButton"),
      cancelLabel: t("common.cancel"),
    });
    if (!confirmed) return;
    const path = await saveDialog({
      title: t("settings.exportDialogTitle"),
      defaultPath: defaultTransferFileName(new Date()),
      filters: [{
        name: t("settings.transferDialogFilter"),
        extensions: ["qtray-export"],
      }],
    });
    if (path) exportConfiguration.mutate(ensureTransferExtension(path));
  };

  const beginImport = async () => {
    setTransferFeedback(null);
    const path = await openDialog({
      title: t("settings.importDialogTitle"),
      multiple: false,
      directory: false,
      filters: [{
        name: t("settings.transferDialogFilter"),
        extensions: ["qtray-export"],
      }],
    });
    if (!path) return;
    const confirmed = await confirmDialog(t("settings.importConfirm"), {
      title: t("settings.transferTitle"),
      kind: "warning",
      okLabel: t("settings.importConfirmButton"),
      cancelLabel: t("common.cancel"),
    });
    if (confirmed) importConfiguration.mutate(path);
  };

  // 安装会退出应用（NSIS 覆盖安装需先解锁自身文件），确认后再触发
  const beginInstall = async () => {
    const confirmed = await confirmDialog(t("settings.installConfirm"), {
      title: t("settings.installConfirmTitle"),
      kind: "info",
      okLabel: t("settings.installConfirmButton"),
      cancelLabel: t("common.cancel"),
    });
    if (confirmed) install.mutate();
  };

  if (!open || !draft) return null;
  const update = updateState.data;
  const available = update?.available ?? null;
  const downloadedPath = update?.downloaded_path ?? null;
  const operationError = resolveUpdateError({
    checkError: checkNow.isError ? checkNow.error : null,
    downloadError: download.isError ? download.error : null,
    installError: install.isError ? install.error : null,
    backendError: update?.last_error,
    hasAvailable: Boolean(available),
  });
  const operationErrorDetail = resolveUpdateErrorDetail({
    operationError,
    backendError: update?.last_error,
    backendErrorDetail: update?.last_error_detail,
  });
  const updateStatus = resolveUpdateStatus({
    checking: checkNow.isPending,
    hasAvailable: Boolean(available),
    error: operationError,
  });
  const canDownload = Boolean(available?.downloadable);
  const updateAction = resolveUpdateAction({
    downloading: download.isPending,
    canDownload,
    hasDownloaded: downloadedPath != null,
    portable: portableRun,
  });
  const percent = downloadProgress ? downloadPercent(downloadProgress) : null;

  return (
    <DialogShell
      title={t("settings.title")}
      description={t("settings.description")}
      onClose={onClose}
      closeLabel={t("titlebar.close")}
      size="md"
      className="qt-dialog-settings"
      footer={
        tab === "data" ? (
          <Button onClick={onClose}>{t("titlebar.close")}</Button>
        ) : (
          <>
            <Button onClick={onClose}>{t("common.cancel")}</Button>
            <Button variant="primary" disabled={save.isPending} onClick={() => save.mutate(draft)}>
              {save.isPending ? t("common.saving") : t("settings.save")}
            </Button>
          </>
        )
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
          <button
            type="button"
            aria-selected={tab === "data"}
            onClick={() => setTab("data")}
          >
            <Database size={16} aria-hidden="true" />
            {t("settings.tabData")}
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
              <SettingRow
                title={t("settings.autostart")}
                description={portableRun ? t("settings.autostartPortableHint") : t("settings.autostartHint")}
              >
                <Switch
                  label={t("settings.autostart")}
                  checked={portableRun ? false : draft.autostart}
                  disabled={portableRun}
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
          ) : tab === "update" ? (
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
                    {[
                      `QuotaTray ${update?.current_version ?? "…"}`,
                      runtimeLabel(update?.platform, update?.portable, t("settings.portableTag")),
                      update?.last_check
                        ? relativeTime(update.last_check, lang)
                        : t("settings.neverChecked"),
                    ]
                      .filter(Boolean)
                      .join(" · ")}
                  </p>
                </div>
                <Button
                  variant={updateAction === "install" || updateAction === "download" ? "primary" : undefined}
                  disabled={checkNow.isPending || download.isPending || install.isPending}
                  onClick={() => {
                    if (updateAction === "open-dir") {
                      // 便携 v1 手动更新引导：打开下载目录，由用户退出
                      // 应用后解压覆盖（zip 不是安装包，不自动运行）
                      void api.openUpdateDir().catch((e) =>
                        console.error("打开下载目录失败", e),
                      );
                      return;
                    }
                    if (updateAction === "install") {
                      download.reset();
                      checkNow.reset();
                      void beginInstall();
                      return;
                    }
                    setDownloadProgress(
                      updateAction === "download"
                        ? {
                            downloaded_bytes: 0,
                            total_bytes: available?.asset_size ?? null,
                            bytes_per_second: 0,
                          }
                        : null,
                    );
                    if (updateAction === "download") {
                      checkNow.reset();
                      install.reset();
                      download.mutate();
                    } else {
                      download.reset();
                      install.reset();
                      checkNow.mutate();
                    }
                  }}
                >
                  {updateAction === "downloading"
                    ? t("settings.downloading")
                    : updateAction === "install"
                      ? t("settings.install")
                      : updateAction === "open-dir"
                        ? t("settings.openDownloadDir")
                        : updateAction === "download"
                          ? portableRun
                            ? t("settings.downloadPackage")
                            : t("settings.download")
                          : t("settings.checkNow")}
                </Button>
              </div>
              {download.isPending && downloadProgress && (
                <div className="qt-update-download-progress">
                  <div
                    className={`qt-update-progress-track ${percent == null ? "is-indeterminate" : ""}`}
                    role="progressbar"
                    aria-label={t("settings.downloading")}
                    aria-valuemin={percent == null ? undefined : 0}
                    aria-valuemax={percent == null ? undefined : 100}
                    aria-valuenow={percent ?? undefined}
                  >
                    <span style={percent == null ? undefined : { width: `${percent}%` }} />
                  </div>
                  <p>{formatDownloadProgress(downloadProgress)}</p>
                </div>
              )}
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
              <SettingRow title={t("settings.updateProxyPortTitle")} description={t("settings.updateProxyPortHint")}>
                <input
                  className="qt-input"
                  type="number"
                  min={1}
                  max={65535}
                  step={1}
                  value={draft.update_proxy_port ?? ""}
                  onChange={(event) => {
                    const raw = event.target.value;
                    const parsed = Number(raw);
                    // 空/非法输入 → null（直连）；超界收到 1..65535，
                    // 与后端 sanitize 的兜底同语义
                    const port =
                      raw === "" || !Number.isFinite(parsed)
                        ? null
                        : Math.min(65535, Math.max(1, Math.round(parsed)));
                    setDraft({ ...draft, update_proxy_port: port });
                  }}
                />
              </SettingRow>
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
              {downloadedPath && (
                <p className="qt-settings-success">
                  {portableRun
                    ? t("settings.downloadedPortable", { path: downloadedPath })
                    : t("settings.downloaded", { path: downloadedPath })}
                </p>
              )}
              {operationError && (
                <p className="qt-inline-error">
                  {operationError}
                  {operationErrorDetail && (
                    <span
                      className="qt-error-detail-icon"
                      title={operationErrorDetail}
                      aria-label={operationErrorDetail}
                    >
                      <AlertCircle size={13} aria-hidden="true" />
                    </span>
                  )}
                </p>
              )}
            </>
          ) : (
            <>
              <div className="qt-transfer-intro">
                <span className="qt-transfer-intro-icon">
                  <AlertTriangle size={18} aria-hidden="true" />
                </span>
                <div>
                  <h3>{t("settings.transferTitle")}</h3>
                  <p>{t("settings.transferDescription")}</p>
                  <p>{t("settings.transferWarning")}</p>
                </div>
              </div>
              <SettingRow
                title={t("settings.exportTitle")}
                description={t("settings.exportHint")}
              >
                <Button
                  disabled={exportConfiguration.isPending || importConfiguration.isPending}
                  onClick={() => void beginExport()}
                >
                  <FileDown size={15} aria-hidden="true" />
                  {exportConfiguration.isPending
                    ? t("settings.exporting")
                    : t("settings.exportButton")}
                </Button>
              </SettingRow>
              <SettingRow
                title={t("settings.importTitle")}
                description={t("settings.importHint")}
              >
                <Button
                  variant="danger"
                  disabled={exportConfiguration.isPending || importConfiguration.isPending}
                  onClick={() => void beginImport()}
                >
                  <FileUp size={15} aria-hidden="true" />
                  {importConfiguration.isPending
                    ? t("settings.importing")
                    : t("settings.importButton")}
                </Button>
              </SettingRow>
              {transferFeedback && (
                <p className={
                  transferFeedback.kind === "success"
                    ? "qt-settings-success"
                    : "qt-inline-error"
                }>
                  {transferFeedback.text}
                </p>
              )}
              <SettingRow
                title={t("settings.clearTitle")}
                description={t("settings.clearHint")}
              >
                <Button
                  variant="danger"
                  icon={Trash2}
                  onClick={() => setClearOpen(true)}
                >
                  {t("settings.clearButton")}
                </Button>
              </SettingRow>
            </>
          )}
          {save.isError && <p className="qt-inline-error">{String(save.error)}</p>}
        </div>
      </div>
      <ClearConfigDialog
        open={clearOpen}
        onClose={() => setClearOpen(false)}
        onConfirm={() => {
          // 空壳阶段：确认后仅关闭弹窗，实际清空待接线 core 清空命令
          setClearOpen(false);
        }}
      />
    </DialogShell>
  );
}
