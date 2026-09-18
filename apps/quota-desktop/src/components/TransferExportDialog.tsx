import { AlertTriangle, FileDown, Info } from "lucide-react";
import { useEffect, useState } from "react";
import { useLang } from "../i18n";
import type { ExportOptions } from "../types";
import { transferErrorMessage } from "./configTransferView";
import {
  buildExportOptions,
  validateExportInput,
  type ExportMode,
} from "./transferExportView";
import { Button, DialogShell, SegmentedControl } from "./ui";

// 导出模态（T-16，替代原系统 confirm 流程）：档位分段（密码档默认推荐）
// → 条件展开（密码档 info + 双输入校验；便捷档保留等同明文凭据警示卡）
// → 前端校验通过后交调用方弹系统保存框并执行（提交后才弹 picker）。
// onExport 返回 "saved" = 执行已开始（成功反馈落在数据页 transferFeedback）；
// "cancelled" = 用户在保存框取消，留在模态可调整档位重试；reject = 执行
// 失败，就地展示错误。Esc/焦点圈定/移动端全屏由 DialogShell 既有机制提供。
export function TransferExportDialog({
  open,
  onClose,
  onExport,
}: {
  open: boolean;
  onClose: () => void;
  onExport: (options: ExportOptions) => Promise<"saved" | "cancelled">;
}) {
  const { t } = useLang();
  const [mode, setMode] = useState<ExportMode>("password");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) return;
    // 关闭即重置：口令不留前端状态，重开回到密码档默认
    setMode("password");
    setPassword("");
    setConfirm("");
    setBusy(false);
    setError(null);
  }, [open]);

  const submit = async () => {
    const problem = validateExportInput(mode, password, confirm);
    if (problem) {
      setError(
        problem.key === "settings.exportPasswordTooShort"
          ? t(problem.key, { min: problem.min })
          : t(problem.key),
      );
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await onExport(buildExportOptions(mode, password));
      if (result === "saved") {
        onClose();
      } else {
        setBusy(false);
      }
    } catch (e) {
      setError(transferErrorMessage(e));
      setBusy(false);
    }
  };

  if (!open) return null;

  return (
    <DialogShell
      title={t("settings.exportModalTitle")}
      description={t("settings.exportModalDescription")}
      // busy 期间锁定 Esc/右上角 X：保存框与写盘进行中，关闭会让结果不可见
      onClose={busy ? () => {} : onClose}
      closeLabel={t("common.cancel")}
      size="sm"
      footer={
        <>
          <Button disabled={busy} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            icon={FileDown}
            disabled={busy}
            onClick={() => void submit()}
          >
            {busy ? t("settings.exporting") : t("settings.exportModalConfirm")}
          </Button>
        </>
      }
    >
      <div className="qt-transfer-export-body">
        <SegmentedControl
          value={mode}
          options={[
            { value: "password", label: t("settings.exportModePassword") },
            { value: "convenient", label: t("settings.exportModeConvenient") },
          ]}
          onChange={(next) => {
            setMode(next);
            // 档位切换清残留错误：错误文案只描述当前档的输入
            setError(null);
          }}
        />
        {mode === "password" ? (
          <>
            <p className="qt-transfer-export-info">
              <Info size={14} aria-hidden="true" />
              <span>{t("settings.exportPasswordInfo")}</span>
            </p>
            <label className="qt-field">
              <span>{t("settings.exportPasswordLabel")}</span>
              <input
                className="qt-input"
                type="password"
                autoComplete="new-password"
                disabled={busy}
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            </label>
            <label className="qt-field">
              <span>{t("settings.exportPasswordConfirmLabel")}</span>
              <input
                className="qt-input"
                type="password"
                autoComplete="new-password"
                disabled={busy}
                value={confirm}
                onChange={(event) => setConfirm(event.target.value)}
              />
            </label>
          </>
        ) : (
          <div className="qt-transfer-intro">
            <span className="qt-transfer-intro-icon">
              <AlertTriangle size={18} aria-hidden="true" />
            </span>
            <div>
              <p>{t("settings.exportConvenientWarning")}</p>
            </div>
          </div>
        )}
        {error && <p className="qt-inline-error">{error}</p>}
      </div>
    </DialogShell>
  );
}
