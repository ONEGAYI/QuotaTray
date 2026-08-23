// 设置对话框：刷新间隔 / 低额度阈值 / 开机自启 / 语言（三态）/ 主题（三态）/
// 托盘圆环每圈单位。保存走既有 save_settings 链路（磁盘权威）。
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import { useSettings } from "../queries";
import type { Settings } from "../types";

interface Props {
  open: boolean;
  onClose: () => void;
}

const inputCls =
  "w-24 rounded border border-slate-300 bg-white px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-slate-100";
const selectCls =
  "w-36 rounded border border-slate-300 bg-white px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-slate-100";
const labelCls = "text-sm text-slate-600 dark:text-slate-300";

export function SettingsDialog({ open, onClose }: Props) {
  const qc = useQueryClient();
  const { t } = useLang();
  const settings = useSettings();
  const [draft, setDraft] = useState<Settings | null>(null);

  useEffect(() => {
    if (open && settings.data) setDraft({ ...settings.data });
  }, [open, settings.data]);

  const save = useMutation({
    mutationFn: (s: Settings) => api.saveSettings(s),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["settings"] });
      void qc.invalidateQueries({ queryKey: ["provider"] }); // 间隔变化即时生效
      onClose();
    },
  });

  if (!open) return null;
  if (!draft) return null;

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/30 p-4">
      <div className="w-full max-w-md overflow-hidden rounded-lg bg-white shadow-xl dark:bg-slate-800">
        <div className="border-b border-slate-200 px-5 py-3 dark:border-slate-700">
          <h2 className="font-medium">{t("settings.title")}</h2>
        </div>
        <form
          className="space-y-4 px-5 py-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (draft) save.mutate(draft);
          }}
        >
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
                setDraft({ ...draft, low_balance_threshold_percent: Number(e.target.value) })
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
            <span className={labelCls}>{t("settings.language")}</span>
            <select
              value={draft.language}
              onChange={(e) => setDraft({ ...draft, language: e.target.value })}
              className={selectCls}
            >
              <option value="zh">{t("settings.langZh")}</option>
              <option value="en">{t("settings.langEn")}</option>
              <option value="system">{t("settings.langSystem")}</option>
            </select>
          </label>

          <label className="flex items-center justify-between gap-4">
            <span className={labelCls}>{t("settings.theme")}</span>
            <select
              value={draft.theme}
              onChange={(e) => setDraft({ ...draft, theme: e.target.value })}
              className={selectCls}
            >
              <option value="light">{t("settings.themeLight")}</option>
              <option value="dark">{t("settings.themeDark")}</option>
              <option value="system">{t("settings.themeSystem")}</option>
            </select>
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
