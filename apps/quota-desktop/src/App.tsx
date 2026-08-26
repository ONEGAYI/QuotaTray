// 主窗口：供应商列表 + 添加/设置入口。
// 关闭窗口 = 隐藏收托盘（Rust 侧处理），React 不参与退出逻辑。
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Plus, Settings as SettingsIcon } from "lucide-react";
import { useReducer, useState } from "react";
import { EditDialog } from "./components/EditDialog";
import { MainPanelTabs } from "./components/MainPanelTabs";
import { ProviderCard } from "./components/ProviderCard";
import { SettingsDialog } from "./components/SettingsDialog";
import { TitleBar } from "./components/TitleBar";
import { UsageStatsPage } from "./components/UsageStatsPage";
import { LangProvider, useLang } from "./i18n";
import { ThemeProvider } from "./theme";
import { useNativeMetas, useProviders, useRefreshNow, useSettings, useSnapshots } from "./queries";
import type { ProviderEntry } from "./types";
import { Button } from "./components/ui";
import { initialMainPanelState, reduceMainPanelTransition } from "./mainPanelView";

const queryClient = new QueryClient();

function AppInner() {
  useRefreshNow();
  const { t } = useLang();
  const providers = useProviders();
  const settings = useSettings();
  const snapshots = useSnapshots();
  const nativeMetas = useNativeMetas();
  const [editOpen, setEditOpen] = useState(false);
  const [editing, setEditing] = useState<ProviderEntry | null>(null);
  const [editingCurrency, setEditingCurrency] = useState<string | undefined>();
  // 每次打开/关闭都递增：新增与编辑共用，取消后重开不残留上次中间态
  // （含 key 输入框——残留会导致"只想改名"的保存把放弃的 key 一并写入）
  const [dialogSeq, setDialogSeq] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [mainPanel, dispatchMainPanel] = useReducer(
    reduceMainPanelTransition,
    initialMainPanelState,
  );

  const intervalMinutes = settings.data?.refresh_interval_minutes ?? 5;
  const threshold = settings.data?.low_balance_threshold_percent ?? 80;

  return (
    <div className="qt-app-shell">
      <TitleBar />

      <main className="qt-main-content">
        <header className="qt-page-heading">
          <div>
            <MainPanelTabs
              selected={mainPanel.target}
              accountsLabel={t("app.accounts")}
              usageLabel={t("app.usageStats")}
              ariaLabel={t("app.viewTabs")}
              onSelect={(panel) => dispatchMainPanel({ type: "select", panel })}
            />
            <p>
              <span className="qt-page-status-dot" />
              {t("app.accountCount", { count: providers.data?.length ?? 0 })}
            </p>
          </div>
          <div className="qt-page-actions">
            <Button icon={SettingsIcon} onClick={() => setSettingsOpen(true)}>
              {t("app.settings")}
            </Button>
            <Button
              variant="primary"
              icon={Plus}
              onClick={() => {
                setEditing(null);
                setEditingCurrency(undefined);
                setDialogSeq((sequence) => sequence + 1);
                setEditOpen(true);
              }}
            >
              {t("app.addAccount")}
            </Button>
          </div>
        </header>

        <div
          id="qt-main-panel"
          className={`qt-main-panel is-${mainPanel.phase}`}
          role="tabpanel"
          aria-labelledby={`qt-tab-${mainPanel.visible}`}
          onAnimationEnd={(event) => {
            if (event.currentTarget === event.target) {
              dispatchMainPanel({ type: "animation-end" });
            }
          }}
        >
          {/*
           * 两面板常挂载、hidden 切换可见性：卸载重挂载会让全部卡片重走
           * useProviderQuery 并丢失展开/菜单等本地状态，而换页只是显示
           * 操作，不应触发查询。hidden 的翻转时点沿用 visible 契约
           * （最大模糊点替换内容），动画作用于容器不受影响。
           */}
          <div className="qt-panel-page" hidden={mainPanel.visible !== "accounts"}>
            {providers.isLoading && (
              <div className="qt-loading-card">{t("app.loading")}</div>
            )}
            {providers.isError && (
              <p className="qt-inline-error">
                {t("app.configError", { msg: String(providers.error) })}
              </p>
            )}
            {providers.data != null && providers.data.length === 0 && (
              <div className="qt-empty-state">
                <p>{t("app.emptyTitle")}</p>
                <span>{t("app.emptyHint")}</span>
              </div>
            )}
            <div className="qt-provider-list">
              {(providers.data ?? []).map((entry) => {
                const nativeProviderId =
                  entry.kind.type === "native" ? entry.kind.provider : undefined;
                return (
                  <ProviderCard
                    key={entry.id}
                    entry={entry}
                    intervalMinutes={intervalMinutes}
                    thresholdPercent={threshold}
                    snapshot={snapshots.data?.[entry.id]}
                    nativeMeta={
                      nativeProviderId
                        ? nativeMetas.data?.find((meta) => meta.id === nativeProviderId)
                        : undefined
                    }
                    onEdit={(provider, usageCurrency) => {
                      setEditing(provider);
                      setEditingCurrency(usageCurrency);
                      setDialogSeq((sequence) => sequence + 1);
                      setEditOpen(true);
                    }}
                  />
                );
              })}
            </div>
          </div>
          <div className="qt-panel-page" hidden={mainPanel.visible === "accounts"}>
            <UsageStatsPage
              providers={providers.data ?? []}
              providersLoading={providers.isLoading}
              providersError={providers.error}
            />
          </div>
        </div>
      </main>

      <EditDialog
        open={editOpen}
        initial={editing}
        usageCurrency={editingCurrency}
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
