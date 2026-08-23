import { describe, expect, it } from "vitest";
import { resolveUpdateError, resolveUpdateStatus } from "./settingsView";

describe("更新设置视图", () => {
  it("最新手动操作错误优先于后端历史错误", () => {
    expect(
      resolveUpdateError({
        checkError: new Error("本次检测失败"),
        downloadError: null,
        backendError: "旧错误",
        hasAvailable: false,
      }),
    ).toContain("本次检测失败");
  });

  it("已有可用版本时不展示历史检测错误", () => {
    expect(
      resolveUpdateError({
        checkError: null,
        downloadError: null,
        backendError: "旧错误",
        hasAvailable: true,
      }),
    ).toBeNull();
  });

  it("检测失败不会误判为已是最新", () => {
    expect(resolveUpdateStatus({ checking: false, hasAvailable: false, error: "失败" }))
      .toBe("error");
  });
});
