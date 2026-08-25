import { describe, expect, it, vi } from "vitest";
import {
  PROVIDERS_CHANGED_EVENT,
  invalidateProviderCaches,
} from "./queries";

describe("Provider 跨窗口变更事件", () => {
  it("使用稳定事件名并失效所有 Provider 派生缓存", () => {
    expect(PROVIDERS_CHANGED_EVENT).toBe("providers-changed");
    const invalidateQueries = vi.fn();

    invalidateProviderCaches({ invalidateQueries });

    expect(invalidateQueries.mock.calls.map(([filter]) => filter.queryKey)).toEqual([
      ["providers"],
      ["provider"],
      ["provider-state"],
      ["snapshots"],
    ]);
  });
});
