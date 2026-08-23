import { afterEach, describe, expect, it, vi } from "vitest";
import { exactTime, relativeTime } from "./display";

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
