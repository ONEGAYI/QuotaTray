// 主窗口：供应商列表 + 添加/设置入口。
// 关闭窗口 = 隐藏收托盘（Rust 侧处理），React 不参与退出逻辑。
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { EditDialog } from "./components/EditDialog";
import { ProviderCard } from "./components/ProviderCard";
import { SettingsDialog } from "./components/SettingsDialog";
import { useProviders, useRefreshNow, useSettings, useSnapshots } from "./queries";
import type { ProviderEntry } from "./types";

const queryClient = new QueryClient();

function AppInner() {
  useRefreshNow();
  const providers = useProviders();
  const settings = useSettings();
  const snapshots = useSnapshots();
  const [editOpen, setEditOpen] = useState(false);
  const [editing, setEditing] = useState<ProviderEntry | null>(null);
  const [dialogSeq, setDialogSeq] = useState(0); // 新增对话框每次打开重置表单
  const [settingsOpen, setSettingsOpen] = useState(false);

  const intervalMinutes = settings.data?.refresh_interval_minutes ?? 5;
  const threshold = settings.data?.low_balance_threshold_percent ?? 80;

  return (
    <div className="min-h-screen bg-slate-50 text-slate-900">
      <header className="flex items-center gap-3 border-b border-slate-200 bg-white px-5 py-3">
        <h1 className="text-base font-semibold">QuotaTray</h1>
        <span className="text-xs text-slate-400">AI 账户余额监视</span>
        <div className="ml-auto flex gap-2">
          <button
            onClick={() => {
              setEditing(null);
              setDialogSeq((s) => s + 1);
              setEditOpen(true);
            }}
            className="rounded bg-indigo-600 px-3 py-1.5 text-sm text-white hover:bg-indigo-500"
          >
            + 添加
          </button>
          <button
            onClick={() => setSettingsOpen(true)}
            className="rounded border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
          >
            设置
          </button>
        </div>
      </header>

      <main className="mx-auto max-w-2xl space-y-3 p-5">
        {providers.isLoading && <p className="text-sm text-slate-400">加载中…</p>}
        {providers.isError && (
          <p className="rounded bg-red-50 px-4 py-3 text-sm text-red-600">
            配置读取失败：{String(providers.error)}
          </p>
        )}
        {providers.data != null && providers.data.length === 0 && (
          <div className="rounded-lg border border-dashed border-slate-300 bg-white p-10 text-center">
            <p className="text-sm text-slate-500">还没有供应商条目</p>
            <p className="mt-1 text-xs text-slate-400">
              点击右上角「添加」接入预置平台，或用模板 JSON 接入任意平台
            </p>
          </div>
        )}
        {(providers.data ?? []).map((entry) => (
          <ProviderCard
            key={entry.id}
            entry={entry}
            intervalMinutes={intervalMinutes}
            thresholdPercent={threshold}
            snapshot={snapshots.data?.[entry.id]}
            onEdit={(e) => {
              setEditing(e);
              setEditOpen(true);
            }}
          />
        ))}
      </main>

      <EditDialog
        open={editOpen}
        initial={editing}
        onClose={() => setEditOpen(false)}
        key={editing?.id ?? `new-${dialogSeq}`}
      />
      <SettingsDialog open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <AppInner />
    </QueryClientProvider>
  );
}
