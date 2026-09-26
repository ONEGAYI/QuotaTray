import { afterEach, describe, expect, it, vi } from "vitest";
import { dataSummary, exactTime, kindLabel, markerNetText, markerRateText, markerSpanText, markerUnobservedText, metricFallbackWindows, relativeTime, remainingPercent, resetCountdown, usedPercent, windowShortLabel } from "./display";

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

describe("定位线净消耗", () => {
  it("净消耗主数值：正负零清楚，最多 2 位小数去尾零，-0 归零", () => {
    expect(markerNetText(30, "percent", "%")).toBe("30%");
    expect(markerNetText(3.375, "percent", "%")).toBe("3.38%");
    expect(markerNetText(-15, "percent", "%")).toBe("-15%");
    expect(markerNetText(0.2, "percent", "%")).toBe("0.2%");
    // 舍入到 0 的微弱净值显示 0，不得出现 "-0"
    expect(markerNetText(-0.004, "percent", "%")).toBe("0%");
    expect(markerNetText(0, "percent", "%")).toBe("0%");
    expect(markerNetText(11.25, "absolute", "CNY")).toBe("11.25 CNY");
    expect(markerNetText(-29.75, "absolute", "credits")).toBe("-29.75 credits");
    expect(markerNetText(1.2, "absolute", "")).toBe("1.2");
  });

  it("未观测净变化恒带符号，零值无符号", () => {
    expect(markerUnobservedText(18, "percent", "%")).toBe("+18%");
    expect(markerUnobservedText(-29.75, "absolute", "CNY")).toBe("-29.75 CNY");
    expect(markerUnobservedText(0, "percent", "%")).toBe("0%");
    // 舍入到 0 的微弱未观测变化不带符号（零无方向）
    expect(markerUnobservedText(-0.004, "percent", "%")).toBe("0%");
    expect(markerUnobservedText(0.004, "percent", "%")).toBe("0%");
  });
});

describe("已用与剩余百分比口径", () => {
  it("usedPercent：'%' 直读，金额走 used/total 换算，数据不足 null", () => {
    expect(usedPercent({ used: 42, unit: "%" })).toBe(42);
    expect(usedPercent({ used: 30, total: 200, unit: "USD" })).toBe(15);
    expect(usedPercent({ used: 10, total: 0 })).toBeNull();
    expect(usedPercent({})).toBeNull();
    expect(usedPercent({ unit: "%" })).toBeNull();
  });

  it("remainingPercent：与 core remaining_percent 镜像——100−已用，数据不足 null", () => {
    // "%" 单位：100 − used（订阅/限额窗口的剩余百分比）
    expect(remainingPercent({ used: 42, unit: "%" })).toBe(58);
    // 金额单位：100 − used/total 换算
    expect(remainingPercent({ used: 30, total: 200, unit: "USD" })).toBe(85);
    // 与 usedPercent 互补：两口径之和恒为 100
    expect(
      (usedPercent({ used: 30, total: 200, unit: "USD" }) ?? 0) +
        (remainingPercent({ used: 30, total: 200, unit: "USD" }) ?? 0),
    ).toBe(100);
    // 数据不足同 usedPercent：total<=0、字段缺失、'%' 缺 used
    expect(remainingPercent({ used: 10, total: 0 })).toBeNull();
    expect(remainingPercent({})).toBeNull();
    expect(remainingPercent({ unit: "%" })).toBeNull();
  });
});

describe("单窗口主文案 dataSummary（剩余口径，T-22）", () => {
  it("能算百分比 → 剩余 N%（与 tray.rs remaining_percent_text 成对：zh 剩余 / en Left）", () => {
    // '%' 直读 used 后取补：used 42 → 剩余 58
    expect(dataSummary({ used: 42, unit: "%" }, "zh")).toBe("剩余 58%");
    expect(dataSummary({ used: 42, unit: "%" }, "en")).toBe("Left 58%");
    // 金额窗口：used/total 换算后取补（30/200 = 15% 已用 → 85% 剩余）
    expect(dataSummary({ used: 30, total: 200, unit: "USD" }, "zh")).toBe("剩余 85%");
    expect(dataSummary({ used: 30, total: 200, unit: "USD" }, "en")).toBe("Left 85%");
  });

  it("无百分比有 remaining → 剩余金额（与 tray.rs remaining_text 成对，不变）", () => {
    expect(dataSummary({ remaining: 62.97, unit: "CNY" }, "zh")).toBe("剩余 62.97 CNY");
    expect(dataSummary({ remaining: 62.97, unit: "CNY" }, "en")).toBe("Left 62.97 CNY");
    expect(dataSummary({ remaining: 5 }, "zh")).toBe("剩余 5.00");
  });

  it("两者皆缺 → 已获取回退（双语不变）", () => {
    expect(dataSummary({ used: 10 }, "zh")).toBe("已获取");
    expect(dataSummary({ used: 10 }, "en")).toBe("Fetched");
  });
});

