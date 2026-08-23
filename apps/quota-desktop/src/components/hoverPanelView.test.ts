import { describe, expect, it } from "vitest";
import type { ProviderEntry } from "../types";
import { hoverRingView, resolveHoverProvider } from "./hoverPanelView";

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
});
