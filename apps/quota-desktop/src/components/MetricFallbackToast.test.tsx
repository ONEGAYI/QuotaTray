// T-23 试查回退警告 toast 渲染契约（spec #137 / issue #141）：
// 试查成功且回退清单非空时呈现警告——右对齐行（qt-fallback-toast-row）、
// 带关闭按钮（aria-label 可达）、文案列出回退窗口名与回退方向；
// auto 或清单空时组件自判不呈现（调用方只喂偏好与窗口数据）。
// mock i18n 的 t 内联与真实 interpolate 同语义的占位替换
// （vi.mock 工厂 hoisting 不能引用顶层导入，见 ProviderCard 先例）。
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { zh } from "../i18n/zh";
import type { UsageData } from "../types";
import { MetricFallbackToast } from "./MetricFallbackToast";

vi.mock("../i18n", () => ({
  useLang: () => ({
    lang: "zh",
    t: (key: keyof typeof zh, params?: Record<string, string | number>) =>
      zh[key].replace(/\{(\w+)\}/g, (match, name: string) =>
        params && name in params ? String(params[name]) : match,
      ),
  }),
}));

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
});
