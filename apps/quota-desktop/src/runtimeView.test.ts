import { describe, expect, it } from "vitest";
import { reduceDisclosure, runtimeUiPolicy, shouldCloseDialogOnPop } from "./runtimeView";

describe("runtimeUiPolicy", () => {
  it("Android 使用触摸优先壳层并裁掉桌面专属能力", () => {
    expect(runtimeUiPolicy("android")).toEqual({
      mobile: true,
      hover: false,
      titleBar: false,
      tray: false,
      autostart: false,
      selfUpdate: false,
      cliAssist: false,
      fullScreenDialogs: true,
      bottomNavigation: true,
      consoleLink: false,
    });
  });

  it("桌面保留既有壳层和悬停能力", () => {
    expect(runtimeUiPolicy("desktop")).toEqual({
      mobile: false,
      hover: true,
      titleBar: true,
      tray: true,
      autostart: true,
      selfUpdate: true,
      cliAssist: true,
      fullScreenDialogs: false,
      bottomNavigation: false,
      consoleLink: true,
    });
  });
});

describe("reduceDisclosure", () => {
  it("点击切换展开，选择与返回均关闭", () => {
    expect(reduceDisclosure(false, "toggle")).toBe(true);
    expect(reduceDisclosure(true, "toggle")).toBe(false);
    expect(reduceDisclosure(true, "select")).toBe(false);
    expect(reduceDisclosure(true, "dismiss")).toBe(false);
  });
});

describe("shouldCloseDialogOnPop", () => {
  it("Android 返回键只关闭离开的最上层页面，不误关重新露出的下层页面", () => {
    expect(shouldCloseDialogOnPop("dialog-a", "dialog-a")).toBe(false);
    expect(shouldCloseDialogOnPop("dialog-b", "dialog-a")).toBe(true);
    expect(shouldCloseDialogOnPop("dialog-a", null)).toBe(true);
  });
});
