// 主窗口：供应商列表 + 添加/设置入口。
// 关闭窗口 = 隐藏收托盘（Rust 侧处理），React 不参与退出逻辑。
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { EditDialog } from "./components/EditDialog";
import { ProviderCard } from "./components/ProviderCard";
import { SettingsDialog } from "./components/SettingsDialog";
import { TitleBar } from "./components/TitleBar";
import { LangProvider, useLang } from "./i18n";
import { ThemeProvider } from "./theme";
import { useProviders, useRefreshNow, useSettings, useSnapshots } from "./queries";
import type { ProviderEntry } from "./types";

const queryClient = new QueryClient();

function AppInner() {
  useRefreshNow();
  const { t } = useLang();
  const providers = useProviders();
  const settings = useSettings();
  const snapshots = useSnapshots();
  const [editOpen, setEditOpen] = useState(false);
  const [editing, setEditing] = useState<ProviderEntry | null>(null);
  // 每次打开/关闭都递增：新增与编辑共用，取消后重开不残留上次中间态
  // （含 key 输入框——残留会导致"只想改名"的保存把放弃的 key 一并写入）
  const [dialogSeq, setDialogSeq] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);

  const intervalMinutes = settings.data?.refresh_interval_minutes ?? 5;
  const threshold = settings.data?.low_balance_threshold_percent ?? 80;

  return (
    <div className="min-h-screen bg-slate-50 text-slate-900 dark:bg-slate-900 dark:text-slate-100">
      {/* 无装饰窗口：自定义标题栏承载应用标识、语言/主题快捷切换与窗口控制 */}
      <TitleBar />
      <header className="flex items-center justify-end gap-2 border-b border-slate-200 bg-white px-5 py-2.5 dark:border-slate-700 dark:bg-slate-800">
        <button
          onClick={() => {
            setEditing(null);
            setDialogSeq((s) => s + 1);
            setEditOpen(true);
          }}
          className="rounded bg-indigo-600 px-3 py-1.5 text-sm text-white hover:bg-indigo-500 dark:bg-indigo-500 dark:hover:bg-indigo-400"
        >
          {t("app.add")}
        </button>
        <button
          onClick={() => setSettingsOpen(true)}
          className="rounded border border-slate-300 px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100 dark:border-slate-600 dark:text-slate-300 dark:hover:bg-slate-700"
        >
          {t("app.settings")}
        </button>
      </header>

      <main className="mx-auto max-w-2xl space-y-3 p-5">
        {providers.isLoading && (
          <p className="text-sm text-slate-400 dark:text-slate-500">{t("app.loading")}</p>
        )}
        {providers.isError && (
          <p className="rounded bg-red-50 px-4 py-3 text-sm text-red-600 dark:bg-red-950/40 dark:text-red-400">
            {t("app.configError", { msg: String(providers.error) })}
          </p>
        )}
        {providers.data != null && providers.data.length === 0 && (
          <div className="rounded-lg border border-dashed border-slate-300 bg-white p-10 text-center dark:border-slate-600 dark:bg-slate-800/60">
            <p className="text-sm text-slate-500 dark:text-slate-400">{t("app.emptyTitle")}</p>
            <p className="mt-1 text-xs text-slate-400 dark:text-slate-500">
              {t("app.emptyHint")}
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
              setDialogSeq((s) => s + 1);
              setEditOpen(true);
            }}
          />
        ))}
      </main>

      <EditDialog
        open={editOpen}
        initial={editing}
        onClose={() => {
          setEditOpen(false);
          setDialogSeq((s) => s + 1); // 关闭即作废当前表单态，重开从 initial 重建
        }}
        key={`${editing?.id ?? "new"}-${dialogSeq}`}
      />
      <SettingsDialog open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <LangProvider>
        <ThemeProvider>
          <AppInner />
        </ThemeProvider>
      </LangProvider>
    </QueryClientProvider>
  );
}
