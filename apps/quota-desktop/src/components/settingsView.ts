import type { DownloadProgress } from "../types";

export type UpdateViewStatus = "checking" | "available" | "error" | "current";

export function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"] as const;
  const exponent = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const scaled = value / 1024 ** exponent;
  return exponent === 0 ? `${Math.round(scaled)} B` : `${scaled.toFixed(1)} ${units[exponent]}`;
}

export function downloadPercent(progress: DownloadProgress): number | null {
  if (progress.total_bytes == null || progress.total_bytes <= 0) return null;
  return Math.min(100, Math.round((progress.downloaded_bytes / progress.total_bytes) * 100));
}

export function formatDownloadProgress(progress: DownloadProgress): string {
  const received = formatBytes(progress.downloaded_bytes);
  const speed = `${formatBytes(progress.bytes_per_second)}/s`;
  const percent = downloadPercent(progress);
  if (progress.total_bytes == null || percent == null) return `${received} · ${speed}`;
  return `${received} / ${formatBytes(progress.total_bytes)} · ${speed} · ${percent}%`;
}

export function resolveUpdateError({
  checkError,
  downloadError,
  installError,
  backendError,
  hasAvailable,
}: {
  checkError: unknown;
  downloadError: unknown;
  /** 运行安装包失败（文件丢失等），可选——后端清记录后前端刷新即恢复。 */
  installError?: unknown;
  backendError: string | null | undefined;
  hasAvailable: boolean;
}): string | null {
  if (checkError != null) return String(checkError);
  if (downloadError != null) return String(downloadError);
  if (installError != null) return String(installError);
  return !hasAvailable ? backendError ?? null : null;
}

/** 错误行悬停详情：仅当主错误文案恰好来自后端 last_error（自动检测
 * 失败）时才有对应 detail（如限流 403 的 GitHub message）；操作错误
 * （手动检测/下载/安装）与无错误时返回 null。 */
export function resolveUpdateErrorDetail({
  operationError,
  backendError,
  backendErrorDetail,
}: {
  operationError: string | null;
  backendError: string | null | undefined;
  backendErrorDetail: string | null | undefined;
}): string | null {
  if (operationError == null) return null;
  return operationError === backendError ? backendErrorDetail ?? null : null;
}

export function resolveUpdateStatus({
  checking,
  hasAvailable,
  error,
}: {
  checking: boolean;
  hasAvailable: boolean;
  error: string | null;
}): UpdateViewStatus {
  // 操作错误优先于"发现新版本"：下载/检测失败要立即以错误态呈现，
  // 不能静默维持可下载徽章（错误详情另由 error 行展示）
  if (checking) return "checking";
  if (error) return "error";
  if (hasAvailable) return "available";
  return "current";
}

/** 设置页更新主按钮的动作分派（决定文案与点击行为）。
 * zip 更新形态：已下载动作是「打开下载目录」引导手动覆盖，
 * 不提供运行安装包入口。 */
export type UpdateAction = "downloading" | "install" | "open-dir" | "download" | "check";

/** 更新页版本行的运行形态标签：架构 +（便携形态时）便携标记。
 * 后端未返回 platform（异常/旧后端）时返回空串，调用方拼接时跳过该段。 */
export function runtimeLabel(
  platform: string | null | undefined,
  portable: boolean | null | undefined,
  portableText: string,
): string {
  const arch = platform?.trim() || "";
  return portable ? (arch ? `${arch} · ${portableText}` : portableText) : arch;
}

export function resolveUpdateAction({
  downloading,
  canDownload,
  hasDownloaded,
  manualUpdate = false,
}: {
  downloading: boolean;
  canDownload: boolean;
  hasDownloaded: boolean;
  /** zip 更新形态（ARM64 Preview / Portable），走手动覆盖引导。 */
  manualUpdate?: boolean;
}): UpdateAction {
  if (downloading) return "downloading";
  // 安装态要求仍有可下载的新版本：换版本/检测失败时后端已清记录
  if (canDownload) {
    if (hasDownloaded) return manualUpdate ? "open-dir" : "install";
    return "download";
  }
  return "check";
}
