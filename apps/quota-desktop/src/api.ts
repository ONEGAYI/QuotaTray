// IPC 封装：全部后端交互收敛于此，组件不直接触碰 invoke。
import { invoke } from "@tauri-apps/api/core";
import type {
  NativeMeta,
  ProviderEntry,
  QueryOutcome,
  Settings,
  SnapshotEntry,
} from "./types";

export const api = {
  listProviders: (): Promise<ProviderEntry[]> => invoke("list_providers"),
  upsertProvider: (
    entry: ProviderEntry,
    newApiKey: string | null,
  ): Promise<void> => invoke("upsert_provider", { entry, newApiKey: newApiKey ?? undefined }),
  removeProvider: (id: string): Promise<void> => invoke("remove_provider", { id }),
  listNativeMetas: (): Promise<NativeMeta[]> => invoke("list_native_metas"),
  validateTemplate: (configJson: string): Promise<void> =>
    invoke("validate_template", { configJson }),
  testTemplate: (
    configJson: string,
    apiKey: string | null,
    baseUrl: string | null,
  ): Promise<QueryOutcome> =>
    invoke("test_template", {
      configJson,
      apiKey: apiKey ?? undefined,
      baseUrl: baseUrl ?? undefined,
    }),
  queryProvider: (id: string): Promise<QueryOutcome> => invoke("query_provider", { id }),
  getSettings: (): Promise<Settings> => invoke("get_settings"),
  saveSettings: (settings: Settings): Promise<void> => invoke("save_settings", { settings }),
  /** 推送解析后的实际主题（ThemeProvider 调用，托盘圆环图标配色取用）。 */
  setResolvedTheme: (theme: "light" | "dark"): Promise<void> =>
    invoke("set_resolved_theme", { theme }),
  getSnapshots: (): Promise<Record<string, SnapshotEntry>> => invoke("get_snapshots"),
};

/** 生成 6 位 base32 短 id（新增条目用，CLI 同款约定）。 */
export function newEntryId(): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  const bytes = new Uint8Array(6);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => alphabet[b % 32]).join("");
}
