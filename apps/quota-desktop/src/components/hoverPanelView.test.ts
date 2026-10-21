import { describe, expect, it } from "vitest";
import type { ProviderEntry, Settings } from "../types";
import {
  batchFailureForRow,
  hoverPanelMode,
  hoverPrimaryLabelKind,
  hoverRingView,
  hoverRows,
  hoverTargetHintKey,
  isCompactViewport,
  refreshSequentially,
  resolveHoverProvider,
} from "./hoverPanelView";

function provider(id: string, enabled = true): ProviderEntry {
  return {
    id,
    name: id,
    enabled,
    kind: { type: "native", provider: "deepseek" },
  };
}

describe("resolveHoverProvider", () => {
  it("优先选择设置指定的启用条目，失效时回退第一个启用条目", () => {
    const providers = [provider("disabled", false), provider("first"), provider("chosen")];
    expect(resolveHoverProvider(providers, "chosen")?.id).toBe("chosen");
    expect(resolveHoverProvider(providers, "disabled")?.id).toBe("first");
    expect(resolveHoverProvider(providers, "missing")?.id).toBe("first");
  });
});

describe("hoverRingView", () => {
  it("与托盘圆环保持余额分层及百分比剩余语义", () => {
    expect(hoverRingView({ remaining: 180 }, 100)).toEqual({ fillPercent: 80, center: "180" });
    expect(hoverRingView({ unit: "%", used: 42 }, 100)).toEqual({ fillPercent: 58, center: "58%" });
    expect(hoverRingView({ remaining: 1_250 }, 100)).toEqual({ fillPercent: 100, center: "1250" });
    expect(hoverRingView(undefined, 100)).toBeNull();
  });

  it("主度量偏好分档（T-24）：amount 档余额环优先、auto/percent 百分比优先，互为回退", () => {
    // 两者皆可的形态：used/total 可换算百分比 + remaining 有值
    const both = { used: 30, total: 200, remaining: 170 };
    expect(hoverRingView(both, 100, "auto")).toEqual({ fillPercent: 85, center: "85%" });
    expect(hoverRingView(both, 100, "percent")).toEqual({ fillPercent: 85, center: "85%" });
    // amount 档：余额环走每圈单位机制（170/100 = 1 满圈 + 0.7 顶层弧）
    expect(hoverRingView(both, 100, "amount")).toEqual({ fillPercent: 70, center: "170" });
    // 指定度量算不出时静默回退另一度量
    expect(hoverRingView({ unit: "%", used: 42 }, 100, "amount")).toEqual({ fillPercent: 58, center: "58%" });
    expect(hoverRingView({ remaining: 180 }, 100, "percent")).toEqual({ fillPercent: 80, center: "180" });
  });
});

describe("isCompactViewport", () => {
  it("完整高度（520）为否，压缩高度（260）为是，阈值边界为否", () => {
    expect(isCompactViewport(520)).toBe(false);
    expect(isCompactViewport(400)).toBe(false);
    expect(isCompactViewport(399)).toBe(true);
    expect(isCompactViewport(260)).toBe(true);
    expect(isCompactViewport(Number.NaN)).toBe(false);
  });
});

describe("hoverPanelMode（spec #151：内容模式）", () => {
  it("single 原样返回；缺省与未知值回退 multi（与后端 sanitize 同向）", () => {
    expect(hoverPanelMode("single")).toBe("single");
    expect(hoverPanelMode("multi")).toBe("multi");
    expect(hoverPanelMode(undefined)).toBe("multi");
    expect(hoverPanelMode(null)).toBe("multi");
    expect(hoverPanelMode("list")).toBe("multi");
  });
});

