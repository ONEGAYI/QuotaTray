import {
  KEEP_LAST_GOOD_MS,
  type NativeMeta,
  type ProviderEntry,
  type QueryOutcome,
  type SnapshotEntry,
  type UsageData,
} from "../types";

/** 控制台直达 URL：条目自定义覆盖优先，回退 native 预置默认
 *  （core `provider::resolve_console_url` 的前端镜像——渲染入口共用
 *  同一语义，None/undefined 时不渲染按钮）。 */
export function resolveConsoleUrl(
  entry: Pick<ProviderEntry, "console_url">,
  nativeMeta?: NativeMeta,
): string | null {
  return entry.console_url ?? nativeMeta?.console_url ?? null;
}

/** 编辑表单「控制台地址」输入校验：空（=回退/清除覆盖）或 `http(s)://`
 *  形态（scheme 大小写不敏感，RFC 3986；裸 scheme/单斜杠畸形拒绝）；
 *  与后端 open_console_url 的 Rust 侧白名单同一口径（两侧均先 trim）。 */
export function isValidConsoleUrlInput(url: string): boolean {
  const trimmed = url.trim();
  if (trimmed === "") return true;
  const sep = trimmed.indexOf("://");
  if (sep === -1) return false;
  const scheme = trimmed.slice(0, sep).toLowerCase();
  return scheme === "http" || scheme === "https";
}

export type ProviderCardKind =
  | "normal"
  | "snapshot"
  | "stale"
  | "transient"
  | "deterministic"
  | "invalid"
  | "disabled"
  | "loading"
  | "empty";

export interface ProviderCardState {
  kind: ProviderCardKind;
  data: UsageData[];
  at: number | null;
  source: "live" | "snapshot" | "none";
  errorMessage: string | null;
  /** 查询错误的排查详情（已脱敏响应体等）；invalid 业务失效与无错态为 null。 */
  errorDetail: string | null;
}

interface Input {
  enabled: boolean;
  outcome?: QueryOutcome;
  snapshot?: SnapshotEntry;
  isFetching?: boolean;
  nowMs?: number;
}

/** QueryOutcome + 启动快照 → 卡片展示状态，保持与 tray/keep-last-good 契约一致。 */
export function deriveProviderCardState({
  enabled,
  outcome,
  snapshot,
  isFetching = false,
  nowMs = Date.now(),
}: Input): ProviderCardState {
  const fallbackData = outcome?.data ?? snapshot?.data ?? [];
  const fallbackAt = outcome?.at ?? snapshot?.at ?? null;
  const source = outcome?.data != null ? "live" : snapshot ? "snapshot" : "none";

  if (!enabled) {
    return {
      kind: "disabled",
      data: fallbackData,
      at: fallbackAt,
      source,
      errorMessage: null,
      errorDetail: null,
    };
  }

  if (!outcome) {
    if (snapshot) {
      return {
        kind: "snapshot",
        data: snapshot.data,
        at: snapshot.at,
        source: "snapshot",
        errorMessage: null,
        errorDetail: null,
      };
    }
    return {
      kind: isFetching ? "loading" : "empty",
      data: [],
      at: null,
      source: "none",
      errorMessage: null,
      errorDetail: null,
    };
  }

  if (!outcome.ok && outcome.error) {
    const keepGood =
      outcome.error.kind === "transient" &&
      outcome.data != null &&
      outcome.at != null &&
      nowMs - outcome.at <= KEEP_LAST_GOOD_MS;
    if (keepGood) {
      return {
        kind: "stale",
        data: outcome.data ?? [],
        at: outcome.at,
        source: "live",
        errorMessage: outcome.error.message,
        errorDetail: outcome.error.detail ?? null,
      };
    }
    return {
      kind: outcome.error.kind === "deterministic" ? "deterministic" : "transient",
      data: [],
      at: outcome.at,
      source: "none",
      errorMessage: outcome.error.message,
      errorDetail: outcome.error.detail ?? null,
    };
  }

  const invalid = outcome.data?.find((item) => item.is_valid === false);
  if (invalid) {
    return {
      kind: "invalid",
      data: [],
      at: outcome.at,
      source: "none",
      errorMessage: invalid.invalid_message ?? null,
      errorDetail: null,
    };
  }

  return {
    kind: "normal",
    data: outcome.data ?? [],
    at: outcome.at,
    source: "live",
    errorMessage: null,
    errorDetail: null,
  };
}

/** 查询错误是否有可复制的排查内容（invalid 业务失效不是查询错误）。 */
export function canCopyError(state: ProviderCardState): boolean {
  return state.errorMessage != null && state.kind !== "invalid";
}

/** 复制到剪贴板的报错全文：headline（kind + message）+ 空行 + 脱敏详情；
 * 无 detail 时回退纯 headline（与卡片展示文案一致的纯函数，供组件调用）。 */
export function errorCopyText(state: ProviderCardState): string {
  const kind = state.kind === "deterministic" ? "deterministic" : "transient";
  const headline = `[${kind}] ${state.errorMessage ?? ""}`;
  return state.errorDetail ? `${headline}\n\n${state.errorDetail}` : headline;
}
