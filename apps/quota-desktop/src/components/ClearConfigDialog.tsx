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
// 归零后才可点击。Esc/取消随时可退出，重新打开会重置倒数。
export function ClearConfigDialog({
  open,
  onClose,
  onConfirm,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
}) {
  const { t } = useLang();
  const [remaining, setRemaining] = useState(CLEAR_CONFIG_COUNTDOWN_SECONDS);

  useEffect(() => {
    if (!open) {
      setRemaining(CLEAR_CONFIG_COUNTDOWN_SECONDS);
      return;
    }
    if (remaining <= 0) return;
    const timer = setTimeout(() => setRemaining(stepCountdown(remaining)), 1000);
    return () => clearTimeout(timer);
  }, [open, remaining]);

  if (!open) return null;
  const button = resolveConfirmButton(remaining);

  return (
    <DialogShell
      title={t("settings.clearConfirmHeading")}
      description={t("settings.clearConfirmDescription")}
      onClose={onClose}
      closeLabel={t("common.cancel")}
      size="sm"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button
            variant="danger"
            icon={Trash2}
            disabled={button.locked}
            onClick={onConfirm}
          >
            {button.seconds == null
              ? t(button.labelKey)
              : t(button.labelKey, { seconds: button.seconds })}
          </Button>
        </>
      }
    >
      <p className="qt-confirm-message qt-confirm-danger">
        {t("settings.clearConfirmWarning")}
      </p>
    </DialogShell>
  );
}
