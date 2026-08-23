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

/** 更新检测状态（设置页「更新」分页展示；调度任务检测后由手动/保存动作失效）。 */
export function useUpdateState() {
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
