// 设置对话框：刷新间隔 / 低额度阈值 / 开机自启 / 语言（占位）。
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useSettings } from "../queries";
import type { Settings } from "../types";

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SettingsDialog({ open, onClose }: Props) {
  const qc = useQueryClient();
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
      <div className="w-full max-w-md overflow-hidden rounded-lg bg-white shadow-xl">
        <div className="border-b border-slate-200 px-5 py-3">
          <h2 className="font-medium">设置</h2>
        </div>
        <form
          className="space-y-4 px-5 py-4"
          onSubmit={(e) => {
            e.preventDefault();
            if (draft) save.mutate(draft);
          }}
        >
          <label className="flex items-center justify-between gap-4">
            <span className="text-sm text-slate-600">自动刷新间隔（分钟）</span>
            <input
              type="number"
              min={1}
              max={1440}
              value={draft.refresh_interval_minutes}
              onChange={(e) =>
                setDraft({ ...draft, refresh_interval_minutes: Number(e.target.value) })
              }
              className="w-24 rounded border border-slate-300 px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none"
            />
          </label>

          <label className="flex items-center justify-between gap-4">
            <span className="text-sm text-slate-600">低额度提醒阈值（已用 %）</span>
            <input
              type="number"
              min={0}
              max={100}
              value={draft.low_balance_threshold_percent}
              onChange={(e) =>
                setDraft({ ...draft, low_balance_threshold_percent: Number(e.target.value) })
              }
              className="w-24 rounded border border-slate-300 px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none"
            />
          </label>

          <label className="flex items-center justify-between gap-4">
            <span className="text-sm text-slate-600">开机自启</span>
            <input
              type="checkbox"
              checked={draft.autostart}
              onChange={(e) => setDraft({ ...draft, autostart: e.target.checked })}
              className="h-4 w-4"
            />
          </label>

          <label className="flex items-center justify-between gap-4 opacity-40">
            <span className="text-sm text-slate-600">语言</span>
            <select disabled value={draft.language} className="w-24 rounded border border-slate-300 px-2 py-1.5 text-sm">
              <option value="zh">中文</option>
              <option value="en">English</option>
            </select>
          </label>

          {save.isError && (
            <p className="rounded bg-red-50 px-3 py-2 text-sm text-red-600">{String(save.error)}</p>
          )}
        </form>
        <div className="flex justify-end gap-2 border-t border-slate-200 px-5 py-3">
          <button onClick={onClose} className="rounded px-4 py-1.5 text-sm text-slate-600 hover:bg-slate-100">
            取消
          </button>
          <button
            onClick={() => save.mutate(draft)}
            disabled={save.isPending}
            className="rounded bg-indigo-600 px-4 py-1.5 text-sm text-white hover:bg-indigo-500 disabled:opacity-50"
          >
            {save.isPending ? "保存中…" : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}
