// 悬停浮窗渲染契约。
// 渲染链路与 ProviderCard.test 同构：mock queries 走快照分支注入数据，
// 用 renderToStaticMarkup 断言输出（仓库无 DOM 测试库，交互语义下沉到
// hoverPanelView 纯函数层覆盖）。
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";
import type { ProviderEntry, SnapshotEntry, UsageData } from "../types";
import HoverPanel from "./HoverPanel";

// 数据经 vi.hoisted 供 mock 工厂读取（工厂先于模块求值，闭包变量会 TDZ）
const ctx = vi.hoisted(() => ({
  lang: "zh" as "zh" | "en",
  snapshots: {} as Record<string, SnapshotEntry>,
  providers: [] as ProviderEntry[],
  mode: "single" as "multi" | "single",
  trayIconEntryId: null as string | null,
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
  useRefreshNow: () => {},
  useHoverPanelShown: () => {},
  useHoverPanelHidden: () => {},
  useProviders: () => ({ data: ctx.providers }),
  useSettings: () => ({
    data: {
      hover_panel_mode: ctx.mode,
      tray_icon_entry_id: ctx.trayIconEntryId,
      ring_units_per_circle: 100,
      low_balance_remaining_percent: 20,
    },
  }),
  useSnapshots: () => ({ data: ctx.snapshots }),
  useNativeMetas: () => ({ data: undefined }),
  useProviderStates: () => ({ data: undefined, isFetching: false }),
  usePeakFlipTick: () => Date.UTC(2026, 8, 26),
}));

function provider(id: string, name: string, enabled = true): ProviderEntry {
  return {
    id,
    name,
    kind: { type: "native", provider: "deepseek" },
    enabled,
  } satisfies ProviderEntry;
}

// HoverPanel 渲染期读取 window.innerHeight（压缩布局判定）——Node 环境
// 无 window，桩出完整高度走非压缩分支（用量列表可见）
beforeEach(() => {
  vi.stubGlobal("window", { innerHeight: 520 });
  ctx.lang = "zh";
  ctx.mode = "single";
  ctx.trayIconEntryId = null;
  ctx.providers = [provider("p1", "账户")];
  ctx.snapshots = {};
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

describe("hero 措辞（PR #146 review 修复项 2：en 与 Rust i18n.rs 的 Left 成对）", () => {
  it("en 单窗口 hero label 为 Left（zh 剩余额度），不残留 Remaining", () => {
    ctx.lang = "en";
    const html = renderPanel([{ used: 42, unit: "%" }]);
    expect(html).toContain("Left");
    expect(html).not.toContain("Remaining");
    ctx.lang = "zh";
    expect(renderPanel([{ used: 42, unit: "%" }])).toContain("剩余额度");
  });

  it("en 多窗口 hero label 带剩余短标注：Left 5h（zh 剩余 5h）", () => {
    ctx.lang = "en";
    const html = renderPanel([
      { used: 42, unit: "%", plan_name: "GLM Coding Plan（5h）" },
      { used: 80, unit: "%", plan_name: "GLM Coding Plan（week）" },
    ]);
    expect(html).toContain("Left 5h");
    expect(html).not.toContain("Remaining");
    ctx.lang = "zh";
    expect(renderPanel([
      { used: 42, unit: "%", plan_name: "GLM Coding Plan（5h）" },
      { used: 80, unit: "%", plan_name: "GLM Coding Plan（week）" },
    ])).toContain("剩余 5h");
  });
});

describe("多条目列表（spec #151 / T-29）", () => {
  beforeEach(() => {
    ctx.mode = "multi";
    ctx.trayIconEntryId = "p2";
    ctx.providers = [
      provider("p1", "账户甲"),
      provider("p2", "账户乙"),
      provider("p3", "已禁用", false),
    ];
    ctx.snapshots = {
      p1: { data: [{ used: 42, unit: "%", plan_name: "5h" }], at: Date.UTC(2026, 8, 26, 12, 0, 0) },
      p2: {
        data: [
          { used: 84, unit: "%", plan_name: "GLM Coding Plan（5h）" },
          { used: 1, unit: "%", plan_name: "GLM Coding Plan（MCP）" },
        ],
        at: Date.UTC(2026, 8, 26, 12, 0, 0),
      },
    };
  });

  it("只渲染启用条目，禁用条目不出现", () => {
    const html = renderToStaticMarkup(<HoverPanel />);
    expect(html).toContain("账户甲");
    expect(html).toContain("账户乙");
    expect(html).not.toContain("已禁用");
    expect((html.match(/id="qt-hover-row-/g) ?? []).length).toBe(2);
  });

  it("显示目标无数据时仍保留头像环（只画轨道，标记身份不依赖数值）", () => {
    ctx.snapshots = {};
    const html = renderToStaticMarkup(<HoverPanel />);
    expect((html.match(/qt-hover-avatar-ring/g) ?? []).length).toBe(1);
    expect(html).not.toContain("qt-hover-avatar-value");
  });

  it("显示目标只有一个：托盘圆环数据源那一行带头像进度环", () => {
    const html = renderToStaticMarkup(<HoverPanel />);
    expect((html.match(/qt-hover-avatar-ring/g) ?? []).length).toBe(1);
    // 目标行（p2）同时带 is-target 修饰
    expect((html.match(/class="qt-hover-row is-target/g) ?? []).length).toBe(1);
  });

  it("多窗口条目逐窗口展示指标：剩余 5h 16% 与 剩余 MCP 99% 同屏", () => {
    const html = renderToStaticMarkup(<HoverPanel />);
    expect(html).toContain("剩余 5h");
    expect(html).toContain("16%");
    expect(html).toContain("剩余 MCP");
    expect(html).toContain("99%");
  });

  it("低余额窗口的数值带警示类（剩余 16% ≤ 阈值 20）", () => {
    const html = renderToStaticMarkup(<HoverPanel />);
    expect(html).toMatch(/<b class="is-alert">/);
  });

  it("多条目模式不渲染单账户的账户下拉选择器", () => {
    const html = renderToStaticMarkup(<HoverPanel />);
    expect(html).not.toContain("qt-hover-selectors");
  });
});

describe("单账户回退模式（spec #151 / T-31）", () => {
  it("单账户模式保留账户/模型选择器且不渲染列表", () => {
    const html = renderPanel([{ used: 42, unit: "%" }]);
    expect(html).toContain("qt-hover-selectors");
    expect(html).not.toContain("qt-hover-list");
  });

  it("金额型多窗口 hero 仍用「可用余额」，不接窗口后缀（与升级前逐字一致）", () => {
    const html = renderPanel([
      { remaining: 62.97, unit: "CNY", plan_name: "日额度" },
      { remaining: 120, unit: "CNY", plan_name: "月额度" },
    ]);
    expect(html).toContain("可用余额");
    expect(html).not.toContain("可用余额 日额度");
  });

  it("有数据但算不出主数值时 hero 回退「尚无数据」（升级前 card.noData 口径）", () => {
    const html = renderPanel([{ unit: "CNY", plan_name: "空窗口" }]);
    expect(html).toContain("尚无数据");
    expect(html).not.toContain("已获取");
  });
});
