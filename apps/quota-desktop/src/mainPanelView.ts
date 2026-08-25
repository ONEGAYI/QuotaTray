export type MainPanel = "accounts" | "usage";
export type MainPanelPhase = "idle" | "blurring" | "revealing";

export interface MainPanelState {
  /** 当前实际渲染的面板；仅在模糊退场结束时替换。 */
  visible: MainPanel;
  /** 用户最后一次选择的面板。 */
  target: MainPanel;
  phase: MainPanelPhase;
}

export type MainPanelAction =
  | { type: "select"; panel: MainPanel }
  | { type: "animation-end" };

export const initialMainPanelState: MainPanelState = {
  visible: "accounts",
  target: "accounts",
  phase: "idle",
};

/**
 * 面板内容只在 blurring 动画结束（即最大模糊点）时替换。
 * 退场中再次选择仅更新目标，保证最终呈现用户最后一次点击的面板。
 */
export function reduceMainPanelTransition(
  state: MainPanelState,
  action: MainPanelAction,
): MainPanelState {
  if (action.type === "select") {
    if (action.panel === state.target) return state;

    if (state.phase === "idle") {
      return { ...state, target: action.panel, phase: "blurring" };
    }

    if (state.phase === "revealing" && action.panel !== state.visible) {
      return { ...state, target: action.panel, phase: "blurring" };
    }

    return { ...state, target: action.panel };
  }

  if (state.phase === "blurring") {
    return { visible: state.target, target: state.target, phase: "revealing" };
  }
  if (state.phase === "revealing") {
    return { ...state, phase: "idle" };
  }
  return state;
}
