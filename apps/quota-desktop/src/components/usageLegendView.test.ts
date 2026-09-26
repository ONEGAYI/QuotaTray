import { describe, expect, it } from "vitest";
import { buildLegendItems, createLegendHoverController, focusPlatformInfo, LEGEND_CLOSE_GRACE_MS, legendTriggerVisible, pressLegendRemove, toggleSeriesFocus } from "./usageLegendView";

function fakeTimers() {
  // 对齐浏览器语义：fire 之后的 clear 是 no-op（clearTimeout 对已触发的 id 无作用）
  const entries: { id: number; ms: number; fire: () => void; cleared: boolean; fired: boolean }[] = [];
  let seq = 0;
  const timers = {
    set: (callback: () => void, ms: number) => {
      const id = ++seq;
      entries.push({ id, ms, fire: callback, cleared: false, fired: false });
      return id;
    },
    clear: (id: number) => {
      const entry = entries.find((item) => item.id === id);
      if (entry && !entry.fired) entry.cleared = true;
    },
  };
  const fireDue = () => {
    for (const entry of [...entries]) if (!entry.cleared && !entry.fired) { entry.fired = true; entry.fire(); }
  };
  return { timers, entries, fireDue };
}

describe("悬停浮层收起宽限（移出后延迟收起，期间回来即取消）", () => {
  it("移出触发区后到点收恰一次", () => {
    const fake = fakeTimers();
    let closed = 0;
    const controller = createLegendHoverController(() => { closed += 1; }, fake.timers);
    controller.scheduleClose();
    fake.fireDue();
    expect(closed).toBe(1);
  });

  it("登记的延迟时长为 LEGEND_CLOSE_GRACE_MS", () => {
    const fake = fakeTimers();
    const controller = createLegendHoverController(() => {}, fake.timers);
    controller.scheduleClose();
    expect(fake.entries[fake.entries.length - 1]?.ms).toBe(LEGEND_CLOSE_GRACE_MS);
  });

  it("宽限期内回到触发区或浮层，取消待执行的收起", () => {
    const fake = fakeTimers();
    let closed = 0;
    const controller = createLegendHoverController(() => { closed += 1; }, fake.timers);
    controller.scheduleClose();
    controller.cancelClose();
    fake.fireDue();
    expect(closed).toBe(0);
  });

  it("连续移出（空隙间往返多次触发 mouseleave）只保留最后一笔，旧倒计时被清除", () => {
    const fake = fakeTimers();
    let closed = 0;
    const controller = createLegendHoverController(() => { closed += 1; }, fake.timers);
    controller.scheduleClose();
    controller.scheduleClose();
    expect(fake.entries[0].cleared).toBe(true);
    fake.fireDue();
    expect(closed).toBe(1);
  });

  it("组件卸载（dispose）后不再收起，且后续调度不再登记倒计时", () => {
    const fake = fakeTimers();
    let closed = 0;
    const controller = createLegendHoverController(() => { closed += 1; }, fake.timers);
    controller.scheduleClose();
    controller.dispose();
    controller.scheduleClose();
    expect(fake.entries.every((entry) => entry.cleared)).toBe(true);
    fake.fireDue();
    expect(closed).toBe(0);
  });

  it("未调度时取消与收起互不干扰（no-op 安全）", () => {
    const fake = fakeTimers();
    const controller = createLegendHoverController(() => {}, fake.timers);
    expect(() => controller.cancelClose()).not.toThrow();
    expect(fake.entries).toHaveLength(0);
  });

  it("close 回调内自取消（closeLegend 首行 cancelClose 的生产形态）：恰收起一次且后续调度不受污染", () => {
    const fake = fakeTimers();
    let closed = 0;
    const controller = createLegendHoverController(() => { closed += 1; controller.cancelClose(); }, fake.timers);
    controller.scheduleClose();
    fake.fireDue();
    expect(closed).toBe(1);
    controller.scheduleClose();
    fake.fireDue();
    expect(closed).toBe(2);
  });

  it("移出→宽限内返回→再移出→到期：整链恰收起一次", () => {
    const fake = fakeTimers();
    let closed = 0;
    const controller = createLegendHoverController(() => { closed += 1; }, fake.timers);
    controller.scheduleClose();
    controller.cancelClose();
    controller.scheduleClose();
    fake.fireDue();
    expect(closed).toBe(1);
  });
});

