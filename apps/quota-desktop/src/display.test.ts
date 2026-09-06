import { afterEach, describe, expect, it, vi } from "vitest";
import { exactTime, kindLabel, markerRateText, markerSpanText, relativeTime, resetCountdown, windowShortLabel } from "./display";

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

describe("定位线时间差", () => {
  const mins = (n: number) => n * 60_000;

  it("分钟粒度向下取整且至少 1 分钟，零值段省略", () => {
    expect(markerSpanText(20_000, "zh")).toBe("1分");
    expect(markerSpanText(20_000, "en")).toBe("1m");
    expect(markerSpanText(mins(45), "zh")).toBe("45分");
    expect(markerSpanText(mins(90), "en")).toBe("1h 30m");
    expect(markerSpanText(mins(24 * 60 + 15), "zh")).toBe("1天15分");
    expect(markerSpanText(mins(2 * 24 * 60 + 3 * 60 + 15), "zh")).toBe("2天3小时15分");
    expect(markerSpanText(mins(2 * 24 * 60 + 3 * 60 + 15), "en")).toBe("2d 3h 15m");
  });
});

describe("定位线平均消耗速率", () => {
  it("最多 2 位小数并去尾零，负值保留符号表示回升", () => {
    expect(markerRateText(15, "percent", "%")).toBe("15%/h");
    expect(markerRateText(3.375, "percent", "%")).toBe("3.38%/h");
    expect(markerRateText(0.2, "percent", "%")).toBe("0.2%/h");
    expect(markerRateText(-3, "percent", "%")).toBe("-3%/h");
    expect(markerRateText(-0.21, "percent", "%")).toBe("-0.21%/h");
    expect(markerRateText(-0.2, "percent", "%")).toBe("-0.2%/h");
    // 舍入到 0 的微弱回升显示 0，不得出现 "-0"
    expect(markerRateText(-0.004, "percent", "%")).toBe("0%/h");
    expect(markerRateText(0.004, "percent", "%")).toBe("0%/h");
  });

  it("余额序列带绝对单位，单位为空时仅剩每时值", () => {
    expect(markerRateText(3.5, "absolute", "CNY")).toBe("3.5 CNY/h");
    expect(markerRateText(1234.567, "absolute", "credits")).toBe("1234.57 credits/h");
    expect(markerRateText(1.234, "absolute", "")).toBe("1.23/h");
  });
});

describe("多窗口短标签", () => {
  it("提取 plan_name 的全角括号内容，week 映射为双语", () => {
    expect(windowShortLabel("GLM Coding Plan（5h）", 0, "zh")).toBe("5h");
    expect(windowShortLabel("GLM Coding Plan（MCP）", 2, "en")).toBe("MCP");
    expect(windowShortLabel("GLM Coding Plan（week）", 1, "zh")).toBe("周限");
    expect(windowShortLabel("GLM Coding Plan（week）", 1, "en")).toBe("weekly");
    expect(windowShortLabel("Kimi Code（5h）", 0, "zh")).toBe("5h");
    expect(windowShortLabel("Kimi Code（week）", 1, "en")).toBe("weekly");
  });

  it("无括号用全名，无名回退窗口序号", () => {
    expect(windowShortLabel("five_hour", 0, "zh")).toBe("five_hour");
    expect(windowShortLabel(undefined, 1, "zh")).toBe("窗口 2");
    expect(windowShortLabel(undefined, 0, "en")).toBe("window 1");
  });
});

describe("条目类型标签", () => {
  it("native 用平台名（元数据缺失回退 provider id）", () => {
    const kind = { type: "native", provider: "deepseek" } as const;
    expect(kindLabel(kind, "DeepSeek", "zh")).toBe("DeepSeek");
    expect(kindLabel(kind, undefined, "en")).toBe("deepseek");
  });

  it("模板与脚本各归各——script 不得落入模板文案", () => {
    expect(kindLabel({ type: "template", request: { url: "https://a.com" }, extract: {} }, undefined, "zh")).toBe("模板");
    expect(kindLabel({ type: "script", code: "" }, undefined, "zh")).toBe("脚本");
    expect(kindLabel({ type: "script", code: "" }, undefined, "en")).toBe("script");
  });
});
