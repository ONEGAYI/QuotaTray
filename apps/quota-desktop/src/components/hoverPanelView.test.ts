import { describe, expect, it } from "vitest";
import type { ProviderEntry } from "../types";
import { hoverRingView, isCompactViewport, resolveHoverProvider } from "./hoverPanelView";

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
