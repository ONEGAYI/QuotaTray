// #134 内容检查：目录设置区块的渲染输出——开关两态小字、既有
// revision/来源/最近检查/立即更新入口的可见性。目录区块抽为纯 props
// 子组件（自持 busy/结果反馈），renderToStaticMarkup 可直接渲染，
// 无需 jsdom；useLang/api 以 vi.mock 提供（ProviderCard.test 先例）。
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { zh } from "../i18n/zh";
import type { CatalogStatus } from "../types";
import { CatalogSettingsSection } from "./SettingsDialog";

vi.mock("../i18n", () => ({
  useLang: () => ({ lang: "zh", t: (key: keyof typeof zh) => zh[key] }),
}));
vi.mock("../api", () => ({ api: { catalogUpdate: vi.fn() } }));

/** 相对当前时钟 2 小时前完成过一次成功检查的稳定样本。 */
function sampleStatus(overrides: Partial<CatalogStatus> = {}): CatalogStatus {
  return {
    revision: 42,
    origin: "cached",
    fallback_reason: null,
    last_attempt_ms: Date.now() - 2 * 3_600_000,
    last_success_ms: Date.now() - 2 * 3_600_000,
    last_error: null,
    ...overrides,
  };
}

function render(options: {
  autoUpdate: boolean;
  mobile?: boolean;
  /** 显式传 undefined = 目录状态未加载（不能用 ?? 回退，会被样本顶替）。 */
  catalogStatus?: CatalogStatus;
}): string {
  const catalogStatus = "catalogStatus" in options ? options.catalogStatus : sampleStatus();
  return renderToStaticMarkup(
    <CatalogSettingsSection
      mobile={options.mobile ?? false}
      autoUpdate={options.autoUpdate}
      onAutoUpdateChange={() => {}}
      catalogStatus={catalogStatus}
    />,
  );
}

describe("目录设置区块渲染（#134）", () => {
  it("开启态（桌面）：小字说明约每 6 小时检查，既有信息与入口齐备", () => {
    const html = render({ autoUpdate: true });
    // 新小字：跟随开关状态（开启 → 周期口径）
    expect(html).toContain(zh["settings.catalogScheduleOnDesktop"]);
    // 既有信息：开关行说明、revision、来源、最近检查
    expect(html).toContain(zh["settings.catalogAutoUpdateTitle"]);
    expect(html).toContain(zh["settings.catalogAutoUpdateHint"]);
    expect(html).toContain("revision 42");
    expect(html).toContain("已缓存");
    expect(html).toContain("上次检查 2 小时前");
    // 立即更新入口
    expect(html).toContain(zh["settings.catalogUpdateNow"]);
    // 开关为开启态
    expect(html).toMatch(/<input[^>]*type="checkbox"[^>]*checked=""/);
  });

  it("关闭态：小字改述可手动「立即更新」，不再出现开启态周期文案", () => {
    const html = render({ autoUpdate: false });
    expect(html).toContain(zh["settings.catalogScheduleOff"]);
    expect(html).toContain(zh["settings.catalogUpdateNow"]);
    expect(html).not.toContain(zh["settings.catalogScheduleOnDesktop"]);
    // 开关为关闭态（无 checked 属性）
    expect(html).toMatch(/<input[^>]*type="checkbox"(?![^>]*checked="")/);
  });

  it("开启态（Android）：小字用前台口径，不出现桌面「应用运行期间」措辞", () => {
    const html = render({ autoUpdate: true, mobile: true });
    expect(html).toContain(zh["settings.catalogScheduleOnMobile"]);
    expect(html).not.toContain(zh["settings.catalogScheduleOnDesktop"]);
  });

  it("最近检查失败：状态行呈现失败与 30 分钟重试口径（自动更新开启时）", () => {
    const html = render({
      autoUpdate: true,
      catalogStatus: sampleStatus({ last_error: "HTTP 500" }),
    });
    expect(html).toContain("失败，至少 30 分钟后自动重试");
  });

  it("目录状态未加载：不渲染「上次检查」，小字与立即更新入口仍在", () => {
    const html = render({ autoUpdate: true, catalogStatus: undefined });
    expect(html).not.toContain("上次检查");
    expect(html).toContain(zh["settings.catalogScheduleOnDesktop"]);
    expect(html).toContain(zh["settings.catalogUpdateNow"]);
  });
});
