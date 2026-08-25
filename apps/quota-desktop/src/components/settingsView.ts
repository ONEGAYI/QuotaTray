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

/** 设置页更新主按钮的动作分派（决定文案与点击行为）。 */
export type UpdateAction = "downloading" | "install" | "download" | "check";

export function resolveUpdateAction({
  downloading,
  canDownload,
  hasDownloaded,
}: {
  downloading: boolean;
  canDownload: boolean;
  hasDownloaded: boolean;
}): UpdateAction {
  if (downloading) return "downloading";
  // 安装态要求仍有可下载的新版本：换版本/检测失败时后端已清记录
  if (canDownload) return hasDownloaded ? "install" : "download";
  return "check";
}
