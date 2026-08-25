import { describe, expect, it, vi } from "vitest";
import {
  PROVIDERS_CHANGED_EVENT,
  invalidateDisplaySettingsCache,
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

describe("标题栏显示设置缓存失效", () => {
  it("主题或语言保存后只刷新 settings，不触发 Provider 查询", () => {
    const invalidateQueries = vi.fn();

    invalidateDisplaySettingsCache({ invalidateQueries });

    expect(invalidateQueries.mock.calls.map(([filter]) => filter.queryKey)).toEqual([
      ["settings"],
    ]);
  });
});
