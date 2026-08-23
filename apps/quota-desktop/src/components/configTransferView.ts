function twoDigits(value: number): string {
  return String(value).padStart(2, "0");
}

export function defaultTransferFileName(now: Date): string {
  const date = `${now.getFullYear()}${twoDigits(now.getMonth() + 1)}${twoDigits(now.getDate())}`;
  const time = `${twoDigits(now.getHours())}${twoDigits(now.getMinutes())}${twoDigits(now.getSeconds())}`;
  return `QuotaTray-config-${date}-${time}.qtray-export`;
}

export function ensureTransferExtension(path: string): string {
  return path.toLowerCase().endsWith(".qtray-export") ? path : `${path}.qtray-export`;
}

export function transferErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "unknown error";
}
