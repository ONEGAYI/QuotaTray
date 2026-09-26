// T-22 消息中心低余额卡片渲染契约：取值与措辞统一剩余口径——
// 占位名 remaining 与模板 {remaining} 必须匹配（interpolate 对未提供的
// 占位原样保留，错配会把 "{remaining}" 字面量漏到界面上，此测试位
// 就为拦截该错型）。mock i18n 的 t 内联与真实 interpolate 同语义的
// 占位替换（vi.mock 工厂 hoisting 不能引用顶层导入，见 ProviderCard 先例）。
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { zh } from "../i18n/zh";
import type { CenterMessage } from "./messageCenterView";
import { MessageCenter } from "./MessageCenter";

vi.mock("../i18n", () => ({
  useLang: () => ({
    lang: "zh",
    t: (key: keyof typeof zh, params?: Record<string, string | number>) =>
      zh[key].replace(/\{(\w+)\}/g, (match, name: string) =>
        params && name in params ? String(params[name]) : match,
      ),
  }),
}));
vi.mock("../api", () => ({ api: { installUpdate: vi.fn() } }));
// 下拉壳默认闭合（open=false 返回 null），静态渲染掀不开——仅把
// DropdownMenu 替换为透传容器，卡片本体（MessageCenter 的 t 占位插值）
// 仍走真实渲染路径。
vi.mock("./ui", async () => {
  const actual = await vi.importActual<typeof import("./ui")>("./ui");
  return {
    ...actual,
    DropdownMenu: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  };
});

function render(messages: CenterMessage[]): string {
  const client = new QueryClient();
  const html = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <MessageCenter messages={messages} seen={new Set()} onSeenAll={() => {}} />
    </QueryClientProvider>,
  );
  client.clear();
  return html;
}

describe("低余额卡片文案（剩余口径，T-22）", () => {
  it("正文为「名称 剩余 N%」，占位完全插值无残留", () => {
    const html = render([
      { kind: "low-balance", providerId: "kimi", name: "Kimi", remainingPercent: 15 },
    ]);
    expect(html).toContain("Kimi 剩余 15%");
    // 占位错配（如模板 {remaining} 对传参 percent）会漏出字面量
    expect(html).not.toContain("{remaining}");
    expect(html).not.toContain("{percent}");
    expect(html).not.toContain("已用");
  });

  it("恢复卡片维持既有剩余措辞（本就剩余口径，不回归）", () => {
    const html = render([
      { kind: "balance-recovered", providerId: "kimi", name: "Kimi", remainingPercent: 96 },
    ]);
    expect(html).toContain(zh["msgCenter.balanceRecoveredTitle"]);
    expect(html).toContain("96%");
  });
});
