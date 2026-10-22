// @vitest-environment jsdom
// 悬停面板交互回归（jsdom）：列表点击开抽屉、遮罩点击收起、Esc 两级、
// on/off 环写显示目标、批量刷新汇总、单条刷新失败可见、焦点归还。
// 渲染契约（布局/文案）留在 HoverPanel.test.tsx 的 SSR 用例里，这里只管
// 需要真实事件与焦点的行为。
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderEntry, QueryOutcome, SnapshotEntry } from "../types";
import HoverPanel from "./HoverPanel";

const ctx = vi.hoisted(() => ({
  lang: "zh" as "zh" | "en",
  mode: "multi" as "multi" | "single",
  providers: [] as ProviderEntry[],
  snapshots: {} as Record<string, SnapshotEntry>,
  trayIconEntryId: null as string | null,
  /** 平台查询的返回值：按 id 定制失败/成功（模拟 QueryOutcome.ok=false）。 */
  queryOutcome: null as null | ((id: string) => Promise<QueryOutcome>),
}));

const apiMock = vi.hoisted(() => ({
  hideHoverPanel: vi.fn(async () => {}),
  setHoverPanelPointerInside: vi.fn(async () => {}),
  openMainWindow: vi.fn(async () => {}),
  queryProvider: vi.fn(async (id: string): Promise<QueryOutcome> => {
    if (ctx.queryOutcome) return ctx.queryOutcome(id);
    return { ok: true, data: [], error: null, at: Date.now() };
  }),
  patchSettings: vi.fn(async () => {}),
  upsertProvider: vi.fn(async () => {}),
}));

vi.mock("../api", () => ({ api: apiMock }));
vi.mock("../i18n", () => ({
  LangProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
  useLang: () => ({
    lang: ctx.lang,
    t: (key: string, vars?: Record<string, unknown>) => {
      const table = {
        "card.refresh": "刷新",
        "card.noData": "尚无数据",
        "card.noReason": "未返回原因",
        "hover.detail": "详情",
        "hover.back": "返回",
        "hover.close": "关闭面板",
        "hover.accounts": "账户列表",
        "hover.model": "模型",
        "hover.targetOn": "当前显示在托盘",
        "hover.targetOff": "设为托盘显示目标",
        "hover.openMain": "打开主窗口",
        "hover.availableBalance": "可用余额",
        "hover.remainingQuota": "剩余额度",
      } as Record<string, string>;
      const text = table[key] ?? key;
      return vars ? Object.entries(vars).reduce((acc, [k, v]) => acc.replace(`{{${k}}}`, String(v)), text) : text;
    },
  }),
}));
vi.mock("../theme", () => ({
  ThemeProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
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
  usePeakFlipTick: () => Date.UTC(2026, 9, 22),
}));

function provider(id: string, name: string, enabled = true): ProviderEntry {
  return {
    id,
    name,
    enabled,
    kind: { type: "native", provider: "deepseek" },
  } satisfies ProviderEntry;
}

const at = Date.UTC(2026, 9, 22, 12, 0, 0);

/** 两账户：甲为显示目标，乙非目标；甲乙都有可渲染的百分比窗口。 */
function seedMulti() {
  ctx.mode = "multi";
  ctx.trayIconEntryId = "a";
  ctx.providers = [provider("a", "账户甲"), provider("b", "账户乙")];
  ctx.snapshots = {
    a: { data: [{ used: 25, unit: "%", plan_name: "5h" }], at },
    b: { data: [{ used: 60, unit: "%", plan_name: "5h" }], at },
  };
}

