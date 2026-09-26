// T-23 编辑对话框主度量偏好三段控件契约（spec #137 / issue #141）：
// 控件位于凭据与查询配置之后的展示相关区，三段（自动/百分比/金额），
// 值随条目 primary_metric 载入（缺省 auto）——aria-pressed 锁定选中段。
// native 分支静态渲染（CodeMirror 只在 template/script 分支挂载，
// 顶层 import 于 Node 环境安全）；useLang/api/queries/theme 按先例 mock。
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { zh } from "../i18n/zh";
import type { NativeMeta, ProviderEntry } from "../types";
import { EditDialog } from "./EditDialog";

vi.mock("../i18n", () => ({
  useLang: () => ({
    lang: "zh",
    t: (key: keyof typeof zh) => zh[key],
  }),
}));
vi.mock("../api", () => ({
  api: {
    validateTemplate: vi.fn(),
    validateScript: vi.fn(),
    upsertProvider: vi.fn(),
  },
  newEntryId: () => "new-id",
}));
vi.mock("../queries", () => ({
  useNativeMetas: () => ({ data: [meta] }),
  invalidateProviderCaches: vi.fn(),
}));
vi.mock("../theme", () => ({ useTheme: () => "light" }));
// DialogShell 经 createPortal 挂 document.body（Node 环境无 document），
// 替换为透传容器；SegmentedControl 走真实实现（aria-pressed 是被测契约）。
vi.mock("./ui", async () => {
  const actual = await vi.importActual<typeof import("./ui")>("./ui");
  return {
    ...actual,
    DialogShell: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  };
});

const meta: NativeMeta = {
  id: "deepseek",
  name: "DeepSeek",
  pricing: null,
  pricing_by_currency: {},
  custom_models: [],
  supports_plan_variant: false,
  uses_cli_credentials: false,
  uses_api_key2: false,
  console_url: null,
};

function renderDialog(initial: ProviderEntry | null): string {
  const client = new QueryClient();
  const html = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <EditDialog open initial={initial} onClose={() => {}} />
    </QueryClientProvider>,
  );
  client.clear();
  return html;
}

/** 提取某段的 aria-pressed 值（SegmentedControl 无 name，按按钮文案定位）。 */
function pressedOf(html: string, label: string): string | undefined {
  const match = html.match(new RegExp(`aria-pressed="(true|false)">(${label})<`));
  return match?.[1];
}

describe("主度量偏好三段控件（T-23）", () => {
  const base: ProviderEntry = {
    id: "p1",
    name: "账户",
    kind: { type: "native", provider: "deepseek" },
    enabled: true,
  };

  it("三段齐备（自动/百分比/金额），缺省载入 auto 段", () => {
    const html = renderDialog({ ...base });
    expect(html).toContain(zh["edit.primaryMetric"]);
    expect(html).toContain(zh["edit.primaryMetricAuto"]);
    expect(html).toContain(zh["edit.primaryMetricPercent"]);
    expect(html).toContain(zh["edit.primaryMetricAmount"]);
    expect(pressedOf(html, zh["edit.primaryMetricAuto"])).toBe("true");
    expect(pressedOf(html, zh["edit.primaryMetricPercent"])).toBe("false");
  });

  it("值随条目 primary_metric 载入：percent 偏好选中百分比段", () => {
    const html = renderDialog({ ...base, primary_metric: "percent" });
    expect(pressedOf(html, zh["edit.primaryMetricPercent"])).toBe("true");
    expect(pressedOf(html, zh["edit.primaryMetricAuto"])).toBe("false");
  });

  it("新增条目（initial=null）同样缺省 auto", () => {
    const html = renderDialog(null);
    expect(pressedOf(html, zh["edit.primaryMetricAuto"])).toBe("true");
  });
});
