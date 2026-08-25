import { describe, expect, it } from "vitest";
import { proximityGlowStrength } from "./mainPanelTabsView";

describe("主窗口页签鼠标聚光", () => {
  it("鼠标位于文字中心时完整显现柔光", () => {
    expect(
      proximityGlowStrength(
        { x: 80, y: 32 },
        { x: 80, y: 32 },
        120,
      ),
    ).toBe(1);
  });

  it("鼠标远离文字中心时平滑降低显现强度", () => {
    expect(
      proximityGlowStrength(
        { x: 20, y: 32 },
        { x: 80, y: 32 },
        120,
      ),
    ).toBeCloseTo(0.5, 5);
  });

  it("鼠标超出有效半径时完全隐藏柔光", () => {
    expect(
      proximityGlowStrength(
        { x: -40, y: 32 },
        { x: 80, y: 32 },
        120,
      ),
    ).toBe(0);
  });

  it("拒绝无效的显现半径", () => {
    expect(() =>
      proximityGlowStrength(
        { x: 0, y: 0 },
        { x: 0, y: 0 },
        0,
      ),
    ).toThrow("glow radius must be positive");
  });
});
