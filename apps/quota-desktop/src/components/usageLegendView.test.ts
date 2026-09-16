import { describe, expect, it } from "vitest";
import { buildLegendItems, focusPlatformInfo, legendTriggerVisible, pressLegendRemove, toggleSeriesFocus } from "./usageLegendView";

describe("使用统计聚焦组合 popover 逻辑", () => {
  it("桌面端且已有组合时显示入口", () => {
    expect(legendTriggerVisible(1, false)).toBe(true);
    expect(legendTriggerVisible(4, false)).toBe(true);
  });

  it("移动端不显示入口（移动端保留横向 chips）", () => {
    expect(legendTriggerVisible(3, true)).toBe(false);
  });

  it("没有可比组合时不显示入口", () => {
    expect(legendTriggerVisible(0, false)).toBe(false);
  });

  it("单击未聚焦组合时切换为聚焦（含从其他组合转移）", () => {
    expect(toggleSeriesFocus(null, "p1::w1")).toBe("p1::w1");
    expect(toggleSeriesFocus("p2::w2", "p1::w1")).toBe("p1::w1");
  });

  it("再次单击已聚焦组合时失焦", () => {
    expect(toggleSeriesFocus("p1::w1", "p1::w1")).toBeNull();
  });

  it("首次点击删除图标进入待确认（armed）态", () => {
    expect(pressLegendRemove(null, "p1::w1")).toEqual({ kind: "armed", id: "p1::w1" });
  });

  it("再次点击已 armed 行确认删除", () => {
    expect(pressLegendRemove("p1::w1", "p1::w1")).toEqual({ kind: "removed", id: "p1::w1" });
  });

  it("armed 期间点击其他行改为待确认那一行", () => {
    expect(pressLegendRemove("p1::w1", "p2::w2")).toEqual({ kind: "armed", id: "p2::w2" });
  });
});

describe("聚焦组合行列表（含失效条目兜底）", () => {
  const selections = [
    { provider_id: "p1", window_key: "w1", color_slot: 0 },
    { provider_id: "gone", window_key: "stale", color_slot: 2 },
  ];
  const idOf = (providerId: string, windowKey: string) => JSON.stringify([providerId, windowKey]);

  it("按 selection 全量生成行，可见曲线标记 available", () => {
    const items = buildLegendItems(
      selections,
      new Set([idOf("p1", "w1")]),
      new Map([[idOf("p1", "w1"), "P1 · 窗口 1"]]),
    );
    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({ id: idOf("p1", "w1"), providerId: "p1", windowKey: "w1", colorSlot: 0, available: true, name: "P1 · 窗口 1" });
  });

  it("失效条目 available=false，候选缺名时回退原始 id 展示", () => {
    const items = buildLegendItems(selections, new Set([idOf("p1", "w1")]), new Map());
    expect(items[1]).toMatchObject({ id: idOf("gone", "stale"), providerId: "gone", windowKey: "stale", colorSlot: 2, available: false, name: "gone · stale" });
  });

  it("顺序与 selection 存储顺序一致", () => {
    const items = buildLegendItems(selections, new Set(), new Map());
    expect(items.map((item) => item.id)).toEqual([idOf("p1", "w1"), idOf("gone", "stale")]);
  });
});

describe("聚焦平台取数（卡头药丸内下陷显示窗）", () => {
  const items = buildLegendItems(
    [
      { provider_id: "p1", window_key: "w1", color_slot: 0 },
      { provider_id: "p2", window_key: "w2", color_slot: 2 },
    ],
    new Set([JSON.stringify(["p1", "w1"]), JSON.stringify(["p2", "w2"])]),
    new Map([
      [JSON.stringify(["p1", "w1"]), "P1 · 窗口 1"],
      [JSON.stringify(["p2", "w2"]), "P2 · 周限"],
    ]),
  );
  const scopes = [
    { id: JSON.stringify(["p1", "w1"]), metric: "percent" as const, samples: [{ value: 41.2 }, { value: 78 }] },
    { id: JSON.stringify(["p2", "w2"]), metric: "absolute" as const, samples: [] },
  ];

  it("未聚焦或聚焦项不在行列表时返回 null", () => {
    expect(focusPlatformInfo(null, items, scopes)).toBeNull();
    expect(focusPlatformInfo("gone", items, scopes)).toBeNull();
  });

  it("聚焦时取行名称、色槽与最新样本值", () => {
    expect(focusPlatformInfo(JSON.stringify(["p1", "w1"]), items, scopes)).toEqual({
      name: "P1 · 窗口 1",
      colorSlot: 0,
      value: 78,
      metric: "percent",
    });
  });

  it("组合暂无样本时值为 null 但保留名称与度量", () => {
    expect(focusPlatformInfo(JSON.stringify(["p2", "w2"]), items, scopes)).toEqual({
      name: "P2 · 周限",
      colorSlot: 2,
      value: null,
      metric: "absolute",
    });
  });
});
