import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyThemeTransition,
  expandRadius,
  originFromRect,
  shouldAnimate,
  themeOriginVars,
} from "./themeTransition";

describe("主题切换圆形扩散", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("expandRadius 取点击点到视口四角的最大距离（覆盖全屏的终态半径）", () => {
    // 视口正中：四角等距，勾股 400-300-500
    expect(expandRadius(400, 300, 800, 600)).toBeCloseTo(500);
    // 角落点击：最远角是对角，800×600 对角线 1000
    expect(expandRadius(0, 0, 800, 600)).toBeCloseTo(1000);
    expect(expandRadius(800, 600, 800, 600)).toBeCloseTo(1000);
    // 非对称点：最远角是右下 → hypot(700, 550)
    expect(expandRadius(100, 50, 800, 600)).toBeCloseTo(Math.hypot(700, 550));
  });

  it("themeOriginVars 输出扩散圆心与终态半径 CSS 变量（半径向上取整防边缘缝隙）", () => {
    expect(themeOriginVars({ x: 100, y: 50 }, 890)).toEqual({
      "--qt-theme-x": "100px",
      "--qt-theme-y": "50px",
      "--qt-theme-r": "890px",
    });
    expect(themeOriginVars({ x: 0, y: 0 }, 890.2)["--qt-theme-r"]).toBe("891px");
  });

  it("shouldAnimate 仅在实际主题变化时为真（dark→system 且系统同为暗色等无视觉变化场景跳过）", () => {
    expect(shouldAnimate("light", "dark")).toBe(true);
    expect(shouldAnimate("dark", "light")).toBe(true);
    expect(shouldAnimate("dark", "dark")).toBe(false);
    expect(shouldAnimate("light", "light")).toBe(false);
  });

  it("originFromRect 有效矩形取中心（按钮锚点），缺失/零尺寸回退视口中心", () => {
    expect(originFromRect({ left: 100, top: 40, width: 32, height: 32 }, { width: 800, height: 600 })).toEqual({
      x: 116,
      y: 56,
    });
    expect(originFromRect(null, { width: 800, height: 600 })).toEqual({ x: 400, y: 300 });
    expect(originFromRect({ left: 0, top: 0, width: 0, height: 0 }, { width: 800, height: 600 })).toEqual({
      x: 400,
      y: 300,
    });
  });

  it("旧帧捕获前保持原主题，进入 View Transition 更新回调后才切换 DOM 并提交状态", () => {
    let update: (() => void) | undefined;
    const toggle = vi.fn();
    const setProperty = vi.fn();
    const startViewTransition = vi.fn((callback: () => void) => {
      update = callback;
      return {};
    });
    vi.stubGlobal("document", {
      documentElement: { classList: { toggle }, style: { setProperty } },
      startViewTransition,
    });
    vi.stubGlobal("window", {
      innerWidth: 800,
      innerHeight: 600,
      matchMedia: vi.fn(() => ({ matches: false })),
    });
    const commit = vi.fn();

    applyThemeTransition("dark", { x: 100, y: 50 }, commit);

    expect(startViewTransition).toHaveBeenCalledOnce();
    expect(toggle).not.toHaveBeenCalled();
    expect(commit).not.toHaveBeenCalled();

    update?.();
    expect(toggle).toHaveBeenCalledWith("dark", true);
    expect(commit).toHaveBeenCalledWith("dark");
  });

  it.each(["unsupported", "reduced-motion"] as const)(
    "%s 时退化为即时切换并提交状态",
    (mode) => {
      const toggle = vi.fn();
      const startViewTransition = mode === "unsupported" ? undefined : vi.fn();
      vi.stubGlobal("document", {
        documentElement: { classList: { toggle }, style: { setProperty: vi.fn() } },
        startViewTransition,
      });
      vi.stubGlobal("window", {
        innerWidth: 800,
        innerHeight: 600,
        matchMedia: vi.fn(() => ({ matches: mode === "reduced-motion" })),
      });
      const commit = vi.fn();

      applyThemeTransition("light", { x: 100, y: 50 }, commit);

      if (startViewTransition) expect(startViewTransition).not.toHaveBeenCalled();
      expect(toggle).toHaveBeenCalledWith("dark", false);
      expect(commit).toHaveBeenCalledWith("light");
    },
  );
});
