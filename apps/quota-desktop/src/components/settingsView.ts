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
  backendError,
  hasAvailable,
}: {
  checkError: unknown;
  downloadError: unknown;
  backendError: string | null | undefined;
  hasAvailable: boolean;
}): string | null {
  if (checkError != null) return String(checkError);
  if (downloadError != null) return String(downloadError);
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
