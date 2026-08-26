import { describe, expect, it, vi } from "vitest";
import {
  PROVIDERS_CHANGED_EVENT,
  historyFromNow,
  invalidateDisplaySettingsCache,
  invalidateProviderCaches,
} from "./queries";

describe("Provider 跨窗口变更事件", () => {
  it("使用稳定事件名", () => {
    expect(PROVIDERS_CHANGED_EVENT).toBe("providers-changed");
  });

  it("按条目失效：单条目变更只刷新列表、该条目查询/只读视图与快照", () => {
    const invalidateQueries = vi.fn();

    invalidateProviderCaches({ invalidateQueries }, "p1");

    expect(invalidateQueries.mock.calls.map(([filter]) => filter.queryKey)).toEqual([
      ["providers"],
      ["provider", "p1"],
      ["provider-state", "p1"],
      ["history", "p1"],
      ["snapshots"],
    ]);
  });

  it("全量失效：配置导入等所有条目都可能变化的场景", () => {
    const invalidateQueries = vi.fn();

    invalidateProviderCaches({ invalidateQueries });

    expect(invalidateQueries.mock.calls.map(([filter]) => filter.queryKey)).toEqual([
      ["providers"],
      ["provider"],
      ["provider-state"],
      ["history"],
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

describe("历史查询滚动时间窗", () => {
  it("每次查询都按当下时刻计算范围下界", () => {
    expect(historyFromNow(7_000, 20_000)).toBe(13_000);
    expect(historyFromNow(7_000, 5_000)).toBe(0);
  });
});
