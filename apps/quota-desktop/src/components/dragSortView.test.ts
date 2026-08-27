import { describe, expect, it } from "vitest";
import {
  computeSettleOffset,
  computeShifts,
  computeTargetIndex,
  nextKeyboardTarget,
  reorderIds,
  SETTLE_DURATION_MAX_MS,
  SETTLE_DURATION_MIN_MS,
  settleDuration,
  velocityFromSamples,
} from "./dragSortView";

/** 均匀高度卡片列（h=100、gap=10）：tops = 0 / 110 / 220 / 330，中心 = top+50。 */
const evenRects = [
  { top: 0, height: 100 },
  { top: 110, height: 100 },
  { top: 220, height: 100 },
  { top: 330, height: 100 },
];

/** 高度参差卡片列：A(h80)@0、B(h120)@90、C(h80)@220，中心 = 40 / 150 / 260。 */
const unevenRects = [
  { top: 0, height: 80 },
  { top: 90, height: 120 },
  { top: 220, height: 80 },
];

describe("computeTargetIndex", () => {
  it("未越过任何邻卡中心时保持在原位", () => {
    // 拖 idx0：中心 50+60=110，仍低于 B 中心 160
    expect(computeTargetIndex(evenRects, 0, 50 + 60)).toBe(0);
  });

  it("越过邻卡中心即交换槽位", () => {
    // 中心 170 > B 中心 160 → 目标 1
    expect(computeTargetIndex(evenRects, 0, 50 + 120)).toBe(1);
  });

  it("大幅拖出列表两端时夹取到首尾", () => {
    expect(computeTargetIndex(evenRects, 0, 50 + 5000)).toBe(3);
    expect(computeTargetIndex(evenRects, 3, 380 - 5000)).toBe(0);
  });

  it("向上拖越过上方邻卡中心时目标前移", () => {
    // 拖 idx2（中心 270）：上移 130 → 中心 140 < B 中心 160 → 目标 1
    expect(computeTargetIndex(evenRects, 2, 270 - 130)).toBe(1);
    // 再上移越过 A 中心 50 → 目标 0
    expect(computeTargetIndex(evenRects, 2, 40)).toBe(0);
  });

  it("高度参差时按各自中点判定（高卡需要拖得更远才交换）", () => {
    // 拖 B（中心 150）：上移 60 → 中心 90 仍 > A 中心 40 → 原地
    expect(computeTargetIndex(unevenRects, 1, 150 - 60)).toBe(1);
    // 上移 115 → 中心 35 < A 中心 40 → 目标 0
    expect(computeTargetIndex(unevenRects, 1, 150 - 115)).toBe(0);
    // 下移 120 → 中心 270 > C 中心 260 → 目标 2
    expect(computeTargetIndex(unevenRects, 1, 150 + 120)).toBe(2);
  });
});

describe("computeShifts", () => {
  it("向下拖一格：区间内卡片各上移一个槽位", () => {
    // [A,B,C] 拖 A(0)→1 得 [B,A,C]：B 挪到槽 0（top 0-90=-90），C 不动
    expect(computeShifts(unevenRects, 0, 1)).toEqual([0, -90, 0]);
  });

  it("向下拖两格：跨过的卡片依各自槽位差上移", () => {
    // [A,B,C] 拖 A(0)→2 得 [B,C,A]：B →槽0（-90）、C →槽1（90-220=-130）
    expect(computeShifts(unevenRects, 0, 2)).toEqual([0, -90, -130]);
  });

  it("向上拖：让出槽位的卡片依各自槽位差下移", () => {
    // [A,B,C] 拖 C(2)→0 得 [C,A,B]：C →槽0（0-220=-220）、A →槽1（+90）、B →槽2（+130）
    expect(computeShifts(unevenRects, 2, 0)).toEqual([90, 130, 0]);
  });

  it("目标槽位等于原位时全部归零", () => {
    expect(computeShifts(unevenRects, 1, 1)).toEqual([0, 0, 0]);
  });
});

