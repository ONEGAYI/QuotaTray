import { describe, expect, it } from "vitest";
import {
  initialMainPanelState,
  reduceMainPanelTransition,
} from "./mainPanelView";

describe("主窗口面板切换", () => {
  it("在最大模糊点替换内容，再让目标面板上浮变清晰", () => {
    const blurring = reduceMainPanelTransition(initialMainPanelState, {
      type: "select",
      panel: "usage",
    });
    expect(blurring).toEqual({
      visible: "accounts",
      target: "usage",
      phase: "blurring",
    });

    const revealing = reduceMainPanelTransition(blurring, { type: "animation-end" });
    expect(revealing).toEqual({
      visible: "usage",
      target: "usage",
      phase: "revealing",
    });

    expect(reduceMainPanelTransition(revealing, { type: "animation-end" })).toEqual({
      visible: "usage",
      target: "usage",
      phase: "idle",
    });
  });

  it("模糊过程中连续点击时以最后选择为准", () => {
    const blurring = reduceMainPanelTransition(initialMainPanelState, {
      type: "select",
      panel: "usage",
    });
    const retargeted = reduceMainPanelTransition(blurring, {
      type: "select",
      panel: "accounts",
    });

    expect(reduceMainPanelTransition(retargeted, { type: "animation-end" })).toEqual({
      visible: "accounts",
      target: "accounts",
      phase: "revealing",
    });
  });

  it("重复选择当前面板时不启动动画", () => {
    expect(
      reduceMainPanelTransition(initialMainPanelState, {
        type: "select",
        panel: "accounts",
      }),
    ).toBe(initialMainPanelState);
  });
});
