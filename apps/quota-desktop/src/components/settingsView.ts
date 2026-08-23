export type UpdateViewStatus = "checking" | "available" | "error" | "current";

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
  if (checking) return "checking";
  if (hasAvailable) return "available";
  if (error) return "error";
  return "current";
}
