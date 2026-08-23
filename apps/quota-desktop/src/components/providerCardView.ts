import {
  KEEP_LAST_GOOD_MS,
  type QueryOutcome,
  type SnapshotEntry,
  type UsageData,
} from "../types";

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
      };
    }
    return {
      kind: isFetching ? "loading" : "empty",
      data: [],
      at: null,
      source: "none",
      errorMessage: null,
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
      };
    }
    return {
      kind: outcome.error.kind === "deterministic" ? "deterministic" : "transient",
      data: [],
      at: outcome.at,
      source: "none",
      errorMessage: outcome.error.message,
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
    };
  }

  return {
    kind: "normal",
    data: outcome.data ?? [],
    at: outcome.at,
    source: "live",
    errorMessage: null,
  };
}
