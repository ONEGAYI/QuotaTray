// React Query hooks：轮询调度（GUI-spec §3 查询调度在前端实现）。
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type { UpdateStateDto } from "./types";

export const PROVIDERS_CHANGED_EVENT = "providers-changed";
/** 条目重排事件：各条目数据未变，只失效列表缓存（与后端常量成对）。 */
export const PROVIDERS_REORDERED_EVENT = "providers-reordered";

type QueryInvalidator = {
  invalidateQueries: (filters: { queryKey: readonly unknown[] }) => unknown;
};

/** 标题栏主题/语言只属于显示设置，不影响任何 Provider 查询结果。 */
export function invalidateDisplaySettingsCache(qc: QueryInvalidator) {
  void qc.invalidateQueries({ queryKey: ["settings"] });
}

/** Provider 配置变化会影响两个 WebView 各自缓存的列表、查询与快照。
 * entryId 缺省时全量失效（配置导入等所有条目都可能变化的场景）；
 * 单条目变更（新增/编辑/启停/删除）只失效该条目的派生缓存，
 * 其余条目的查询继续沿用各自轮询周期，不陪查。 */
export function invalidateProviderCaches(qc: QueryInvalidator, entryId?: string) {
  void qc.invalidateQueries({ queryKey: ["providers"] });
  if (entryId) {
    void qc.invalidateQueries({ queryKey: ["provider", entryId] });
    void qc.invalidateQueries({ queryKey: ["provider-state", entryId] });
    void qc.invalidateQueries({ queryKey: ["history", entryId] });
  } else {
    void qc.invalidateQueries({ queryKey: ["provider"] });
    void qc.invalidateQueries({ queryKey: ["provider-state"] });
    void qc.invalidateQueries({ queryKey: ["history"] });
  }
  void qc.invalidateQueries({ queryKey: ["snapshots"] });
}

export function useProviders() {
  const qc = useQueryClient();
  useEffect(() => {
    const unlistenProviders = listen<string>(PROVIDERS_CHANGED_EVENT, (event) => {
      invalidateProviderCaches(qc, event.payload);
    });
    // 重排只动顺序不动数据：仅列表失效，派生缓存继续沿用各自轮询周期
    const unlistenReordered = listen(PROVIDERS_REORDERED_EVENT, () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
    });
    const unlistenImport = listen<number>("configuration-imported", () => {
      invalidateProviderCaches(qc);
      void qc.invalidateQueries({ queryKey: ["native-metas"] });
      void qc.invalidateQueries({ queryKey: ["settings"] });
    });
    return () => {
      void Promise.all([unlistenProviders, unlistenReordered, unlistenImport]).then((unlisten) => {
        unlisten.forEach((fn) => fn());
      });
    };
  }, [qc]);
  return useQuery({
    queryKey: ["providers"],
    queryFn: api.listProviders,
  });
}

export function useSettings() {
  return useQuery({
    queryKey: ["settings"],
    queryFn: api.getSettings,
  });
}

/** 单条目轮询：默认 5 分钟，后台（托盘态）继续刷新。
 * staleTime 对齐轮询周期：周期内的重挂载、窗口恢复聚焦、网络重连
 * 都不追加查询——刷新只由首次取数、轮询到点、手动刷新与条目变更/
 * 导入失效触发；周期外的恢复聚焦由框架默认 refetchOnWindowFocus
 * 补查一次，时点已贴近下次轮询，视为轮询的提前量而非独立刷新。 */
export function useProviderQuery(id: string, enabled: boolean, intervalMinutes: number) {
  const intervalMs = intervalMinutes * 60_000;
  return useQuery({
    queryKey: ["provider", id],
    queryFn: () => api.queryProvider(id),
    enabled,
    refetchInterval: intervalMs,
    refetchIntervalInBackground: true,
    staleTime: intervalMs,
    // 错误双轨已在 QueryOutcome 内表达，不启用框架层重试
    retry: false,
  });
}

/** 单条共享结果只读视图：不发网络请求，供独立悬停 WebView 消费。 */
export function useProviderState(id: string, enabled: boolean) {
  return useQuery({
    queryKey: ["provider-state", id],
    queryFn: () => api.getProviderState(id),
    enabled,
    retry: false,
  });
}

export function historyFromNow(spanMs: number, nowMs: number = Date.now()): number {
  return Math.max(0, nowMs - spanMs);
}