describe("主度量偏好分档 dataSummary（T-24，与 tray.rs entry_lines 成对）", () => {
  // 两者皆可的形态：used/total 可换算百分比 + remaining 有值
  const both = { used: 30, total: 200, remaining: 62.97, unit: "CNY" };

  it("auto/percent 档维持推断基线：百分比优先（现状顺序不回归）", () => {
    expect(dataSummary(both, "zh", "auto")).toBe("剩余 85%");
    expect(dataSummary(both, "en", "auto")).toBe("Left 85%");
    expect(dataSummary(both, "zh", "percent")).toBe("剩余 85%");
    expect(dataSummary(both, "en", "percent")).toBe("Left 85%");
  });

  it("amount 档金额文案优先——即使可算百分比（本 spec 原始诉求：余额型主看金额）", () => {
    expect(dataSummary(both, "zh", "amount")).toBe("剩余 62.97 CNY");
    expect(dataSummary(both, "en", "amount")).toBe("Left 62.97 CNY");
  });

  it("指定度量算不出时静默回退另一度量（逐窗口独立判定）", () => {
    // amount 档无 remaining → 回退剩余百分比
    expect(dataSummary({ used: 42, unit: "%" }, "zh", "amount")).toBe("剩余 58%");
    expect(dataSummary({ used: 42, unit: "%" }, "en", "amount")).toBe("Left 58%");
    // percent 档算不出百分比 → 回退金额
    expect(dataSummary({ remaining: 62.97, unit: "CNY" }, "zh", "percent")).toBe("剩余 62.97 CNY");
    expect(dataSummary({ remaining: 62.97, unit: "CNY" }, "en", "percent")).toBe("Left 62.97 CNY");
  });

  it("两度量皆缺：各档统一已获取回退（双语）", () => {
    expect(dataSummary({ used: 10 }, "zh", "amount")).toBe("已获取");
    expect(dataSummary({ used: 10 }, "en", "percent")).toBe("Fetched");
  });
});

describe("主度量偏好回退检测 metricFallbackWindows（T-23，spec #137）", () => {
  it("auto 恒空清单：按数据推断无回退概念，即使数据完全不支持百分比", () => {
    const balanceOnly: import("./types").UsageData[] = [
      { remaining: 62.97, unit: "CNY", plan_name: "余额" },
    ];
    expect(metricFallbackWindows("auto", balanceOnly, "zh")).toEqual([]);
  });

  it("percent 偏好：无百分比原材料（remainingPercent 算不出）的窗口回退金额", () => {
    // 纯余额窗口算不出剩余百分比 → 回退
    expect(
      metricFallbackWindows(
        "percent",
        [{ remaining: 62.97, unit: "CNY", plan_name: "MCP 窗口" }],
        "zh",
      ),
    ).toEqual(["MCP 窗口"]);
    // '%' 直读与 used/total 换算两条百分比原材料路径都算得出 → 不回退
    expect(
      metricFallbackWindows(
        "percent",
        [{ used: 42, unit: "%" }, { used: 30, total: 200, unit: "USD" }],
        "zh",
      ),
    ).toEqual([]);
  });

  it("amount 偏好：无 remaining 的窗口回退百分比", () => {
    expect(
      metricFallbackWindows(
        "amount",
        [{ used: 42, unit: "%", plan_name: "5h 窗口" }],
        "zh",
      ),
    ).toEqual(["5h 窗口"]);
    // 有 remaining（含可换算出 remaining 的金额窗口）→ 不回退
    expect(
      metricFallbackWindows("amount", [{ remaining: 5, plan_name: "余额" }], "zh"),
    ).toEqual([]);
  });

  it("混合窗口只列回退者；窗口名取 plan_name、无名回退序数（双语）", () => {
    const windows: import("./types").UsageData[] = [
      { used: 42, unit: "%", plan_name: "GLM Coding Plan（5h）" },
      { remaining: 3.2, unit: "CNY" },
      { used: 1, total: 10, unit: "CNY", plan_name: "月度" },
    ];
    // 第 1、3 窗口有百分比原材料（'%' 直读 / used÷total 换算），仅第 2 回退
    expect(metricFallbackWindows("percent", windows, "zh")).toEqual(["窗口 2"]);
    expect(metricFallbackWindows("percent", windows, "en")).toEqual(["window 2"]);
  });

  it("回退目标也算不出（两度量皆缺）的窗口不列入：数据不足非回退（PR #146 review）", () => {
    // 裸已用（无 total 换不出百分比、无 remaining）：percent 偏好下回退目标
    // （金额）同样缺，展示层走已获取兜底——toast 不得预告"将按金额显示"
    expect(metricFallbackWindows("percent", [{ used: 10, plan_name: "裸已用" }], "zh"))
      .toEqual([]);
    // amount 偏好镜像：回退目标（百分比）算不出同样不列入
    expect(metricFallbackWindows("amount", [{ used: 10, plan_name: "裸已用" }], "zh"))
      .toEqual([]);
    // 混合：可回退者照列、两缺者剔除、偏好直接可算者不列
    expect(
      metricFallbackWindows(
        "percent",
        [
          { remaining: 1, unit: "CNY", plan_name: "MCP" },
          { used: 10 },
          { used: 42, unit: "%" },
        ],
        "zh",
      ),
    ).toEqual(["MCP"]);
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
