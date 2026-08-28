// 主窗口：供应商列表 + 添加/设置入口。
// 关闭窗口 = 隐藏收托盘（Rust 侧处理），React 不参与退出逻辑。
import { QueryClient, QueryClientProvider, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { Plus, Settings as SettingsIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { api } from "./api";
import { EditDialog } from "./components/EditDialog";
import { PortableInitGate } from "./components/PortableInitGate";
import { MainPanelTabs } from "./components/MainPanelTabs";
import { ProviderCard } from "./components/ProviderCard";
import { SettingsDialog } from "./components/SettingsDialog";
import { TitleBar } from "./components/TitleBar";
import { UsageStatsPage } from "./components/UsageStatsPage";
import type { CenterMessage } from "./components/messageCenterView";
import { mergeMessage, messageId } from "./components/messageCenterView";
import { LangProvider, useLang } from "./i18n";
import { ThemeProvider } from "./theme";
import {
  useBootState,
  useNativeMetas,
  useProviders,
  useRefreshNow,
  useSettings,
  useSnapshots,
} from "./queries";
import type { ProviderEntry } from "./types";
import { Button } from "./components/ui";
import { initialMainPanelState, reduceMainPanelTransition } from "./mainPanelView";
import { useCardDragSort } from "./useCardDragSort";

const queryClient = new QueryClient();

function AppInner() {
  useRefreshNow();
  const { t } = useLang();
  // 同机互斥提示：第二实例启动被 single-instance 拦截后聚焦本窗，
  // 顶部短暂 toast 让用户明白「为什么新点的没有打开」
  const [instanceToast, setInstanceToast] = useState(false);
  // 连续触发（用户连点 exe）时从最后一次起算 3 秒：重设计时器
  const toastTimer = useRef<number | null>(null);
  useEffect(() => {
    const unlisten = listen("instance-already-running", () => {
      setInstanceToast(true);
      if (toastTimer.current != null) window.clearTimeout(toastTimer.current);
      toastTimer.current = window.setTimeout(() => setInstanceToast(false), 3000);
    });
    return () => {
      if (toastTimer.current != null) window.clearTimeout(toastTimer.current);
      void unlisten.then((fn) => fn());
    };
  }, []);
  // 消息中心：后端「更新就绪」广播（自动下载完成 / 重启后探测恢复）入列，
  // 铃铛红点由未读判定驱动；会话级内存态，重启后由后端重新广播恢复
  const [messages, setMessages] = useState<CenterMessage[]>([]);
  const [messageSeen, setMessageSeen] = useState<ReadonlySet<string>>(() => new Set());
  useEffect(() => {
    const unlisten = listen<{ version: string }>("update-ready", (event) => {
      setMessages((prev) =>
        mergeMessage(prev, { kind: "update-ready", version: event.payload.version }),
      );
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);
  const onMessagesSeen = useCallback(() => {
    setMessageSeen((prev) => {
      const next = new Set(prev);
      for (const message of messages) next.add(messageId(message));
      return next;
    });
  }, [messages]);
  const qc = useQueryClient();
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

  const listRef = useRef<HTMLDivElement>(null);
  const providerIds = useMemo(
    () => (providers.data ?? []).map((entry) => entry.id),
    [providers.data],
  );
  // 落库尾随合并：乐观更新即时生效，后端写盘按 300ms 合并——键盘长按
  // 调序（系统按键重复 ~30 次/秒）不再每次 keydown 都触发全量 config
  // 保存 + 托盘重建；快速连续拖拽同理。最后一次的顺序即最终落库序。
  const persistReorderRef = useRef(0);
  useEffect(() => () => window.clearTimeout(persistReorderRef.current), []);
  const commitDragOrder = useCallback(
    (orderedIds: string[]) => {
      const current = providers.data;
      // 与当前列表对不上（拖拽中并发增删）：放弃乐观更新并跳过落库，
      // 等待变更来源自身的刷新给出正确状态（后端集合校验保留作纵深防御）
      if (!current || current.length !== orderedIds.length) return;
      const byId = new Map(current.map((entry) => [entry.id, entry]));
      const reordered = orderedIds
        .map((id) => byId.get(id))
        .filter((entry): entry is ProviderEntry => entry != null);
      if (reordered.length !== orderedIds.length) return;
      qc.setQueryData(["providers"], reordered);
      window.clearTimeout(persistReorderRef.current);
      persistReorderRef.current = window.setTimeout(() => {
        // 落库前核对乐观序仍是当前序：debounce 窗口内列表若被外部替换
        // （GUI/CLI 导入配置等），闭包里的旧序会覆盖新状态——后端集合
        // 校验拦不住「集合相同、顺序不同」（典型：重导本机备份包），
        // 只有此处按序比对才能挡住静默回滚
        const current = qc.getQueryData<ProviderEntry[]>(["providers"]);
        const stillCurrent =
          current != null &&
          current.length === orderedIds.length &&
          current.every((entry, index) => entry.id === orderedIds[index]);
        if (!stillCurrent) return;
        void api.reorderProviders(orderedIds).catch(() => {
          // 后端集合失配（如 CLI 同时删卡）：拉取真实状态恢复
          void qc.invalidateQueries({ queryKey: ["providers"] });
        });
      }, 300);
    },
    [providers.data, qc],
  );
  const dragSort = useCardDragSort({
    containerRef: listRef,
    ids: providerIds,
    onCommit: commitDragOrder,
  });
  const handleEdit = useCallback((provider: ProviderEntry, usageCurrency?: string) => {
    setEditing(provider);
    setEditingCurrency(usageCurrency);
    setDialogSeq((sequence) => sequence + 1);
    setEditOpen(true);
  }, []);

  return (
    <div className="qt-app-shell">
      {instanceToast && (
        <div className="qt-toast" role="status">{t("app.instanceRunning")}</div>
      )}
      <TitleBar messages={messages} messageSeen={messageSeen} onMessagesSeen={onMessagesSeen} />

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
            <div
              ref={listRef}
              className={`qt-provider-list${dragSort.active ? " is-drag-active" : ""}`}
            >
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
                    onEdit={handleEdit}
                    dragHandleProps={dragSort.handleProps(entry.id)}
                    dragShift={
                      dragSort.active && entry.id !== dragSort.dragId
                        ? dragSort.shifts[entry.id] ?? 0
                        : undefined
                    }
                    isDragSource={entry.id === dragSort.dragId}
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

/** 启动分支层：便携首启（portable.key 缺失）先渲染安全确认页，
 * 确认补齐后端后再挂载主界面（期间主界面 hooks 依赖 AppState，
 * 提前挂载只会得到整屏错误）。 */
function BootLayer() {
  const boot = useBootState();
  if (boot.isLoading) return null;
  if (boot.data?.pendingPortableInit) {
    return <PortableInitGate onDone={() => void boot.refetch()} />;
  }
  return <AppInner />;
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <LangProvider>
        <ThemeProvider>
          <BootLayer />
        </ThemeProvider>
      </LangProvider>
    </QueryClientProvider>
  );
}
