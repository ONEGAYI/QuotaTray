// 悬停浮窗渲染契约（PR #146 review 修复项 1）：用量列表进度条方向。
// 同一行的主文案已是「剩余 N%」（dataSummary 剩余口径）、圆环也是剩余
// 口径——进度条填充必须三口径一致：剩余越多填充越多（used 42 → 58%）。
// 渲染链路与 ProviderCard.test 同构：mock queries 走快照分支注入数据。
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";
import type { ProviderEntry, SnapshotEntry, UsageData } from "../types";
import HoverPanel from "./HoverPanel";

// 快照数据经 vi.hoisted 供 mock 工厂读取（工厂先于模块求值，闭包变量会 TDZ）
const ctx = vi.hoisted(() => ({
  lang: "zh" as "zh" | "en",
  snapshots: {} as Record<string, SnapshotEntry>,
}));

vi.mock("../i18n", () => ({
  LangProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
  useLang: () => ({
    lang: ctx.lang,
    t: (key: keyof typeof zh) => (ctx.lang === "zh" ? zh : en)[key],
  }),
}));
vi.mock("../theme", () => ({
  ThemeProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
}));
vi.mock("../api", () => ({
  api: {
    hideHoverPanel: vi.fn(),
    setHoverPanelPointerInside: vi.fn(),
    openMainWindow: vi.fn(),
    queryProvider: vi.fn(),
    patchSettings: vi.fn(),
    upsertProvider: vi.fn(),
  },
}));
vi.mock("../queries", () => ({
  useProviderStateEvents: () => {},
  useProviders: () => ({
    data: [
      {
        id: "p1",
        name: "账户",
        kind: { type: "native", provider: "deepseek" },
        enabled: true,
      } satisfies ProviderEntry,
    ],
  }),
  useSettings: () => ({ data: undefined }),
  useSnapshots: () => ({ data: ctx.snapshots }),
  useNativeMetas: () => ({ data: undefined }),
  useProviderState: () => ({ data: undefined, isFetching: false }),
  usePeakFlipTick: () => Date.UTC(2026, 8, 26),
}));

// HoverPanel 渲染期读取 window.innerHeight（压缩布局判定）——Node 环境
// 无 window，桩出完整高度走非压缩分支（用量列表可见）
beforeEach(() => {
  vi.stubGlobal("window", { innerHeight: 520 });
  ctx.lang = "zh";
});
afterEach(() => vi.unstubAllGlobals());

/** 快照注入渲染：deriveProviderCardState 的 snapshot 分支（与主页真实链路同构）。 */
function renderPanel(data: UsageData[]): string {
  ctx.snapshots = {
    p1: { data, at: Date.UTC(2026, 8, 26, 12, 0, 0) },
  };
  return renderToStaticMarkup(<HoverPanel />);
}

describe("用量列表进度条方向（剩余填充，PR #146 review 修复项 1）", () => {
  it("进度条按剩余比例填充：used 42 → width 58%，与同行文案「剩余 58%」、圆环口径一致", () => {
    const html = renderPanel([{ used: 42, unit: "%", plan_name: "GLM Coding Plan（5h）" }]);
    // 同行文案剩余口径（不回归）
    expect(html).toContain("剩余 58%");
    // 填充与可访问值同为剩余：58 而非已用 42
    expect(html).toMatch(/<div class="qt-hover-progress"[^>]*aria-valuenow="58"/);
    expect(html).toContain(`style="width:58%"`);
    expect(html).not.toContain(`style="width:42%"`);
  });

  it("多窗口各按自身剩余比例填充（5h 剩 58、week 剩 20）", () => {
    const html = renderPanel([
      { used: 42, unit: "%", plan_name: "GLM Coding Plan（5h）" },
      { used: 80, unit: "%", plan_name: "GLM Coding Plan（week）" },
    ]);
    expect(html).toContain(`style="width:58%"`);
    expect(html).toContain(`style="width:20%"`);
    expect(html).not.toContain(`style="width:42%"`);
    expect(html).not.toContain(`style="width:80%"`);
  });

  it("纯金额窗口无百分比原材料，不渲染进度条", () => {
    const html = renderPanel([{ remaining: 62.97, unit: "CNY" }]);
    expect(html).not.toContain("qt-hover-progress");
  });
});
