// IPC 封装：全部后端交互收敛于此，组件不直接触碰 invoke。
import { invoke } from "@tauri-apps/api/core";
import type {
  BootStateDto,
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
  /** 启动状态：pendingPortableInit=true 时前端渲染便携首启确认页。 */
  getBootState: (): Promise<BootStateDto> => invoke("get_boot_state"),
  /** 便携首启确认：创建主密钥并补齐后端启动（用户已显式确认）。 */
  confirmPortableInit: (): Promise<void> => invoke("confirm_portable_init"),
  /** 便携首启取消：清理 Data（仅 WebView2 缓存）并退出应用。 */
  cancelPortableInit: (): Promise<void> => invoke("cancel_portable_init"),
  /** 打开更新下载目录（便携形态手动覆盖引导）。 */
  openUpdateDir: (): Promise<void> => invoke("open_update_dir"),
  /** 打开控制台直达 URL（scheme 白名单在 Rust 侧收口，仅 http/https）。 */
  openConsoleUrl: (url: string): Promise<void> => invoke("open_console_url", { url }),
  listProviders: (): Promise<ProviderEntry[]> => invoke("list_providers"),
  /** 将用户预览过的无凭据 AI 诊断包写入指定路径。 */
  writeAssistPackage: (path: string, contents: string): Promise<void> =>
    invoke("write_assist_package", { path, contents }),
  /** 返回开发版或安装包内真实存在的 quota CLI 绝对路径。 */
  resolveQuotaCliPath: (): Promise<string> => invoke("resolve_quota_cli_path"),
  upsertProvider: (
    entry: ProviderEntry,
    newApiKey: string | null,
    newApiKey2: string | null = null,
  ): Promise<void> =>
    invoke("upsert_provider", {
      entry,
      newApiKey: newApiKey ?? undefined,
      newApiKey2: newApiKey2 ?? undefined,
    }),
  removeProvider: (id: string): Promise<void> => invoke("remove_provider", { id }),
  /** 清空全部用户数据（条目/凭据密文/定价/查询历史；应用偏好与主密钥
   *  保留）。调用方须已通过二级确认弹窗取得显式确认。 */
  clearAllData: (): Promise<void> => invoke("clear_all_data"),
  /** 按完整 id 顺序重排条目（卡片拖拽排序落库；集合不一致时后端拒绝）。 */
  reorderProviders: (ids: string[]): Promise<void> => invoke("reorder_providers", { ids }),
  listNativeMetas: (): Promise<NativeMeta[]> => invoke("list_native_metas"),
  validateTemplate: (configJson: string): Promise<void> =>
    invoke("validate_template", { configJson }),
  testTemplate: (
    configJson: string,
    apiKey: string | null,
    baseUrl: string | null,
    apiKey2: string | null = null,
  ): Promise<QueryOutcome> =>
    invoke("test_template", {
      configJson,
      apiKey: apiKey ?? undefined,
      baseUrl: baseUrl ?? undefined,
      apiKey2: apiKey2 ?? undefined,
    }),
  validateScript: (configJson: string): Promise<void> =>
    invoke("validate_script", { configJson }),
  testScript: (
    configJson: string,
    apiKey: string | null,
    baseUrl: string | null,
    apiKey2: string | null = null,
  ): Promise<QueryOutcome> =>
    invoke("test_script", {
      configJson,
      apiKey: apiKey ?? undefined,
      baseUrl: baseUrl ?? undefined,
      apiKey2: apiKey2 ?? undefined,
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
  /** Android：下载 APK 写入系统文档选择器（SAF）返回的 content:// 位置。 */
  downloadUpdateToUri: (path: string): Promise<void> =>
    invoke("download_update_to_uri", { path }),
  /** Android：以系统安装器打开已保存的 APK；false = 系统无安装器。 */
  openDownloadedApk: (path: string): Promise<boolean> =>
    invoke("open_downloaded_apk", { path }),
  /** Android：打开「允许安装未知应用」授权页（安装弹回时的出路）。 */
  openInstallConsent: (): Promise<void> => invoke("open_install_consent"),
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
