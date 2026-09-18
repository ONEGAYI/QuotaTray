// 导出模态（T-16）的档位与口令校验纯逻辑：组件只负责渲染与提交接线，
// 档位语义、校验规则与 IPC 载荷构造的契约集中在此便于单测。
import type { ExportOptions } from "../types";

/** 密码档口令最小长度（与 core `ExportOptions` 按 char 计数的硬校验一致）。 */
export const EXPORT_PASSWORD_MIN_LENGTH = 8;

/** 模态内语义化档位；IPC 载荷（serde externally tagged）在 buildExportOptions 转换。 */
export type ExportMode = "password" | "convenient";

/** 校验失败的就地提示文案键（含插值参数；null = 可提交）。 */
export type ExportInputError =
  | { key: "settings.exportPasswordTooShort"; min: number }
  | { key: "settings.exportPasswordMismatch" };

/** 校验档位输入：便捷档无输入恒通过；密码档要求口令 ≥8 字符且两次一致。
 *  口令长度优先于一致性报告（先解决最基础的输入量问题）。 */
export function validateExportInput(
  mode: ExportMode,
  password: string,
  confirm: string,
): ExportInputError | null {
  if (mode !== "password") return null;
  // 按 code point 计数，与 core 的 char 计数口径对齐（CJK/emoji 不被低估）
  const length = Array.from(password).length;
  if (length < EXPORT_PASSWORD_MIN_LENGTH) {
    return { key: "settings.exportPasswordTooShort", min: EXPORT_PASSWORD_MIN_LENGTH };
  }
  if (password !== confirm) {
    return { key: "settings.exportPasswordMismatch" };
  }
  return null;
}

/** 构造 IPC 载荷：Rust `ExportOptions`（serde externally tagged）的镜像形状。 */
export function buildExportOptions(mode: ExportMode, password: string): ExportOptions {
  return mode === "convenient" ? "Convenient" : { Password: { password } };
}
