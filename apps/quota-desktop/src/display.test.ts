import { afterEach, describe, expect, it, vi } from "vitest";
import { exactTime, relativeTime, resetCountdown, windowShortLabel } from "./display";

describe("最后成功时间展示", () => {
  afterEach(() => vi.useRealTimers());

  it("按既有分档生成人类可读的相对时间", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-23T08:00:00.000Z"));

    expect(relativeTime(Date.now() - 5_000, "zh")).toBe("刚刚");
    expect(relativeTime(Date.now() - 150_000, "zh")).toBe("2 分钟前");
    expect(relativeTime(Date.now() - 7_200_000, "en")).toBe("2h ago");
  });

  it("Tooltip 的精确时间包含完整本地日期与秒", () => {
    const at = new Date(2026, 7, 23, 16, 5, 9).getTime();
    const zh = exactTime(at, "zh");
    const en = exactTime(at, "en");

    for (const part of ["2026", "08", "23", "16", "05", "09"]) {
      expect(zh).toContain(part);
    }
    expect(en).toContain("2026");
    expect(en).toContain("09");
  });
});

describe("额度重置倒计时", () => {
  const NOW = Date.parse("2026-08-23T08:00:00.000Z");
  const mins = (n: number) => NOW + n * 60_000;

  it("缺省或已到期返回 null（无展示意义）", () => {
    expect(resetCountdown(undefined, NOW)).toBeNull();
    expect(resetCountdown(null, NOW)).toBeNull();
    expect(resetCountdown(NOW, NOW)).toBeNull();
    expect(resetCountdown(NOW - 1, NOW)).toBeNull();
  });

  it("按窗口量级分档：分钟 / 时+分 / 天+时", () => {
    expect(resetCountdown(mins(21), NOW)).toBe("21m");
    expect(resetCountdown(mins(201), NOW)).toBe("3h21m");
    expect(resetCountdown(mins(180), NOW)).toBe("3h");
    expect(resetCountdown(mins(4 * 24 * 60 + 17 * 60), NOW)).toBe("4d17h");
    expect(resetCountdown(mins(4 * 24 * 60), NOW)).toBe("4d");
    // 跨入天级后丢弃分钟粒度（周/月窗口小时精度已足够）
    expect(resetCountdown(mins(24 * 60 + 17), NOW)).toBe("1d");
  });

  it("缺省 now 参数时使用当前时刻", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(NOW));
    expect(resetCountdown(mins(21))).toBe("21m");
    vi.useRealTimers();
  });
});

describe("多窗口短标签", () => {
  it("提取 plan_name 的全角括号内容，week 映射为双语", () => {
    expect(windowShortLabel("GLM Coding Plan（5h）", 0, "zh")).toBe("5h");
    expect(windowShortLabel("GLM Coding Plan（MCP）", 2, "en")).toBe("MCP");
    expect(windowShortLabel("GLM Coding Plan（week）", 1, "zh")).toBe("周限");
    expect(windowShortLabel("GLM Coding Plan（week）", 1, "en")).toBe("weekly");
  });

  it("无括号用全名，无名回退窗口序号", () => {
    expect(windowShortLabel("five_hour", 0, "zh")).toBe("five_hour");
    expect(windowShortLabel(undefined, 1, "zh")).toBe("窗口 2");
    expect(windowShortLabel(undefined, 0, "en")).toBe("window 1");
  });
});
