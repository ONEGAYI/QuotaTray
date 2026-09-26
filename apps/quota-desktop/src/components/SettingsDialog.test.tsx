// #134 内容检查：目录设置区块的渲染输出——开关两态小字、既有
// revision/来源/最近检查/立即更新入口的可见性。目录区块抽为纯 props
// 子组件（自持 busy/结果反馈），renderToStaticMarkup 可直接渲染，
// 无需 jsdom；useLang/api 以 vi.mock 提供（ProviderCard.test 先例）。
// #130 迁移反馈：resolveImportFeedback 纯函数（计数口径 + 降级文案
// 组装）与 TransferFeedbackView（warning 条渲染）。
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { interpolate, type TextKey } from "../i18n";
import { zh } from "../i18n/zh";
import type { CatalogStatus, ImportCounts, TransferDegraded } from "../types";
import { CatalogSettingsSection, TransferFeedbackView, resolveImportFeedback } from "./SettingsDialog";

vi.mock("../i18n", () => ({
  // 插值版 t（#130 测试需要 reason/计数插值断言）
  interpolate: (template: string, params?: Record<string, string | number>) =>
    template.replace(/\{(\w+)\}/g, (match, key: string) =>
      params && key in params ? String(params[key]) : match,
    ),
  useLang: () => ({
    lang: "zh",
    t: (key: TextKey, params?: Record<string, string | number>) => {
      const template = zh[key];
      return params
        ? template.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m))
        : template;
    },
  }),
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

/** #130 测试用 t：插值语义与 i18n.interpolate 一致。 */
const t = (key: TextKey, params?: Record<string, string | number>) =>
  interpolate(zh[key], params);

const counts = (overrides: Partial<ImportCounts> = {}): ImportCounts => ({
  providers_added: 2,
  providers_skipped: 1,
  series_added: 3,
  series_skipped: 4,
  ...overrides,
});

describe("导入成功反馈组装（#130）", () => {
  it("无降级：合并模展示全部四计数", () => {
    const result = resolveImportFeedback(
      { counts: counts(), degraded: [] },
      "Merge",
      t,
    );
    expect(result.text).toContain("供应商新增 2 个");
    expect(result.text).toContain("跳过 1 个重复");
    expect(result.text).toContain("比较组合新增 3 条");
    expect(result.text).toContain("跳过 4 条");
    expect(result.degradedTexts).toEqual([]);
  });

  it("无降级：覆盖模展示供应商与组合计数", () => {
    const result = resolveImportFeedback(
      { counts: counts(), degraded: [] },
      "Overwrite",
      t,
    );
    expect(result.text).toContain("已导入 2 个供应商");
    expect(result.text).toContain("3 条比较组合");
  });

  it("比较组合写失败：合并模改用 providers-only 文案，未落盘计数不出现", () => {
    const degraded: TransferDegraded[] = [
      { kind: "usage_comparison", reason: "设置写入失败：disk full" },
    ];
    const result = resolveImportFeedback({ counts: counts(), degraded }, "Merge", t);
    expect(result.text).toContain("供应商新增 2 个");
    expect(result.text).toContain("跳过 1 个重复");
    // 组合计数（3/4）是「已生效」陈述，未落盘时不得出现（#130 计数口径）
    expect(result.text).not.toContain("比较组合");
    expect(result.degradedTexts).toEqual([
      interpolate(zh["settings.importDegradedUsageComparison"], {
        reason: "设置写入失败：disk full",
      }),
    ]);
  });

  it("比较组合写失败：覆盖模同样改用 providers-only 文案", () => {
    const degraded: TransferDegraded[] = [
      { kind: "usage_comparison", reason: "设置写入失败：disk full" },
    ];
    const result = resolveImportFeedback({ counts: counts(), degraded }, "Overwrite", t);
    expect(result.text).toContain("已导入 2 个供应商");
    expect(result.text).not.toContain("比较组合");
  });

  it("历史写失败：成功文案不动（计数真实），降级条说明原因与恢复手段", () => {
    const degraded: TransferDegraded[] = [
      { kind: "history", reason: "历史库读写失败：locked" },
    ];
    const result = resolveImportFeedback({ counts: counts(), degraded }, "Merge", t);
    expect(result.text).toContain("比较组合新增 3 条");
    expect(result.degradedTexts[0]).toContain("历史库读写失败：locked");
    expect(result.degradedTexts[0]).toContain("重新导入");
  });
});

describe("迁移反馈区渲染（#130）", () => {
  it("降级明细存在：成功文案下出现 warning 块，分条呈现", () => {
    const html = renderToStaticMarkup(
      <TransferFeedbackView
        feedback={{
          kind: "success",
          text: "合并完成：供应商新增 2 个、跳过 1 个重复。",
          degradedTexts: [
            "历史数据未写入本机（历史库读写失败：locked）；重新导入可恢复。",
            "比较组合未写入本机（设置写入失败：disk full）；重新导入可恢复。",
          ],
        }}
      />,
    );
    expect(html).toContain("qt-inline-warning");
    expect(html).toContain("qt-settings-success");
    expect(html).toContain("历史库读写失败：locked");
    expect(html).toContain("disk full");
  });

  it("无降级明细：不渲染 warning 块", () => {
    const html = renderToStaticMarkup(
      <TransferFeedbackView
        feedback={{ kind: "success", text: "配置已导出至：D:\\backup.qtray-export" }}
      />,
    );
    expect(html).not.toContain("qt-inline-warning");
    expect(html).toContain("qt-settings-success");
  });

  it("错误态：沿用 inline-error 样式，降级块不参与", () => {
    const html = renderToStaticMarkup(
      <TransferFeedbackView feedback={{ kind: "error", text: "导入失败：口令错误" }} />,
    );
    expect(html).toContain("qt-inline-error");
    expect(html).not.toContain("qt-settings-success");
    expect(html).not.toContain("qt-inline-warning");
  });
});
