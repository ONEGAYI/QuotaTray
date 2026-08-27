import { describe, expect, it } from "vitest";
import {
  CLEAR_CONFIG_COUNTDOWN_SECONDS,
  resolveConfirmButton,
  stepCountdown,
} from "./clearConfigView";

describe("clear config confirm countdown", () => {
  it("locks the confirm button while the countdown is positive", () => {
    expect(resolveConfirmButton(CLEAR_CONFIG_COUNTDOWN_SECONDS)).toEqual({
      locked: true,
      labelKey: "settings.clearConfirmCountdown",
      seconds: CLEAR_CONFIG_COUNTDOWN_SECONDS,
    });
    expect(resolveConfirmButton(1).locked).toBe(true);
  });

  it("unlocks once the countdown reaches zero", () => {
    expect(resolveConfirmButton(0)).toEqual({
      locked: false,
      labelKey: "settings.clearConfirmButton",
      seconds: null,
    });
    // 防御：异常负值不锁死按钮
    expect(resolveConfirmButton(-1).locked).toBe(false);
  });

  it("steps down each second and clamps at zero", () => {
    expect(stepCountdown(CLEAR_CONFIG_COUNTDOWN_SECONDS)).toBe(4);
    expect(stepCountdown(1)).toBe(0);
    expect(stepCountdown(0)).toBe(0);
  });
});
