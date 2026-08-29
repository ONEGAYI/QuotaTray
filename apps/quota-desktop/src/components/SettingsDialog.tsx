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
  savedApkIsCurrent,
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
  mobile?: boolean;
}

type Tab = "general" | "update" | "data";
type TransferFeedback = { kind: "success" | "error"; text: string };

export function SettingsDialog({ open, onClose, mobile = false }: Props) {
  const qc = useQueryClient();
  const { t, lang } = useLang();
  const settings = useSettings();
  const updateState = useUpdateState();
  const [tab, setTab] = useState<Tab>("general");
  const [draft, setDraft] = useState<Settings | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<DownloadProgress | null>(null);
  const [transferFeedback, setTransferFeedback] = useState<TransferFeedback | null>(null);
  const [clearOpen, setClearOpen] = useState(false);
  /** Android：SAF 保存的 APK 位置（content:// URI，会话内存——后端状态表
   * 不记录，离开页面丢失后重下即可）。附带下载时的可用版本快照：
   * 版本不一致（重检测后发现新版本）时自动失效，同版本重复检测不清空，
   * 避免 18MB 白重下（2026-08-29 审查修复）。 */
  const [savedApk, setSavedApk] = useState<{ uri: string; version: string | null } | null>(null);
  /** Android：open_downloaded_apk 返回 false（系统无安装器）后的降级引导。 */
  const [installFallback, setInstallFallback] = useState(false);
  /** Android：授权页入口反馈——"unsupported" = API 26 以下无该设置页；
   * "error" = 桥故障（JNI/类加载失败等），两者的用户出路一致。 */
  const [consentFeedback, setConsentFeedback] = useState<"unsupported" | "error" | null>(null);

  const portableRun = updateState.data?.portable ?? false;
  const manualUpdateRun = updateState.data?.manual_update ?? portableRun;
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
    onSuccess: () => {
      // 已保存 APK 的失效由版本快照对比自动处理（见 savedApk 注释），
      // 无条件清空会在同版本重检测时误伤 18MB 已下载产物
      setInstallFallback(false);
      void qc.invalidateQueries({ queryKey: ["update-state"] });
    },
  });

  const download = useMutation({
    mutationFn: api.downloadUpdate,
    // 已下载路径由后端状态表记录，失效缓存即刷新出「立即安装」入口
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["update-state"] }),
  });

  /** Android：下载 APK 写入 SAF 位置（先经 saveDialog 拿 content:// URI）。 */
  const downloadToUri = useMutation({
    mutationFn: api.downloadUpdateToUri,
    onSuccess: (_data, path) => {
      setSavedApk({ uri: path, version: updateState.data?.available?.version ?? null });
      setInstallFallback(false);
    },
  });

  /** Android：以系统安装器打开已保存的 APK；false = 无安装器降级引导。 */
  const openApk = useMutation({
    mutationFn: api.openDownloadedApk,
    onSuccess: (dispatched) => {
      if (!dispatched) setInstallFallback(true);
    },
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
    // Android 的系统文档选择器按 MIME 类型过滤；tauri-plugin-dialog 仍复用
    // extensions 字段传递该值。桌面端继续使用真实扩展名。
    const path = await saveDialog({
      title: t("settings.exportDialogTitle"),
      defaultPath: defaultTransferFileName(new Date()),
      filters: [{
        name: t("settings.transferDialogFilter"),
        extensions: mobile ? ["application/octet-stream"] : ["qtray-export"],
      }],
    });
    if (path) exportConfiguration.mutate(mobile ? path : ensureTransferExtension(path));
  };

  const beginImport = async () => {
    setTransferFeedback(null);
    // 与导出同口径：Android SAF 需要 MIME，桌面文件选择器需要扩展名。
    const path = await openDialog({
      title: t("settings.importDialogTitle"),
      multiple: false,
      directory: false,
      filters: [{
        name: t("settings.transferDialogFilter"),
        extensions: mobile ? ["application/octet-stream"] : ["qtray-export"],
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

  /** Android 下载入口：SAF 保存对话框拿 content:// 位置，再交给后端
   * 下载写入（MIME 过滤与配置导出同模式；桌面不渲染此入口）。 */
  const beginDownloadApk = async () => {
    setInstallFallback(false);
    const uri = await saveDialog({
      title: t("settings.apkDialogTitle"),
      defaultPath: available?.asset_name ?? undefined,
      filters: [{
        name: t("settings.apkDialogFilter"),
        extensions: ["application/vnd.android.package-archive"],
      }],
    });
    if (uri) downloadToUri.mutate(uri);
  };

  // 手动检测口径的「进页自动检一次」：进入更新页且距上次检测超过
  // 5 分钟（core POLL_INTERVAL_MS 同款节流）时触发。仅移动端：移动端
  // 无调度器，这是更新信息的唯一自动刷新来源；桌面有轮询调度器，
  // 用户关闭「自动检测」开关的意图不能被进页检测违背（2026-08-29
  // 审查修复：此前无平台守卫，桌面关开关后 last_check 停更导致每次
  // 进页仍联网检测）
  useEffect(() => {
    if (!mobile || tab !== "update" || checkNow.isPending || downloadToUri.isPending) return;
    const last = updateState.data?.last_check ?? null;
    const elapsed = last ? Date.now() - last : Number.POSITIVE_INFINITY;
    if (elapsed >= 5 * 60 * 1000) checkNow.mutate();
    // updateState.data 变化会重触发；节流窗口内 last_check 已更新即停
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, updateState.data?.last_check]);

  if (!open || !draft) return null;
  const update = updateState.data;
  const available = update?.available ?? null;
  // 版本快照一致的已保存 APK 才可用（重检测出新版本自动失效）
  const savedApkUri = savedApkIsCurrent(savedApk, available?.version ?? null)
    ? savedApk.uri
    : null;
  const downloadedPath = update?.downloaded_path ?? null;
  const operationError = resolveUpdateError({
    checkError: checkNow.isError ? checkNow.error : null,
    downloadError: download.isError
      ? download.error
      : downloadToUri.isError
        ? downloadToUri.error
        : null,
    installError: install.isError
      ? install.error
      : openApk.isError
        ? openApk.error
        : null,
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
  const downloading = download.isPending || downloadToUri.isPending;
  const updateAction = resolveUpdateAction({
    downloading,
    canDownload,
    hasDownloaded: downloadedPath != null,
    manualUpdate: manualUpdateRun,
    mobileSaved: mobile && savedApkUri != null,
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
              {mobile && (
                <SettingRow title={t("titlebar.language")} description={t("settings.mobileLanguageHint")}>
                  <select
                    className="qt-select"
                    value={draft.language}
                    onChange={(event) => setDraft({ ...draft, language: event.target.value })}
                  >
                    <option value="zh">{t("settings.langZh")}</option>
                    <option value="en">{t("settings.langEn")}</option>
                    <option value="system">{t("settings.langSystem")}</option>
                  </select>
                </SettingRow>
              )}
              {mobile && (
                <SettingRow title={t("titlebar.theme")} description={t("settings.mobileThemeHint")}>
                  <select
                    className="qt-select"
                    value={draft.theme}
                    onChange={(event) => setDraft({ ...draft, theme: event.target.value })}
                  >
                    <option value="light">{t("settings.themeLight")}</option>
                    <option value="dark">{t("settings.themeDark")}</option>
                    <option value="system">{t("settings.themeSystem")}</option>
                  </select>
                </SettingRow>
              )}
              {mobile && (
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
                      setDraft({
                        ...draft,
                        update_proxy_port:
                          raw === "" || !Number.isFinite(parsed)
                            ? null
                            : Math.min(65535, Math.max(1, Math.round(parsed))),
                      });
                    }}
                  />
                </SettingRow>
              )}
              {!mobile && <SettingRow
                title={t("settings.autostart")}
                description={portableRun ? t("settings.autostartPortableHint") : t("settings.autostartHint")}
              >
                <Switch
                  label={t("settings.autostart")}
                  checked={portableRun ? false : draft.autostart}
                  disabled={portableRun}
                  onChange={(autostart) => setDraft({ ...draft, autostart })}
                />
              </SettingRow>}
              {!mobile && <SettingRow title={t("settings.ringUnits")} description={t("settings.ringUnitsHint")}>
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
              </SettingRow>}
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
                  variant={updateAction === "install" || updateAction === "install-mobile" || updateAction === "download" ? "primary" : undefined}
                  disabled={checkNow.isPending || downloading || install.isPending || openApk.isPending}
                  onClick={() => {
                    if (updateAction === "install-mobile") {
                      // Android：content URI 留存前端会话，拉起系统安装器
                      // 由用户确认（Ok(false) 时降级手动引导）。清其余槽位
                      // 错误，避免旧失败残留误导本次操作
                      checkNow.reset();
                      downloadToUri.reset();
                      if (savedApkUri) openApk.mutate(savedApkUri);
                      return;
                    }
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
                      // 移动端三槽位（桌面槽位以外的 Android 变体）同批清理
                      downloadToUri.reset();
                      openApk.reset();
                      if (mobile) {
                        void beginDownloadApk();
                      } else {
                        download.mutate();
                      }
                    } else {
                      download.reset();
                      install.reset();
                      downloadToUri.reset();
                      openApk.reset();
                      checkNow.mutate();
                    }
                  }}
                >
                  {updateAction === "downloading"
                    ? t("settings.downloading")
                    : updateAction === "install"
                      ? t("settings.install")
                      : updateAction === "install-mobile"
                        ? t("settings.installApk")
                        : updateAction === "open-dir"
                          ? t("settings.openDownloadDir")
                          : updateAction === "download"
                            ? manualUpdateRun
                              ? t("settings.downloadPackage")
                              : t("settings.download")
                            : t("settings.checkNow")}
                </Button>
              </div>
              {downloading && downloadProgress && (
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
              {/* 自动检测/自动下载为桌面调度语义（轮询调度器与自动下载
                  联动不移植移动端）；移动端隐藏，代理端口保留通用 */}
              {!mobile && (
                <SettingRow title={t("settings.updateEnabledTitle")} description={t("settings.updateEnabledHint")}>
                  <Switch
                    label={t("settings.updateEnabledTitle")}
                    checked={draft.update_check_enabled}
                    onChange={(update_check_enabled) => setDraft({ ...draft, update_check_enabled })}
                  />
                </SettingRow>
              )}
              {!mobile && (
                <SettingRow
                  title={t("settings.updateAutoDownloadTitle")}
                  description={t("settings.updateAutoDownloadHint")}
                >
                  <Switch
                    label={t("settings.updateAutoDownloadTitle")}
                    checked={draft.update_auto_download}
                    onChange={(update_auto_download) => setDraft({ ...draft, update_auto_download })}
                  />
                </SettingRow>
              )}
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
              {mobile && savedApkUri && (
                <p className="qt-settings-success">
                  {t("settings.downloadedApk", { name: available?.asset_name ?? "APK" })}
                </p>
              )}
              {mobile && savedApkUri && (
                <p className="qt-settings-manual-hint">
                  {t("settings.installConsentHint")}{" "}
                  <button
                    type="button"
                    className="qt-inline-link"
                    onClick={() => {
                      setConsentFeedback(null);
                      // false = API 26 以下无该设置页（Kotlin 侧版本门）
                      void api
                        .openInstallConsent()
                        .then((dispatched) => {
                          if (!dispatched) setConsentFeedback("unsupported");
                        })
                        .catch((e) => {
                          console.error("打开安装授权页失败", e);
                          setConsentFeedback("error");
                        });
                    }}
                  >
                    {t("settings.installConsentOpen")}
                  </button>
                </p>
              )}
              {mobile && consentFeedback === "unsupported" && (
                <p className="qt-inline-error">{t("settings.installConsentUnsupported")}</p>
              )}
              {mobile && consentFeedback === "error" && (
                <p className="qt-inline-error">{t("settings.installConsentFailed")}</p>
              )}
              {installFallback && (
                <p className="qt-inline-error">{t("settings.noInstaller")}</p>
              )}
              {downloadedPath && (
                <p className="qt-settings-success">
                  {portableRun
                    ? t("settings.downloadedPortable", { path: downloadedPath })
                    : manualUpdateRun
                      ? t("settings.downloadedArchive", { path: downloadedPath })
                      : t("settings.downloaded", { path: downloadedPath })}
                </p>
              )}
              {operationError && (
                <p className="qt-inline-error">
                  {operationError}
                  {operationErrorDetail && (
                    <span
                      className="qt-error-detail-icon is-multiline"
                      data-tooltip={operationErrorDetail}
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
        onConfirm={async () => {
          await api.clearAllData();
          // 清空影响全部数据面：全量失效（条目/快照/历史/悬停面板缓存）
          await qc.invalidateQueries();
        }}
      />
    </DialogShell>
  );
}
