import { describe, expect, it } from "vitest";
import { KEEP_LAST_GOOD_MS, type QueryOutcome, type SnapshotEntry } from "../types";
import { deriveProviderCardState } from "./providerCardView";

const NOW = 1_780_000_000_000;
const balance = [{ remaining: 18.7, unit: "CNY" }];

function outcome(patch: Partial<QueryOutcome>): QueryOutcome {
  return { ok: true, data: balance, error: null, at: NOW - 60_000, ...patch };
}

describe("Provider 卡片状态契约", () => {
  it("成功结果展示 live 数据与最后成功时间", () => {
    expect(
      deriveProviderCardState({ enabled: true, outcome: outcome({}), nowMs: NOW }),
    ).toMatchObject({ kind: "normal", data: balance, at: NOW - 60_000, source: "live" });
  });

  it("瞬时失败在 keep-last-good 窗口内展示旧值", () => {
    const at = NOW - KEEP_LAST_GOOD_MS + 1;
    expect(
      deriveProviderCardState({
        enabled: true,
        outcome: outcome({
          ok: false,
          at,
          error: { kind: "transient", message: "网络超时" },
        }),
        nowMs: NOW,
      }),
    ).toMatchObject({ kind: "stale", data: balance, at, errorMessage: "网络超时" });
  });

  it("keep-last-good 的等值边界仍保留旧值", () => {
    const at = NOW - KEEP_LAST_GOOD_MS;
    expect(
      deriveProviderCardState({
        enabled: true,
        outcome: outcome({
          ok: false,
          at,
          error: { kind: "transient", message: "网络超时" },
        }),
        nowMs: NOW,
      }),
    ).toMatchObject({ kind: "stale", at });
  });

  it("瞬时失败超过 keep-last-good 窗口后不再展示旧余额", () => {
    expect(
      deriveProviderCardState({
        enabled: true,
        outcome: outcome({
          ok: false,
          at: NOW - KEEP_LAST_GOOD_MS - 1,
          error: { kind: "transient", message: "网络超时" },
        }),
        nowMs: NOW,
      }),
    ).toMatchObject({ kind: "transient", data: [], errorMessage: "网络超时" });
  });

  it("确定性失败不展示保留数据", () => {
    expect(
      deriveProviderCardState({
        enabled: true,
        outcome: outcome({
          ok: false,
          error: { kind: "deterministic", message: "认证失败" },
        }),
        nowMs: NOW,
      }),
    ).toMatchObject({ kind: "deterministic", data: [], errorMessage: "认证失败" });
  });

  it("查询错误的排查详情随错误态透传，业务失效与无错态为 null", () => {
    const detail = "JSON 解析错误：expected value\n响应体（已脱敏）：\n<html/>";
    expect(
      deriveProviderCardState({
        enabled: true,
        outcome: outcome({
          ok: false,
          error: { kind: "deterministic", message: "响应不是合法 JSON", detail },
        }),
        nowMs: NOW,
      }),
    ).toMatchObject({ kind: "deterministic", errorDetail: detail });
    // 无 detail 的错误：回退 null
    expect(
      deriveProviderCardState({
        enabled: true,
        outcome: outcome({
          ok: false,
          error: { kind: "transient", message: "网络超时" },
        }),
        nowMs: NOW - KEEP_LAST_GOOD_MS - 1,
      }).errorDetail,
    ).toBeNull();
    // invalid 业务失效不是查询错误，不带详情
    expect(
      deriveProviderCardState({
        enabled: true,
        outcome: outcome({
          data: [{ remaining: 1, is_valid: false, invalid_message: "key 过期" }],
        }),
        nowMs: NOW,
      }).errorDetail,
    ).toBeNull();
  });

  it("无本次结果时使用启动快照并保留快照时间", () => {
    const snapshot: SnapshotEntry = { data: balance, at: NOW - 3600_000 };
    expect(
      deriveProviderCardState({ enabled: true, snapshot, nowMs: NOW }),
    ).toMatchObject({ kind: "snapshot", data: balance, at: snapshot.at, source: "snapshot" });
  });

  it("无数据时区分查询中与空状态", () => {
    expect(deriveProviderCardState({ enabled: true, isFetching: true, nowMs: NOW }).kind).toBe(
      "loading",
    );
    expect(deriveProviderCardState({ enabled: true, isFetching: false, nowMs: NOW }).kind).toBe(
      "empty",
    );
  });

  it("停用状态仍保留最后成功数据供卡片展示", () => {
    expect(
      deriveProviderCardState({ enabled: false, outcome: outcome({}), nowMs: NOW }),
    ).toMatchObject({ kind: "disabled", data: balance, at: NOW - 60_000 });
  });

  it("成功数据含失效窗口时进入 invalid 状态", () => {
    const invalid = [{ is_valid: false, invalid_message: "额度已过期" }];
    expect(
      deriveProviderCardState({
        enabled: true,
        outcome: outcome({ data: invalid }),
        nowMs: NOW,
      }),
    ).toMatchObject({ kind: "invalid", data: [], errorMessage: "额度已过期" });
  });

  it("多窗口成功数据保持完整顺序供展开区逐项告警", () => {
    const windows = [
      { plan_name: "月额度", used: 20, total: 100, unit: "CNY" },
      { plan_name: "并发额度", used: 95, total: 100, unit: "%" },
    ];
    expect(
      deriveProviderCardState({ enabled: true, outcome: outcome({ data: windows }), nowMs: NOW }),
    ).toMatchObject({ kind: "normal", data: windows });
  });
});
