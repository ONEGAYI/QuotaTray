// T-23 试查回退警告 toast 渲染契约（spec #137 / issue #141）：
// 试查成功且回退清单非空时呈现警告——右对齐行（qt-fallback-toast-row）、
// 带关闭按钮（aria-label 可达）、文案列出回退窗口名与回退方向；
// auto 或清单空时组件自判不呈现（调用方只喂偏好与窗口数据）。
// mock i18n 的 t 内联与真实 interpolate 同语义的占位替换
// （vi.mock 工厂 hoisting 不能引用顶层导入，见 ProviderCard 先例）。
// mockLang 可变槽位供测试切换语言（工厂体内仅创建闭包、调用时才读取，
// 与 t 闭包内 zh[key] 的晚绑定同机制）。
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";
import type { UsageData } from "../types";
import { MetricFallbackToast } from "./MetricFallbackToast";

let mockLang: "zh" | "en" = "zh";

vi.mock("../i18n", () => ({
  useLang: () => ({
    lang: mockLang,
    t: (key: keyof typeof zh, params?: Record<string, string | number>) =>
      (mockLang === "zh" ? zh : en)[key].replace(/\{(\w+)\}/g, (match, name: string) =>
        params && name in params ? String(params[name]) : match,
      ),
  }),
}));

beforeEach(() => {
  mockLang = "zh";
});

function render(preference: "auto" | "percent" | "amount", windows: UsageData[]): string {
  return renderToStaticMarkup(
    <MetricFallbackToast preference={preference} windows={windows} onClose={() => {}} />,
  );
}

describe("试查回退警告 toast（T-23）", () => {
  const balanceOnly: UsageData[] = [{ remaining: 62.97, unit: "CNY", plan_name: "MCP 窗口" }];

  it("percent 偏好遇纯余额窗口：文案点名窗口与回退方向，右对齐行 + 关闭按钮", () => {
    const html = render("percent", balanceOnly);
    expect(html).toContain("MCP 窗口无百分比数据，将按金额显示");
    // 占位错配（模板 {windows} 对漏传参）会把字面量漏到界面上
    expect(html).not.toContain("{windows}");
    // 右对齐行 + 关闭按钮（aria-label 可达）
    expect(html).toContain("qt-fallback-toast-row");
    expect(html).toContain(`aria-label="${zh["edit.metricFallbackClose"]}"`);
  });

  it("amount 偏好遇百分比窗口：回退方向文案翻转", () => {
    const html = render("amount", [{ used: 42, unit: "%", plan_name: "5h 窗口" }]);
    expect(html).toContain("5h 窗口无剩余金额，将按百分比显示");
  });

  it("多窗口以顿号连接逐一列出", () => {
    const html = render("percent", [
      { remaining: 1, unit: "CNY", plan_name: "MCP" },
      { remaining: 2, unit: "CNY", plan_name: "备用" },
    ]);
    expect(html).toContain("MCP、备用无百分比数据，将按金额显示");
  });

  it("auto 恒不呈现；清单空（数据全支持偏好）同样不呈现", () => {
    expect(render("auto", balanceOnly)).toBe("");
    expect(render("percent", [{ used: 42, unit: "%" }])).toBe("");
  });

  // PR #146 review 修复：英文原模板 "{windows} has no percent data" 在
  // 多窗口（"MCP, backup has …"）下主谓不一致，措辞改为无谓语句式规避
  it("英文多窗口：无谓语句式点名窗口，规避主谓一致问题（percent 档）", () => {
    mockLang = "en";
    const html = render("percent", [
      { remaining: 1, unit: "CNY", plan_name: "MCP" },
      { remaining: 2, unit: "CNY", plan_name: "backup" },
    ]);
    expect(html).toContain("No percent data for MCP, backup; showing amount instead");
    expect(html).not.toContain("has no percent data");
  });

  it("英文单窗口与 amount 档反向文案同句式（无名窗口取英文序数）", () => {
    mockLang = "en";
    expect(render("percent", [{ remaining: 62.97, unit: "CNY", plan_name: "MCP window" }]))
      .toContain("No percent data for MCP window; showing amount instead");
    const html = render("amount", [
      { used: 42, unit: "%", plan_name: "5h" },
      { used: 10, unit: "%" },
    ]);
    expect(html).toContain("No remaining amount for 5h, window 2; showing percent instead");
    expect(html).not.toContain("has no remaining amount");
  });
});
