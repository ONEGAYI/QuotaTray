import { describe, expect, it } from "vitest";
import type { ProviderEntry, Settings } from "../types";
import {
  hoverPanelMode,
  hoverRingView,
  hoverRows,
  hoverTargetHintKey,
  isCompactViewport,
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
});

describe("hoverTargetHintKey（on/off 环悬停提示）", () => {
  it("按目标状态区分提示文案", () => {
    expect(hoverTargetHintKey(true)).toBe("hover.targetOn");
    expect(hoverTargetHintKey(false)).toBe("hover.targetOff");
  });
});
