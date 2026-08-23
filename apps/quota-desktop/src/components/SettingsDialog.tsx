// 设置对话框（聚类分页：常规 / 更新）。
// - 常规：刷新间隔 / 低额度阈值 / 开机自启 / 托盘圆环每圈单位；
// - 更新：自动检测开关、每日检测时刻（随「保存」落盘）+ 版本信息与
//   「立即检查 / 下载安装包」即时操作（不进 draft，走独立 IPC）。
// 语言与主题三态设置在自定义标题栏的快捷菜单中（TitleBar.tsx）。
// 保存走既有 save_settings 链路（磁盘权威）。
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api } from "../api";
import { relativeTime } from "../display";
import { useLang } from "../i18n";
import { useSettings, useUpdateState } from "../queries";
import type { Settings } from "../types";

interface Props {
  open: boolean;
  onClose: () => void;
}

type Tab = "general" | "update";

const inputCls =
  "w-24 rounded border border-slate-300 bg-white px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-slate-100";
const labelCls = "text-sm text-slate-600 dark:text-slate-300";
const tabCls = (active: boolean) =>
  "rounded-t px-4 py-1.5 text-sm transition-colors " +
  (active
    ? "bg-white font-medium text-indigo-600 dark:bg-slate-800 dark:text-indigo-300"
    : "text-slate-500 hover:text-slate-700 dark:text-slate-400 dark:hover:text-slate-200");

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

  // 打开设置页时刷新检测状态（后台调度可能已更新，staleTime 内不会自动重取）
  useEffect(() => {
    if (open) void qc.invalidateQueries({ queryKey: ["update-state"] });
  }, [open, qc]);

  const save = useMutation({
    mutationFn: (s: Settings) => api.saveSettings(s),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["settings"] });
      void qc.invalidateQueries({ queryKey: ["provider"] }); // 间隔变化即时生效
      onClose();
    },
  });

  // 「立即检查」：手动检测不受节流限制，检测结果同步托盘菜单
  const checkNow = useMutation({
    mutationFn: api.checkUpdateNow,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["update-state"] });
    },
  });

  // 「下载安装包」：Rust 侧下载到系统下载目录，返回路径
  const download = useMutation({
    mutationFn: api.downloadUpdate,
    onSuccess: (path) => setDownloadedPath(path),
  });

  if (!open) return null;
  if (!draft) return null;

  const us = updateState.data;
  const available = us?.available ?? null;
  // 「最新版本」栏：检测过显示结论；未检测过显示占位符
  const latestText = checkNow.isPending
    ? t("settings.checking")
    : us == null
      ? "—"
      : available
        ? `v${available.version}`
        : us.last_error
          ? t("settings.updateError", { msg: us.last_error })
          : t("settings.upToDate");

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/30 p-4">
      <div className="w-full max-w-md overflow-hidden rounded-lg bg-white shadow-xl dark:bg-slate-800">
        <div className="border-b border-slate-200 px-5 pt-3 dark:border-slate-700">
          <h2 className="font-medium">{t("settings.title")}</h2>
          <div className="mt-2 flex gap-1">
            <button className={tabCls(tab === "general")} onClick={() => setTab("general")}>
              {t("settings.tabGeneral")}
            </button>
            <button className={tabCls(tab === "update")} onClick={() => setTab("update")}>
              {t("settings.tabUpdate")}
            </button>
          </div>
        </div>

        <form
          className="space-y-4 px-5 py-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (draft) save.mutate(draft);
          }}
        >
          {tab === "general" ? (
            <>
              <label className="flex items-center justify-between gap-4">
                <span className={labelCls}>{t("settings.interval")}</span>
                <input
                  type="number"
                  min={1}
                  max={1440}
                  value={draft.refresh_interval_minutes}
                  onChange={(e) =>
                    setDraft({ ...draft, refresh_interval_minutes: Number(e.target.value) })
                  }
                  className={inputCls}
                />
              </label>

              <label className="flex items-center justify-between gap-4">
                <span className={labelCls}>{t("settings.threshold")}</span>
                <input
                  type="number"
                  min={0}
                  max={100}
                  value={draft.low_balance_threshold_percent}
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      low_balance_threshold_percent: Number(e.target.value),
                    })
                  }
                  className={inputCls}
                />
              </label>

              <label className="flex items-center justify-between gap-4">
                <span className={labelCls}>{t("settings.autostart")}</span>
                <input
                  type="checkbox"
                  checked={draft.autostart}
                  onChange={(e) => setDraft({ ...draft, autostart: e.target.checked })}
                  className="h-4 w-4"
                />
              </label>

              <label className="flex items-center justify-between gap-4">
                <span className={labelCls}>{t("settings.ringUnits")}</span>
                <input
                  type="number"
                  min={1}
                  step="any"
                  value={draft.ring_units_per_circle}
                  onChange={(e) =>
                    setDraft({ ...draft, ring_units_per_circle: Number(e.target.value) })
                  }
                  className={inputCls}
                />
              </label>
            </>
          ) : (
            <>
              <label className="flex items-center justify-between gap-4">
                <span className={labelCls}>{t("settings.updateEnabled")}</span>
                <input
                  type="checkbox"
                  checked={draft.update_check_enabled}
                  onChange={(e) =>
                    setDraft({ ...draft, update_check_enabled: e.target.checked })
                  }
                  className="h-4 w-4"
                />
              </label>

              <label className="flex items-center justify-between gap-4">
                <span className={labelCls}>{t("settings.updateTime")}</span>
                <input
                  type="text"
                  placeholder="HH:MM"
                  value={draft.update_check_time}
                  onChange={(e) => setDraft({ ...draft, update_check_time: e.target.value })}
                  className={inputCls + " w-20 text-center"}
                />
              </label>

              {/* 版本信息区（只读，数据来自 update-state 查询） */}
              <div className="rounded border border-slate-200 p-3 text-sm dark:border-slate-600">
                <div className="flex items-center justify-between">
                  <span className={labelCls}>{t("settings.currentVersion")}</span>
                  <span className="font-mono">v{us?.current_version ?? "…"}</span>
                </div>
                <div className="mt-1.5 flex items-center justify-between">
                  <span className={labelCls}>{t("settings.latestVersion")}</span>
                  <span className="font-mono">{latestText}</span>
                </div>
                {available?.notes && (
                  <p className="mt-1.5 text-xs text-slate-500 dark:text-slate-400">
                    {available.notes}
                  </p>
                )}
                <p className="mt-1.5 text-xs text-slate-400 dark:text-slate-500">
                  {us?.last_check
                    ? t("settings.lastCheck", {
                        time: relativeTime(us.last_check, lang),
                      })
                    : t("settings.neverChecked")}
                </p>
              </div>

              {available && !available.downloadable && (
                <p className="break-all text-xs text-slate-500 dark:text-slate-400">
                  {t("settings.manualUrl", { url: available.html_url })}
                </p>
              )}

              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => {
                    setDownloadedPath(null);
                    checkNow.mutate();
                  }}
                  disabled={checkNow.isPending}
                  className="rounded border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 disabled:opacity-50 dark:border-slate-600 dark:text-slate-300 dark:hover:bg-slate-700"
                >
                  {checkNow.isPending ? t("settings.checking") : t("settings.checkNow")}
                </button>
                {available?.downloadable && (
                  <button
                    type="button"
                    onClick={() => download.mutate()}
                    disabled={download.isPending}
                    className="rounded bg-indigo-600 px-3 py-1.5 text-sm text-white hover:bg-indigo-500 disabled:opacity-50 dark:bg-indigo-500 dark:hover:bg-indigo-400"
                  >
                    {download.isPending ? t("settings.downloading") : t("settings.download")}
                  </button>
                )}
              </div>

              {downloadedPath && (
                <p className="break-all text-xs text-emerald-600 dark:text-emerald-400">
                  {t("settings.downloaded", { path: downloadedPath })}
                  <br />
                  {t("settings.runInstaller")}
                </p>
              )}
              {download.isError && (
                <p className="rounded bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-950/40 dark:text-red-400">
                  {String(download.error)}
                </p>
              )}
              {checkNow.isError && (
                <p className="rounded bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-950/40 dark:text-red-400">
                  {String(checkNow.error)}
                </p>
              )}
            </>
          )}

          {save.isError && (
            <p className="rounded bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-950/40 dark:text-red-400">
              {String(save.error)}
            </p>
          )}
        </form>

        <div className="flex justify-end gap-2 border-t border-slate-200 px-5 py-3 dark:border-slate-700">
          <button
            onClick={onClose}
            className="rounded px-4 py-1.5 text-sm text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={() => save.mutate(draft)}
            disabled={save.isPending}
            className="rounded bg-indigo-600 px-4 py-1.5 text-sm text-white hover:bg-indigo-500 disabled:opacity-50 dark:bg-indigo-500 dark:hover:bg-indigo-400"
          >
            {save.isPending ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