beforeEach(() => {
  seedMulti();
  ctx.lang = "zh";
  ctx.queryOutcome = null;
  apiMock.hideHoverPanel.mockClear();
  apiMock.patchSettings.mockClear();
  apiMock.queryProvider.mockClear();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

/** 行按钮：按账户名定位（列表与抽屉都可能出现同名文本，只取列表内的按钮）。 */
function rowButton(name: string): HTMLButtonElement {
  const list = document.getElementById("qt-hover-list");
  const button = Array.from(list?.querySelectorAll<HTMLButtonElement>(".qt-hover-row") ?? []).find(
    (item) => item.textContent?.includes(name),
  );
  if (!button) throw new Error(`未找到行按钮：${name}`);
  return button;
}

function drawer(): HTMLElement {
  const node = document.getElementById("qt-hover-drawer");
  if (!node) throw new Error("抽屉节点不存在");
  return node;
}

describe("列表 → 抽屉（T-29 / T-30）", () => {
  it("点击行打开抽屉：当前行 aria-expanded、列表移出无障碍树、返回键获得焦点", async () => {
    render(<HoverPanel />);

    expect(drawer().classList.contains("is-open")).toBe(false);
    fireEvent.click(rowButton("账户乙"));

    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));
    expect(rowButton("账户乙").getAttribute("aria-expanded")).toBe("true");
    expect(rowButton("账户甲").getAttribute("aria-expanded")).toBe("false");
    expect(document.getElementById("qt-hover-list")?.getAttribute("aria-hidden")).toBe("true");
    // 抽屉内容指向点击的账户，而不是显示目标
    expect(drawer().textContent).toContain("账户乙");
    expect(document.activeElement).toBe(screen.getByLabelText("返回"));
  });

  it("抽屉打开时点击遮罩收起，并把焦点还给触发它的行", async () => {
    render(<HoverPanel />);
    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    const scrim = document.querySelector<HTMLElement>(".qt-hover-drawer-scrim");
    expect(scrim).not.toBeNull();
    fireEvent.click(scrim!);

    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(false));
    await waitFor(() => expect(document.activeElement).toBe(rowButton("账户乙")), { timeout: 1000 });
  });

  it("Esc 分两级：先收抽屉，再按一次隐藏面板", async () => {
    render(<HoverPanel />);
    fireEvent.click(rowButton("账户甲"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(false));
    expect(apiMock.hideHoverPanel).not.toHaveBeenCalled();

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(apiMock.hideHoverPanel).toHaveBeenCalledTimes(1));
  });

  it("退场动画期间遮罩仍在：关闭瞬间的点击不会穿透到下方行再开抽屉", async () => {
    render(<HoverPanel />);
    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    fireEvent.click(document.querySelector<HTMLElement>(".qt-hover-drawer-scrim")!);
    // 关闭后立即渲染：抽屉已不是 open，但遮罩仍应存在（拦截点击）
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(false));
    expect(document.querySelector(".qt-hover-drawer-scrim")).not.toBeNull();
    expect(document.getElementById("qt-hover-list")?.getAttribute("aria-hidden")).toBe("true");

    fireEvent.click(document.querySelector<HTMLElement>(".qt-hover-drawer-scrim")!);
    expect(drawer().classList.contains("is-open")).toBe(false);
  });

  it("抽屉关闭后列表恢复可读与可聚焦", async () => {
    render(<HoverPanel />);
    fireEvent.click(rowButton("账户甲"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));
    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => expect(document.getElementById("qt-hover-list")?.getAttribute("aria-hidden")).toBeNull());
    await waitFor(() => expect(rowButton("账户甲").tabIndex).toBe(0));
  });

  it("抽屉展开期间条目消失：焦点回落到列表，不留在 body（P3 回归）", async () => {
    const { rerender } = render(<HoverPanel />);
    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    // 乙被禁用：行与抽屉一起消失，焦点原本在抽屉的返回键上
    ctx.providers = [provider("a", "账户甲"), provider("b", "账户乙", false)];
    rerender(<HoverPanel />);

    await waitFor(() => expect(rowButton("账户甲")).not.toBeNull());
    await waitFor(() =>
      expect(document.activeElement).toBe(document.getElementById("qt-hover-list")),
    );
    expect(document.activeElement).not.toBe(document.body);
  });
});

