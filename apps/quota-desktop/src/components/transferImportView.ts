// 导入模态（T-17）的策略/倒计时/armed 状态机纯逻辑：组件只负责渲染、
// 计时与提交接线；档位必填校验、覆盖三重防线的状态转移与 IPC 载荷构造
// 的契约集中在此便于单测。倒计时步进复用清空配置的纯函数
// （clearConfigView 的 stepCountdown），秒数档位保持一致。
import type { ImportOptions, TransferMode } from "../types";

/** 覆盖防线阅读倒计时秒数（与清空配置二级确认同档：阅读锁定 5 秒）。 */
export const IMPORT_OVERWRITE_COUNTDOWN_SECONDS = 5;

/** 模态内语义化策略；IPC 载荷（serde externally tagged）在
 *  buildImportOptions 转换为 Rust `ImportStrategy` 字面量。 */
export type ImportStrategyChoice = "merge" | "overwrite";

/** 校验失败的就地提示文案键（null = 可提交）：密码档容器必填口令，
 *  便捷档容器无输入恒通过。 */
export type ImportInputError = { key: "settings.importPasswordRequired" };

export function validateImportInput(
  containerMode: TransferMode,
  password: string,
): ImportInputError | null {
  if (containerMode === "Password" && password.length === 0) {
    return { key: "settings.importPasswordRequired" };
  }
  return null;
}

/** 提交前置状态（armed 状态机）：文件已选且档位输入合法是两模共同前提；
 *  合并模即达可提交；覆盖模叠加两道防线——倒计时归零前确认钮与风险
 *  勾选一并禁用，归零后勾选放开、确认钮仍待勾选完成。
 *  确认钮 danger 化仅覆盖模；ackEnabled 对合并模无意义（恒 false）。 */
export interface ImportSubmitState {
  /** 确认钮是否可点（不含 busy/inspecting 的瞬态锁）。 */
  canSubmit: boolean;
  /** 确认钮 danger 化：仅覆盖模。 */
  danger: boolean;
  /** 风险勾选框是否可用：覆盖模下倒计时归零后放开。 */
  ackEnabled: boolean;
  /** 覆盖倒计时期间确认钮文案携带的秒数；null = 非锁定态。 */
  countdownSeconds: number | null;
}

export function resolveImportSubmitState(input: {
  hasFile: boolean;
  passwordError: boolean;
  strategy: ImportStrategyChoice;
  acknowledged: boolean;
  countdownRemaining: number;
}): ImportSubmitState {
  const base = input.hasFile && !input.passwordError;
  if (input.strategy === "merge") {
    return { canSubmit: base, danger: false, ackEnabled: false, countdownSeconds: null };
  }
  const unlocked = input.countdownRemaining <= 0;
  return {
    canSubmit: base && unlocked && input.acknowledged,
    danger: true,
    ackEnabled: unlocked,
    countdownSeconds: unlocked ? null : input.countdownRemaining,
  };
}

/** 构造 IPC 载荷：口令仅密码档容器携带（便捷档容器忽略口令字段，
 *  恒置 null，避免口令随载荷多余传输；口令只经内存与 IPC，不落日志）。 */
export function buildImportOptions(
  containerMode: TransferMode,
  password: string,
  strategy: ImportStrategyChoice,
): ImportOptions {
  return {
    password: containerMode === "Password" ? password : null,
    strategy: strategy === "merge" ? "Merge" : "Overwrite",
  };
}