describe("使用统计聚焦组合 popover 逻辑", () => {
  it("已有组合时显示入口（全平台统一药丸，移动端开模态窗）", () => {
    expect(legendTriggerVisible(1)).toBe(true);
    expect(legendTriggerVisible(4)).toBe(true);
  });

  it("没有可比组合时不显示入口", () => {
    expect(legendTriggerVisible(0)).toBe(false);
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
    { provider_id: "p1", window_key: "w1", color_slot: 0, metric: "percent" as const },
    { provider_id: "gone", window_key: "stale", color_slot: 2 },
  ];
  const idOf = (providerId: string, windowKey: string, metric?: "percent" | "absolute") => JSON.stringify([providerId, windowKey, metric]);

  it("按 selection 全量生成行，可见曲线标记 available", () => {
    const items = buildLegendItems(
      selections,
      new Set([idOf("p1", "w1", "percent")]),
      new Map([[idOf("p1", "w1", "percent"), "P1 · 窗口 1"]]),
    );
    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({ id: idOf("p1", "w1", "percent"), providerId: "p1", windowKey: "w1", metric: "percent", colorSlot: 0, available: true, name: "P1 · 窗口 1" });
  });

  it("失效条目 available=false，候选缺名时回退原始 id 展示", () => {
    const items = buildLegendItems(selections, new Set([idOf("p1", "w1", "percent")]), new Map());
    expect(items[1]).toMatchObject({ id: idOf("gone", "stale"), providerId: "gone", windowKey: "stale", colorSlot: 2, available: false, name: "gone · stale" });
  });

  it("顺序与 selection 存储顺序一致", () => {
    const items = buildLegendItems(selections, new Set(), new Map());
    expect(items.map((item) => item.id)).toEqual([idOf("p1", "w1", "percent"), idOf("gone", "stale")]);
  });
});

describe("聚焦平台取数（卡头药丸内下陷显示窗）", () => {
  const items = buildLegendItems(
    [
      { provider_id: "p1", window_key: "w1", color_slot: 0, metric: "percent" as const },
      { provider_id: "p2", window_key: "w2", color_slot: 2, metric: "absolute" as const },
    ],
    new Set([JSON.stringify(["p1", "w1", "percent"]), JSON.stringify(["p2", "w2", "absolute"])]),
    new Map([
      [JSON.stringify(["p1", "w1", "percent"]), "P1 · 窗口 1"],
      [JSON.stringify(["p2", "w2", "absolute"]), "P2 · 周限"],
    ]),
  );
  const scopes = [
    { id: JSON.stringify(["p1", "w1", "percent"]), metric: "percent" as const, samples: [{ value: 41.2 }, { value: 78 }] },
    { id: JSON.stringify(["p2", "w2", "absolute"]), metric: "absolute" as const, samples: [] },
  ];

  it("未聚焦或聚焦项不在行列表时返回 null", () => {
    expect(focusPlatformInfo(null, items, scopes)).toBeNull();
    expect(focusPlatformInfo("gone", items, scopes)).toBeNull();
  });

  it("聚焦时取行名称、色槽与最新样本值", () => {
    expect(focusPlatformInfo(JSON.stringify(["p1", "w1", "percent"]), items, scopes)).toEqual({
      name: "P1 · 窗口 1",
      colorSlot: 0,
      value: 78,
      metric: "percent",
    });
  });

  it("组合暂无样本时值为 null 但保留名称与度量", () => {
    expect(focusPlatformInfo(JSON.stringify(["p2", "w2", "absolute"]), items, scopes)).toEqual({
      name: "P2 · 周限",
      colorSlot: 2,
      value: null,
      metric: "absolute",
    });
  });

  it("行列表有该项但 scopes 暂缺（聚焦切换瞬态）时值为空、度量回退 percent", () => {
    expect(focusPlatformInfo(JSON.stringify(["p1", "w1", "percent"]), items, [])).toEqual({
      name: "P1 · 窗口 1",
      colorSlot: 0,
      value: null,
      metric: "percent",
    });
  });
});