describe("显示目标切换（on/off 环）", () => {
  it("off 环点击后写入 tray_icon_entry_id；on 环点击不写（单选语义不可取消）", async () => {
    render(<HoverPanel />);
    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    const ring = screen.getByLabelText("设为托盘显示目标");
    fireEvent.click(ring);
    await waitFor(() => expect(apiMock.patchSettings).toHaveBeenCalledTimes(1));
    expect(apiMock.patchSettings).toHaveBeenCalledWith({ tray_icon_entry_id: "b" });

    // 抽屉里当前账户（乙）此刻仍是 off：目标仍是甲。切到甲后其环为 on，
    // 再点不应再发请求。这里直接验证 on 态文案与禁用行为。
    fireEvent.click(screen.getByLabelText("返回"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(false));
    fireEvent.click(rowButton("账户甲"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    apiMock.patchSettings.mockClear();
    const onRing = screen.getByLabelText("当前显示在托盘");
    fireEvent.click(onRing);
    expect(apiMock.patchSettings).not.toHaveBeenCalled();
  });
});

describe("刷新（T-31）", () => {
  it("头部全量刷新串行调用全部启用条目，失败汇总带账户名", async () => {
    ctx.queryOutcome = async (id) =>
      id === "b"
        ? { ok: false, data: [], error: { kind: "deterministic", message: "key 无效" }, at: null }
        : { ok: true, data: [], error: null, at: Date.now() };
    render(<HoverPanel />);

    fireEvent.click(screen.getByLabelText("刷新"));

    await waitFor(() => expect(apiMock.queryProvider).toHaveBeenCalledTimes(2));
    expect(apiMock.queryProvider.mock.calls.map(([id]) => id)).toEqual(["a", "b"]);
    const banner = await screen.findByRole("alert");
    expect(banner.textContent).toContain("账户乙");
    expect(banner.textContent).toContain("key 无效");
  });

  it("抽屉内单条刷新平台失败时，错误展示在抽屉里", async () => {
    ctx.queryOutcome = async () => ({ ok: false, data: [], error: { kind: "deterministic", message: "平台拒绝" }, at: null });
    render(<HoverPanel />);
    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    // 抽屉头部与页面头部各有一个「刷新」按钮：取抽屉内的那个
    fireEvent.click(within(drawer()).getByLabelText("刷新"));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("平台拒绝");
    expect(alert.closest("#qt-hover-drawer")).not.toBeNull();
  });

  it("甲的刷新命令迟到 reject，不会串进乙的详情（#159-001）", async () => {
    // 甲的请求挂起，稍后 reject；期间切换到乙
    let rejectA: ((error: Error) => void) | null = null;
    ctx.queryOutcome = (id) => {
      if (id === "a") {
        return new Promise<QueryOutcome>((_resolve, reject) => {
          rejectA = reject;
        });
      }
      return Promise.resolve({ ok: true, data: [], error: null, at: Date.now() });
    };
    render(<HoverPanel />);

    fireEvent.click(rowButton("账户甲"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));
    fireEvent.click(within(drawer()).getByLabelText("刷新"));
    await waitFor(() => expect(rejectA).not.toBeNull());

    // 切到乙抽屉，此时甲的 Promise 才 reject
    fireEvent.click(screen.getByLabelText("返回"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(false));
    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    rejectA!(new Error("A-only transport failure"));
    await waitFor(() => expect(screen.getAllByRole("alert").length).toBeGreaterThan(0));

    // 乙的详情里不能出现甲的错误；错误应挂在列表横幅并带甲的身份
    expect(within(drawer()).queryByRole("alert")).toBeNull();
    const banner = document.querySelector(".qt-hover-error-banner");
    expect(banner?.textContent).toContain("A-only transport failure");
    expect(banner?.textContent).toContain("账户甲");
  });

  it("甲乙同时失败时，乙详情只含乙自己的错误（#159-002）", async () => {
    ctx.queryOutcome = async (id) => ({
      ok: false,
      data: [],
      error: {
        kind: "deterministic",
        message: id === "a" ? "A-only failure" : "B-only failure",
      },
      at: null,
    });
    render(<HoverPanel />);

    fireEvent.click(screen.getByLabelText("刷新"));
    // 列表横幅聚合两条
    await waitFor(() => {
      const banner = document.querySelector(".qt-hover-error-banner");
      expect(banner?.textContent).toContain("A-only failure");
      expect(banner?.textContent).toContain("B-only failure");
    });

    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));
    const alert = await waitFor(() => within(drawer()).getByRole("alert"));
    expect(alert.textContent).toContain("B-only failure");
    // 甲的错误不能因为"同批有乙失败"就透传进来
    expect(alert.textContent).not.toContain("A-only failure");
  });

  it("抽屉关闭后单条刷新失败不会无声消失：改挂列表横幅（P3 回归）", async () => {
    ctx.queryOutcome = async () => ({ ok: false, data: [], error: { kind: "deterministic", message: "平台拒绝" }, at: null });
    render(<HoverPanel />);
    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    fireEvent.click(within(drawer()).getByLabelText("刷新"));
    // 请求在途时收起抽屉：失败回调到达时抽屉已关闭
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(false));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("平台拒绝");
    // 横幅在列表视图，而不是留在已关闭的抽屉里
    expect(alert.classList.contains("qt-hover-error-banner")).toBe(true);
  });

  it("抽屉只展示与当前账户相关的批次错误（P3 回归）", async () => {
    // 全量刷新中只有甲失败；随后打开乙的抽屉，不应看到甲的错误
    ctx.queryOutcome = async (id) =>
      id === "a"
        ? { ok: false, data: [], error: { kind: "deterministic", message: "甲的平台错误" }, at: null }
        : { ok: true, data: [], error: null, at: Date.now() };
    render(<HoverPanel />);

    fireEvent.click(screen.getByLabelText("刷新"));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("甲的平台错误"));

    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));
    // 无关账户的错误不塞进乙的详情；横幅让位给抽屉（收起后回来）
    expect(within(drawer()).queryByRole("alert")).toBeNull();

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(false));
    // 错误没有丢：抽屉收起后横幅重新可见
    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain("甲的平台错误"),
    );
  });

  it("抽屉展示当前账户自己的批次错误（与当前账户相关时进入详情）", async () => {
    ctx.queryOutcome = async (id) =>
      id === "b"
        ? { ok: false, data: [], error: { kind: "deterministic", message: "乙的平台错误" }, at: null }
        : { ok: true, data: [], error: null, at: Date.now() };
    render(<HoverPanel />);

    fireEvent.click(screen.getByLabelText("刷新"));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("乙的平台错误"));

    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));
    const alert = await waitFor(() => within(drawer()).getByRole("alert"));
    expect(alert.textContent).toContain("乙的平台错误");
  });
});

