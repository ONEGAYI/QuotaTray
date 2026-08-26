// IPC 封装：全部后端交互收敛于此，组件不直接触碰 invoke。
import { invoke } from "@tauri-apps/api/core";
import type {
  HistoryPoint,
  NativeMeta,
  ProviderEntry,
  QueryOutcome,
  Settings,
  SettingsPatch,
  SnapshotEntry,
  UpdateStateDto,
} from "./types";

export const api = {
  listProviders: (): Promise<ProviderEntry[]> => invoke("list_providers"),
  /** 将用户预览过的无凭据 AI 诊断包写入指定路径。 */
  writeAssistPackage: (path: string, contents: string): Promise<void> =>
    invoke("write_assist_package", { path, contents }),
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
  validateScript: (configJson: string): Promise<void> =>
    invoke("validate_script", { configJson }),
  testScript: (
    configJson: string,
    apiKey: string | null,
    baseUrl: string | null,
  ): Promise<QueryOutcome> =>
    invoke("test_script", {
      configJson,
      apiKey: apiKey ?? undefined,
      baseUrl: baseUrl ?? undefined,
    }),
  queryProvider: (id: string): Promise<QueryOutcome> => invoke("query_provider", { id }),
  /** 只读后端共享结果表，不触发平台网络请求（悬停面板使用）。 */
  getProviderState: (id: string): Promise<QueryOutcome> => invoke("get_provider_state", { id }),
  /** 读取本地 SQLite 历史，不触发平台网络请求。 */
  getHistory: (id: string, fromMs: number): Promise<HistoryPoint[]> =>
    invoke("get_history", { id, fromMs }),
  getSettings: (): Promise<Settings> => invoke("get_settings"),
  saveSettings: (settings: Settings): Promise<void> => invoke("save_settings", { settings }),
  /** 局部更新设置：后端读现值合并 patch，避免前端缓存全量回写。 */
  patchSettings: (patch: SettingsPatch): Promise<void> =>
    invoke("patch_settings", { patch }),
  /** 将完整配置导出到系统保存对话框选定的路径。 */
  exportConfiguration: (path: string): Promise<void> =>
    invoke("export_configuration", { path }),
  /** 从迁移包整体替换配置，返回导入的供应商数量。 */
  importConfiguration: (path: string): Promise<number> =>
    invoke("import_configuration", { path }),
  /** 推送解析后的实际主题（ThemeProvider 调用，托盘圆环图标配色取用）。 */
  setResolvedTheme: (theme: "light" | "dark"): Promise<void> =>
    invoke("set_resolved_theme", { theme }),
  getSnapshots: (): Promise<Record<string, SnapshotEntry>> => invoke("get_snapshots"),
  getUpdateState: (): Promise<UpdateStateDto> => invoke("get_update_state"),
  checkUpdateNow: (): Promise<UpdateStateDto> => invoke("check_update_now"),
  /** 下载安装包到 %TEMP%/QuotaTray/Downloads，返回完整路径。 */
  downloadUpdate: (): Promise<string> => invoke("download_update"),
  /** 运行已下载的安装包（应用随后自动退出，NSIS 向导接管）。 */
  installUpdate: (): Promise<void> => invoke("install_update"),
  setHoverPanelPointerInside: (inside: boolean): Promise<void> =>
    invoke("set_hover_panel_pointer_inside", { inside }),
  hideHoverPanel: (): Promise<void> => invoke("hide_hover_panel"),
  openMainWindow: (): Promise<void> => invoke("open_main_window"),
};

/** 生成 6 位 base32 短 id（新增条目用，CLI 同款约定）。 */
export function newEntryId(): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  const bytes = new Uint8Array(6);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => alphabet[b % 32]).join("");
}