describe("computeSettleOffset", () => {
  it("落位偏移 = 目标槽位 top - 自身静态 top", () => {
    expect(computeSettleOffset(unevenRects, 0, 2)).toBe(220);
    expect(computeSettleOffset(unevenRects, 2, 0)).toBe(-220);
    expect(computeSettleOffset(unevenRects, 1, 1)).toBe(0);
  });
});

describe("reorderIds", () => {
  it("把 from 位元素移动到 to 位，其余依序滑动补位", () => {
    expect(reorderIds(["a", "b", "c", "d"], 0, 2)).toEqual(["b", "c", "a", "d"]);
    expect(reorderIds(["a", "b", "c", "d"], 3, 0)).toEqual(["d", "a", "b", "c"]);
    expect(reorderIds(["a", "b", "c"], 1, 1)).toEqual(["a", "b", "c"]);
  });

  it("返回新数组，不改动原数组", () => {
    const input = ["a", "b", "c"];
    const output = reorderIds(input, 0, 2);
    expect(output).not.toBe(input);
    expect(input).toEqual(["a", "b", "c"]);
  });

  it("非法索引显式抛错（调用方负责 clamp）", () => {
    expect(() => reorderIds(["a", "b"], -1, 0)).toThrow(RangeError);
    expect(() => reorderIds(["a", "b"], 0, 2)).toThrow(RangeError);
    expect(() => reorderIds([], 0, 0)).toThrow(RangeError);
  });
});

describe("settleDuration", () => {
  it("静止松手取基准时长", () => {
    expect(settleDuration(0)).toBe(SETTLE_DURATION_MIN_MS);
  });

  it("甩得越快滑行越久，且夹取在上下限内", () => {
    const slow = settleDuration(0.5);
    const fast = settleDuration(3);
    expect(slow).toBeGreaterThan(SETTLE_DURATION_MIN_MS);
    expect(fast).toBeGreaterThan(slow);
    expect(settleDuration(100)).toBe(SETTLE_DURATION_MAX_MS);
  });

  it("速度取绝对值：向上甩与向下甩同等待遇", () => {
    expect(settleDuration(-3)).toBe(settleDuration(3));
  });
});

describe("nextKeyboardTarget", () => {
  it("ArrowUp/Down 逐步移动，Home/End 跳首尾", () => {
    expect(nextKeyboardTarget(4, 2, "ArrowUp")).toBe(1);
    expect(nextKeyboardTarget(4, 2, "ArrowDown")).toBe(3);
    expect(nextKeyboardTarget(4, 2, "Home")).toBe(0);
    expect(nextKeyboardTarget(4, 1, "End")).toBe(3);
  });

  it("边界已到头或原地跳转返回 null（调用方据此不提交）", () => {
    expect(nextKeyboardTarget(4, 0, "ArrowUp")).toBeNull();
    expect(nextKeyboardTarget(4, 3, "ArrowDown")).toBeNull();
    expect(nextKeyboardTarget(4, 0, "Home")).toBeNull();
    expect(nextKeyboardTarget(4, 3, "End")).toBeNull();
  });

  it("非方向键返回 null", () => {
    expect(nextKeyboardTarget(4, 1, "Enter")).toBeNull();
    expect(nextKeyboardTarget(4, 1, "ArrowLeft")).toBeNull();
  });
});

describe("velocityFromSamples", () => {
  it("取样本窗口首尾斜率（px/ms）", () => {
    expect(velocityFromSamples([{ t: 0, y: 100 }, { t: 50, y: 90 }, { t: 100, y: 80 }])).toBeCloseTo(-0.2);
    expect(velocityFromSamples([{ t: 0, y: 0 }, { t: 100, y: 250 }])).toBeCloseTo(2.5);
  });

  it("样本不足或零时间跨度返回 0", () => {
    expect(velocityFromSamples([])).toBe(0);
    expect(velocityFromSamples([{ t: 0, y: 100 }])).toBe(0);
    expect(velocityFromSamples([{ t: 100, y: 100 }, { t: 100, y: 130 }])).toBe(0);
  });
});