describe("单账户模式不受多条目交互影响（T-31）", () => {
  it("single 模式不渲染列表与抽屉，头部刷新只查显示目标", async () => {
    ctx.mode = "single";
    render(<HoverPanel />);

    expect(document.getElementById("qt-hover-list")).toBeNull();
    expect(document.querySelector(".qt-hover-drawer")).toBeNull();

    fireEvent.click(screen.getByLabelText("刷新"));
    await waitFor(() => expect(apiMock.queryProvider).toHaveBeenCalledWith("a"));
    expect(apiMock.queryProvider).toHaveBeenCalledTimes(1);
  });

  it("模式切到 single 时收起抽屉，切回 multi 不自动重开（残留 openId 回归）", async () => {
    const { rerender } = render(<HoverPanel />);
    fireEvent.click(rowButton("账户乙"));
    await waitFor(() => expect(drawer().classList.contains("is-open")).toBe(true));

    ctx.mode = "single";
    rerender(<HoverPanel />);
    await waitFor(() => expect(document.getElementById("qt-hover-list")).toBeNull());

    ctx.mode = "multi";
    rerender(<HoverPanel />);
    await waitFor(() => expect(document.getElementById("qt-hover-list")).not.toBeNull());
    // 切回后仍是列表，不应自动弹出上次查看的账户
    expect(drawer().classList.contains("is-open")).toBe(false);
  });
});

describe("时间派生随钟表推进（#159-003）", () => {
  const resetAt = Date.UTC(2026, 9, 22, 12, 2, 0);

  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.setSystemTime(resetAt - 120_000);
    ctx.mode = "multi";
    ctx.trayIconEntryId = "a";
    ctx.providers = [provider("a", "账户甲"), provider("b", "账户乙")];
    ctx.snapshots = {
      a: { data: [{ used: 42, unit: "%", reset_at: resetAt }], at: resetAt - 120_000 },
      b: { data: [{ used: 60, unit: "%", reset_at: resetAt }], at: resetAt - 120_000 },
    };
  });

  it("数据引用不变时，时钟前进一分钟后倒计时从 2m 变 1m", async () => {
    render(<HoverPanel />);
    const list = document.getElementById("qt-hover-list")!;
    await waitFor(() => expect(list.textContent).toContain("2m"));

    // 只推进时钟，数据对象引用保持不变（模拟 React Query 结构共享）
    await act(async () => {
      vi.advanceTimersByTime(60_000);
    });

    await waitFor(() => expect(list.textContent).toContain("1m"));
  });
});