/** 单条 Provider 的本地历史；成功查询事件到达后立即刷新新落库点。 */
export function useHistory(id: string, spanMs: number) {
  const qc = useQueryClient();
  useEffect(() => {
    if (!id) return;
    const unlisten = listen<string>("provider-state-changed", (event) => {
      if (event.payload === id) {
        void qc.invalidateQueries({ queryKey: ["history", id] });
      }
    }).catch((err) => {
      console.error("provider-state-changed 历史失效监听失败：", err);
      return () => {};
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [id, qc]);

  return useQuery({
    queryKey: ["history", id, spanMs],
    queryFn: () => api.getHistory(id, historyFromNow(spanMs)),
    enabled: id.length > 0,
    staleTime: 60_000,
  });
}

/** 多 Provider 本地历史；返回顺序与 ids 一致，供统计比较页聚合候选与曲线。 */
export function useHistories(ids: string[], spanMs: number) {
  const qc = useQueryClient();
  useEffect(() => {
    const unlisten = listen<string>("provider-state-changed", (event) => {
      if (ids.includes(event.payload)) {
        void qc.invalidateQueries({ queryKey: ["history", event.payload] });
      }
    }).catch((err) => {
      console.error("provider-state-changed 多历史失效监听失败：", err);
      return () => {};
    });
    return () => { void unlisten.then((fn) => fn()); };
  }, [ids, qc]);

  return useQueries({
    queries: ids.map((id) => ({
      queryKey: ["history", id, spanMs],
      queryFn: () => api.getHistory(id, historyFromNow(spanMs)),
      enabled: id.length > 0,
      staleTime: 60_000,
    })),
  });
}

/** 后端任一查询完成后刷新对应共享结果视图。 */
export function useProviderStateEvents() {
  const qc = useQueryClient();
  useEffect(() => {
    const unlisten = listen<string>("provider-state-changed", (event) => {
      void qc.invalidateQueries({ queryKey: ["provider-state", event.payload] });
    }).catch((err) => {
      console.error("provider-state-changed 事件监听失败：", err);
      return () => {};
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [qc]);
}

export function useNativeMetas() {
  return useQuery({
    queryKey: ["native-metas"],
    queryFn: api.listNativeMetas,
    // 自定义模型库可由 CLI 修改；窗口重新聚焦后最多沿用 30 秒旧元信息。
    staleTime: 30_000,
  });
}

/** 启动快照（spec §5：首屏先渲染上次成功结果，消除重启空窗）。 */
export function useSnapshots() {
  return useQuery({
    queryKey: ["snapshots"],
    queryFn: api.getSnapshots,
    // 快照只在重启后有意义；条目变更时由调用方主动失效
    staleTime: Infinity,
  });
}

/** 启动状态（首屏分支：便携首启确认页；确认成功后由调用方 refetch）。 */
export function useBootState() {
  return useQuery({
    queryKey: ["boot-state"],
    queryFn: api.getBootState,
    staleTime: Infinity,
  });
}

/** 更新检测状态（设置页「更新」分页展示；自动调度完成后由后端事件即时推送）。 */
export function useUpdateState() {
  const qc = useQueryClient();
  useEffect(() => {
    const unlisten = listen<UpdateStateDto>("update-state-changed", (event) => {
      qc.setQueryData(["update-state"], event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [qc]);
  return useQuery({
    queryKey: ["update-state"],
    queryFn: api.getUpdateState,
    staleTime: 30_000,
  });
}

/** 托盘「立即刷新」/ 悬停触发的事件：全量失效各条目查询。 */
export function useRefreshNow() {
  const qc = useQueryClient();
  useEffect(() => {
    const unlisten = listen("refresh-now", () => {
      void qc.invalidateQueries({ queryKey: ["provider"] });
    }).catch((err) => {
      // capabilities 缺失时 listen 会被 ACL 拒绝——显式暴露而非静默吞掉
      console.error("refresh-now 事件监听失败：", err);
      return () => {};
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [qc]);
}

/** 峰谷翻转感知时刻（#15）：后端每分钟检测到任一条目峰/谷翻转时广播，
 * payload 为后端 epoch 毫秒。常驻 WebView（悬停面板/主窗卡片）的峰谷
 * 标签锚定渲染时刻的 Date.now()，无重渲染即停留在旧判定——本 hook
 * 以事件驱动一次重渲染，与托盘菜单同一 tick 判定；初始值为挂载时刻。 */
export function usePeakFlipTick() {
  const [tick, setTick] = useState(() => Date.now());
  useEffect(() => {
    const unlisten = listen<number>("peak-flip", (event) => {
      setTick(event.payload);
    }).catch((err) => {
      console.error("peak-flip 事件监听失败：", err);
      return () => {};
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);
  return tick;
}
