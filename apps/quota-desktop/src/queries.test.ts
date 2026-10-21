import { describe, expect, it, vi } from "vitest";
import {
  PROVIDERS_CHANGED_EVENT,
  historyFromNow,
  invalidateDisplaySettingsCache,
  invalidateHoverPanelCaches,
  invalidateProviderCaches,
} from "./queries";

describe("Provider 跨窗口变更事件", () => {
  it("使用稳定事件名", () => {
    expect(PROVIDERS_CHANGED_EVENT).toBe("providers-changed");
  });

  it("按条目失效：单条目变更刷新列表、整表状态、该条目查询/只读视图与快照", () => {
    const invalidateQueries = vi.fn();

    invalidateProviderCaches({ invalidateQueries }, "p1");

    expect(invalidateQueries.mock.calls.map(([filter]) => filter.queryKey)).toEqual([
      ["providers"],
      // 悬停面板列表消费整表缓存：条目启停/重命名也改变行集合
      ["provider-states"],
      ["provider", "p1"],
      ["history", "p1"],
      ["snapshots"],
    ]);
  });

  it("全量失效：配置导入等所有条目都可能变化的场景", () => {
    const invalidateQueries = vi.fn();

    invalidateProviderCaches({ invalidateQueries });

    expect(invalidateQueries.mock.calls.map(([filter]) => filter.queryKey)).toEqual([
      ["providers"],
      ["provider-states"],
      ["provider"],
      ["history"],
      ["snapshots"],
    ]);
  });
});

describe("悬停面板唤起时的缓存失效", () => {
  it("整表状态与设置都失效（面板常驻且非激活显示，不会 focus 重取）", () => {
    const invalidateQueries = vi.fn();

    invalidateHoverPanelCaches({ invalidateQueries });

    expect(invalidateQueries.mock.calls.map(([filter]) => filter.queryKey)).toEqual([
      ["provider-states"],
      ["settings"],
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
