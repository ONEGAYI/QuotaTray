import { Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useLang } from "../i18n";
import {
  CLEAR_CONFIG_COUNTDOWN_SECONDS,
  resolveConfirmButton,
  stepCountdown,
} from "./clearConfigView";
import { Button, DialogShell } from "./ui";

// 清空配置的二级确认弹窗：确认按钮倒数 5 秒内锁定（禁用 + 变暗），
// 归零后才可点击。Esc/取消随时可退出，重新打开会重置倒数。确认执行
// 由调用方注入（onConfirm 返回 Promise）；busy 期间双按钮锁定防重入，
// 失败就地展示错误并恢复按钮（与便携首启确认同一交互语言）。
export function ClearConfigDialog({
  open,
  onClose,
  onConfirm,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const { t } = useLang();
  const [remaining, setRemaining] = useState(CLEAR_CONFIG_COUNTDOWN_SECONDS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      setRemaining(CLEAR_CONFIG_COUNTDOWN_SECONDS);
      setBusy(false);
      setError(null);
      return;
    }
    if (remaining <= 0) return;
    const timer = setTimeout(() => setRemaining(stepCountdown(remaining)), 1000);
    return () => clearTimeout(timer);
  }, [open, remaining]);

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await onConfirm();
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  if (!open) return null;
  const button = resolveConfirmButton(remaining);

  return (
    <DialogShell
      title={t("settings.clearConfirmHeading")}
      description={t("settings.clearConfirmDescription")}
      // busy 期间锁定 Esc/右上角 X：清空不可中断，关闭弹窗会让后台
      // 结果不可见（失败错误落在已卸载组件上）
      onClose={busy ? () => {} : onClose}
      closeLabel={t("common.cancel")}
      size="sm"
      footer={
        <>
          <Button disabled={busy} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="danger"
            icon={Trash2}
            disabled={button.locked || busy}
            onClick={() => void confirm()}
          >
            {busy
              ? t("settings.clearBusy")
              : button.seconds == null
                ? t(button.labelKey)
                : t(button.labelKey, { seconds: button.seconds })}
          </Button>
        </>
      }
    >
      <p className="qt-confirm-message qt-confirm-danger">
        {t("settings.clearConfirmWarning")}
      </p>
      {error && <p className="qt-inline-error">{error}</p>}
    </DialogShell>
  );
}
