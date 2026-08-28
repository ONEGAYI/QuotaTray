// 清空配置二次确认的倒计时纯逻辑：确认按钮在倒数归零前保持锁定，
// 组件侧只负责计时与渲染，状态转移契约集中在此便于单测。

export const CLEAR_CONFIG_COUNTDOWN_SECONDS = 5;

export function stepCountdown(remaining: number): number {
  return Math.max(0, remaining - 1);
}

export type ClearConfirmButton =
  | { locked: true; labelKey: "settings.clearConfirmCountdown"; seconds: number }
  | { locked: false; labelKey: "settings.clearConfirmButton"; seconds: null };

export function resolveConfirmButton(remaining: number): ClearConfirmButton {
  if (remaining > 0) {
    return { locked: true, labelKey: "settings.clearConfirmCountdown", seconds: remaining };
  }
  return { locked: false, labelKey: "settings.clearConfirmButton", seconds: null };
}
