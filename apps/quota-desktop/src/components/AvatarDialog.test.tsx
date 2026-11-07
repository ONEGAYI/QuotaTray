// 头像弹窗关闭态守卫契约：DialogShell 自身不看 open——它的 backdrop 是
// `fixed inset:0 / z-index:50 / blur`，一旦在关闭态渲染出来，整个应用首屏
// 会被一块全屏模糊遮罩盖住（而且里面是个空弹窗）。守卫由调用方负责，
// EditDialog 有 `if (!open) return null`，本组件必须同口径。
//
// 静态渲染：DialogShell 经 createPortal 挂 document.body（Node 环境无
// document），替换为带标记的透传容器；被验证的是「关掉了就一个节点都不出」。
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { zh } from "../i18n/zh";
import { AvatarDialog } from "./AvatarDialog";

vi.mock("../i18n", () => ({
  useLang: () => ({ lang: "zh", t: (key: keyof typeof zh) => zh[key] }),
}));
vi.mock("../api", () => ({
  api: { setProviderAvatar: vi.fn(), prepareProviderAvatar: vi.fn() },
}));
vi.mock("../queries", () => ({ invalidateProviderCaches: vi.fn() }));
vi.mock("./ui", async () => {
  const actual = await vi.importActual<typeof import("./ui")>("./ui");
  return {
    ...actual,
    DialogShell: ({ children }: { children: ReactNode }) => (
      <div data-testid="dialog-shell">{children}</div>
    ),
  };
});

function render(open: boolean) {
  const qc = new QueryClient();
  return renderToStaticMarkup(
    <QueryClientProvider client={qc}>
      <AvatarDialog
        open={open}
        entryId="p1"
        entryName="测试条目"
        nativeId="deepseek"
        currentAvatar={undefined}
        onClose={() => {}}
      />
    </QueryClientProvider>,
  );
}

describe("AvatarDialog 关闭态", () => {
  it("open=false 时一个节点都不渲染", () => {
    expect(render(false)).toBe("");
  });

  it("open=true 时渲染弹窗内容", () => {
    const html = render(true);
    expect(html).toContain('data-testid="dialog-shell"');
    expect(html).toContain("qt-avatar-grid");
  });

  it("关闭态不泄漏条目名与内置图标网格", () => {
    const html = render(false);
    expect(html).not.toContain("测试条目");
    expect(html).not.toContain("qt-avatar-grid");
  });
});
