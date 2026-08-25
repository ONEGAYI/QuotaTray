// React Query hooks：轮询调度（GUI-spec §3 查询调度在前端实现）。
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type { UpdateStateDto } from "./types";

export const PROVIDERS_CHANGED_EVENT = "providers-changed";

type QueryInvalidator = {
  invalidateQueries: (filters: { queryKey: readonly unknown[] }) => unknown;
};

/** Provider 配置变化会影响两个 WebView 各自缓存的列表、查询与快照。 */
export function invalidateProviderCaches(qc: QueryInvalidator) {
  void qc.invalidateQueries({ queryKey: ["providers"] });
  void qc.invalidateQueries({ queryKey: ["provider"] });
  void qc.invalidateQueries({ queryKey: ["provider-state"] });
  void qc.invalidateQueries({ queryKey: ["snapshots"] });
}

export function useProviders() {
  const qc = useQueryClient();
  useEffect(() => {
    const unlistenProviders = listen(PROVIDERS_CHANGED_EVENT, () => {
      invalidateProviderCaches(qc);
    });
    const unlistenImport = listen<number>("configuration-imported", () => {
      invalidateProviderCaches(qc);
      void qc.invalidateQueries({ queryKey: ["native-metas"] });
    });
    return () => {
      void Promise.all([unlistenProviders, unlistenImport]).then((unlisten) => {
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

/** 单条目轮询：默认 5 分钟，后台（托盘态）继续刷新。 */
export function useProviderQuery(id: string, enabled: boolean, intervalMinutes: number) {
  return useQuery({
    queryKey: ["provider", id],
    queryFn: () => api.queryProvider(id),
    enabled,
    refetchInterval: intervalMinutes * 60_000,
    refetchIntervalInBackground: true,
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