describe("hoverRows（spec #151 / T-29：列表行派生）", () => {
  const settings = (over: Partial<Settings> = {}) =>
    ({
      tray_icon_entry_id: null,
      ring_units_per_circle: 100,
      low_balance_remaining_percent: 20,
      ...over,
    }) as Settings;

  const at = Date.UTC(2026, 9, 22, 12, 0, 0);

  it("只产出启用条目且保持存储顺序", () => {
    const rows = hoverRows({
      providers: [provider("a"), provider("b", false), provider("c")],
      states: undefined,
      snapshots: undefined,
      settings: settings(),
      lang: "zh",
    });
    expect(rows.map((row) => row.id)).toEqual(["a", "c"]);
  });

  it("显示目标落在 tray_icon_entry_id；指定条目失效时回退第一个启用条目", () => {
    const providers = [provider("a"), provider("b")];
    const targeted = hoverRows({
      providers,
      states: undefined,
      snapshots: undefined,
      settings: settings({ tray_icon_entry_id: "b" }),
      lang: "zh",
    });
    expect(targeted.filter((row) => row.isTarget).map((row) => row.id)).toEqual(["b"]);

    const fallback = hoverRows({
      providers,
      states: undefined,
      snapshots: undefined,
      settings: settings({ tray_icon_entry_id: "gone" }),
      lang: "zh",
    });
    expect(fallback.filter((row) => row.isTarget).map((row) => row.id)).toEqual(["a"]);
  });

  it("每个窗口一行指标，多窗口带窗口短标签；低余额窗口置警示", () => {
    const rows = hoverRows({
      providers: [provider("a")],
      states: undefined,
      snapshots: {
        a: {
          data: [
            { used: 84, unit: "%", plan_name: "GLM Coding Plan（5h）" },
            { used: 1, unit: "%", plan_name: "GLM Coding Plan（MCP）" },
          ],
          at,
        },
      },
      settings: settings(),
      lang: "zh",
      nowMs: at,
    });
    expect(rows[0].metrics.map((metric) => metric.label)).toEqual(["剩余 5h", "剩余 MCP"]);
    expect(rows[0].metrics.map((metric) => metric.value)).toEqual(["16%", "99%"]);
    expect(rows[0].metrics.map((metric) => metric.alert)).toEqual([true, false]);
    expect(rows[0].kind).toBe("snapshot");
  });

  it("单窗口不附加窗口短标签，金额窗口用可用余额措辞", () => {
    const rows = hoverRows({
      providers: [provider("a")],
      states: undefined,
      snapshots: { a: { data: [{ remaining: 62.97, unit: "CNY" }], at } },
      settings: settings(),
      lang: "zh",
      nowMs: at,
    });
    expect(rows[0].metrics).toHaveLength(1);
    expect(rows[0].metrics[0].label).toBe("可用余额");
    expect(rows[0].metrics[0].value).toBe("62.97");
    expect(rows[0].metrics[0].unit).toBe("CNY");
  });

  it("目标环进度与 hoverRingView 同源（百分比剩余 / 余额顶层小数弧）", () => {
    const percentRows = hoverRows({
      providers: [provider("a")],
      states: undefined,
      snapshots: { a: { data: [{ used: 42, unit: "%" }], at } },
      settings: settings(),
      lang: "zh",
      nowMs: at,
    });
    expect(percentRows[0].ringFill).toBe(58);

    const amountRows = hoverRows({
      providers: [provider("b")],
      states: undefined,
      snapshots: { b: { data: [{ remaining: 180 }], at } },
      settings: settings({ ring_units_per_circle: 100 }),
      lang: "zh",
      nowMs: at,
    });
    expect(amountRows[0].ringFill).toBe(80);
  });

  it("无任何数据时指标为空、目标环进度为 null", () => {
    const rows = hoverRows({
      providers: [provider("a")],
      states: undefined,
      snapshots: undefined,
      settings: settings(),
      lang: "zh",
    });
    expect(rows[0].metrics).toEqual([]);
    expect(rows[0].ringFill).toBeNull();
    expect(rows[0].kind).toBe("empty");
  });

  it("live 结果优先于启动快照；错误态直接透传", () => {
    const liveRows = hoverRows({
      providers: [provider("a")],
      states: {
        a: { ok: true, data: [{ used: 20, unit: "%" }], error: null, at: at + 1 },
      },
      snapshots: { a: { data: [{ used: 90, unit: "%" }], at } },
      settings: settings(),
      lang: "zh",
      nowMs: at + 1,
    });
    expect(liveRows[0].kind).toBe("normal");
    expect(liveRows[0].data).toEqual([{ used: 20, unit: "%" }]);
    expect(liveRows[0].metrics[0].value).toBe("80%");

    const failedRows = hoverRows({
      providers: [provider("b")],
      states: {
        b: {
          ok: false,
          data: null,
          error: { kind: "deterministic", message: "bad key", detail: null },
          at,
        },
      },
      snapshots: undefined,
      settings: settings(),
      lang: "zh",
      nowMs: at,
    });
    expect(failedRows[0].kind).toBe("deterministic");
    expect(failedRows[0].errorMessage).toBe("bad key");
    expect(failedRows[0].metrics).toEqual([]);
  });

  it("目标环取首个能算出弧长的窗口（与 ring.rs 跳过空输入的规则一致）", () => {
    const rows = hoverRows({
      providers: [provider("a")],
      states: undefined,
      snapshots: {
        a: {
          data: [
            // 首窗口只有单位，算不出百分比也拿不到余额
            { unit: "%", plan_name: "空窗口" },
            { used: 20, unit: "%", plan_name: "有效窗口" },
          ],
          at,
        },
      },
      settings: settings(),
      lang: "zh",
      nowMs: at,
    });
    expect(rows[0].ringFill).toBe(80);
  });
});

