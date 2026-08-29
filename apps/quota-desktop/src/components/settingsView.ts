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
 * 不提供运行安装包入口。
 * Android：APK 已保存到 SAF 位置（会话内存，不经后端状态表）时动作
 * 是「安装」——拉起系统安装器，不出现桌面目录语义。 */
export type UpdateAction =
  | "downloading"
  | "install"
  | "install-mobile"
  | "open-dir"
  | "download"
  | "check";

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
  mobileSaved = false,
}: {
  downloading: boolean;
  canDownload: boolean;
  hasDownloaded: boolean;
  /** zip 更新形态（ARM64 Preview / Portable），走手动覆盖引导。 */
  manualUpdate?: boolean;
  /** Android：APK 已保存到 SAF 位置（content URI 留存前端会话）。
   * 优先于桌面 install/open-dir 分流——移动端 downloaded_path 恒空，
   * 「已下载」只能由前端本地状态表达。 */
  mobileSaved?: boolean;
}): UpdateAction {
  if (downloading) return "downloading";
  // 安装态要求仍有可下载的新版本：换版本/检测失败时后端已清记录
  // （移动端 savedApkUri 由重检测一并失效，同语义）
  if (canDownload) {
    if (mobileSaved) return "install-mobile";
    if (hasDownloaded) return manualUpdate ? "open-dir" : "install";
    return "download";
  }
  return "check";
}

/** Android 已保存 APK（SAF content URI）的有效性判定：下载时记录的
 * 可用版本快照与当前可用版本一致才可用——重检测发现新版本时自动
 * 失效（旧包不该再装），同版本重复检测不清空（18MB 产物不白费）。
 * available 为 null（无可用更新/检测失败）时只有 null 快照匹配。 */
export function savedApkIsCurrent(
  saved: { uri: string; version: string | null } | null,
  availableVersion: string | null,
): saved is { uri: string; version: string | null } {
  return saved != null && saved.version === availableVersion;
}

/** Android 通知权限行的动作形态（消息中心二阶）：
 * - none：无需按钮（桌面/开关关闭/未加载/已授权）；
 * - request：未请求过（prompt），点按弹系统运行时权限对话框；
 * - open-settings：拒绝过（denied）——Android 13+ 系统不再弹对话框，
 *   跳「应用通知设置」页由用户手动开启是唯一出路。
 * 开关只管 notifications_enabled（发不发），权限行只管系统授权（能不能
 * 弹出），两者解耦——打开开关不隐式弹权限，请求由按钮显式触发。 */
export type NotificationPermissionAction = "none" | "request" | "open-settings";

export function resolveNotificationPermissionAction({
  mobile,
  notificationsEnabled,
  permission,
}: {
  /** 移动壳层（桌面无运行时权限概念，恒 granted）。 */
  mobile: boolean;
  notificationsEnabled: boolean;
  /** 后端 get_notification_permission 结果；null = 尚未加载。 */
  permission: string | null;
}): NotificationPermissionAction {
  if (!mobile || !notificationsEnabled) return "none";
  if (permission === "denied") return "open-settings";
  if (permission === "prompt" || permission === "prompt-with-rationale") return "request";
  return "none";
}

/** 设置页签的消费时序（纯函数）：对话框打开时消费 initialTab——含
 * 「开着期间 prop 变化」的直达场景（消息卡片「查看更新」在设置页已开
 * 时再次触发也要生效）；关闭/未打开不消费（关闭重置由调用方 onClose
 * 负责，此后自然回退默认页签）。 */
export function resolveTabOnOpen<T extends string>(open: boolean, initialTab: T, current: T): T {
  return open ? initialTab : current;
}
