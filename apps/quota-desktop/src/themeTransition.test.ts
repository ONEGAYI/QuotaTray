import { describe, expect, it, vi } from "vitest";
import {
  applySystemThemeTransition,
  expandRadius,
  originFromRect,
  shouldAnimate,
  themeOriginVars,
} from "./themeTransition";

describe("主题切换圆形扩散", () => {
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

  it("系统主题状态只在 View Transition 更新回调内提交，旧帧捕获前保持原主题", () => {
    let update: (() => void) | undefined;
    const runTransition = vi.fn(
      (_next: "light" | "dark", _origin: { x: number; y: number }, onApply: () => void) => {
        update = onApply;
      },
    );
    const setResolved = vi.fn();

    applySystemThemeTransition("dark", { x: 100, y: 50 }, setResolved, runTransition);

    expect(runTransition).toHaveBeenCalledOnce();
    expect(setResolved).not.toHaveBeenCalled();

    update?.();
    expect(setResolved).toHaveBeenCalledWith("dark");
  });
});
