// React Query hooks：轮询调度（GUI-spec §3 查询调度在前端实现）。
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";

export function useProviders() {
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

export function useNativeMetas() {
  return useQuery({
    queryKey: ["native-metas"],
    queryFn: api.listNativeMetas,
    staleTime: Infinity,
  });
}

/** 托盘「立即刷新」/ 悬停触发的事件：全量失效各条目查询。 */
export function useRefreshNow() {
  const qc = useQueryClient();
  useEffect(() => {
    const unlisten = listen("refresh-now", () => {
      void qc.invalidateQueries({ queryKey: ["provider"] });
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [qc]);
}