describe("hoverPrimaryLabelKind（single 模式文案档位）", () => {
  it("按主度量偏好分档并回退：percent / amount / empty", () => {
    expect(hoverPrimaryLabelKind({ used: 42, unit: "%" }, "auto")).toBe("percent");
    expect(hoverPrimaryLabelKind({ remaining: 62.97 }, "amount")).toBe("amount");
    expect(hoverPrimaryLabelKind({ remaining: 62.97 }, "percent")).toBe("amount");
    expect(hoverPrimaryLabelKind({ unit: "CNY" }, "auto")).toBe("empty");
    expect(hoverPrimaryLabelKind(undefined, "auto")).toBe("empty");
  });
});

describe("refreshSequentially（多条目头部刷新）", () => {
  it("严格串行且单条失败不中断后续", async () => {
    const started: string[] = [];
    let active = 0;
    let maxActive = 0;
    const run = async (id: string) => {
      active += 1;
      maxActive = Math.max(maxActive, active);
      started.push(id);
      await new Promise((resolve) => setTimeout(resolve, 1));
      active -= 1;
      if (id === "b") throw new Error("boom");
      return id;
    };

    const result = await refreshSequentially(["a", "b", "c"], run);

    // 按输入顺序逐个执行，且没有并发
    expect(started).toEqual(["a", "b", "c"]);
    expect(maxActive).toBe(1);
    expect(result.ok).toEqual(["a", "c"]);
    expect(result.failed).toHaveLength(1);
    expect(result.failed[0].id).toBe("b");
    expect(result.failed[0].error).toContain("boom");
  });

  it("平台查询失败以 ok=false 正常返回时也计入失败清单", async () => {
    const result = await refreshSequentially(
      ["a", "b"],
      async (id) => ({ ok: id !== "b", error: id === "b" ? { message: "bad key" } : null }),
      (outcome) => (outcome.ok ? null : outcome.error?.message ?? "query failed"),
    );

    expect(result.ok).toEqual(["a"]);
    expect(result.failed).toEqual([{ id: "b", error: "bad key" }]);
  });
});

describe("hoverTargetHintKey（on/off 环悬停提示）", () => {
  it("按目标状态区分提示文案", () => {
    expect(hoverTargetHintKey(true)).toBe("hover.targetOn");
    expect(hoverTargetHintKey(false)).toBe("hover.targetOff");
  });
});

describe("batchFailureForRow（批次失败归属）", () => {
  it("失败条目命中才返回文案；未命中返回 null（别家账户的错误不进抽屉）", () => {
    const failure = { message: "乙: key 无效", ids: ["b"] };
    expect(batchFailureForRow(failure, "b")).toBe("乙: key 无效");
    expect(batchFailureForRow(failure, "a")).toBeNull();
  });

  it("空 ids（整批/未知归属）对所有账户都算相关；null 表示无批次失败", () => {
    expect(batchFailureForRow({ message: "IPC 失败", ids: [] }, "a")).toBe("IPC 失败");
    expect(batchFailureForRow(null, "a")).toBeNull();
    expect(batchFailureForRow(undefined, "a")).toBeNull();
  });
});
