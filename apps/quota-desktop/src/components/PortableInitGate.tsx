// 便携首启门控：portable.key 缺失时接管整个窗口，先原样展示固定
// 安全提示（AGENTS.md 红线 §5）并取得显式确认，确认后才由后端建钥。
// 倒计时锁定与清空配置确认共用同一交互语言（clearConfigView 纯函数）。
import { KeyRound, ShieldAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import {
  CLEAR_CONFIG_COUNTDOWN_SECONDS,
  resolveConfirmButton,
  stepCountdown,
} from "./clearConfigView";
import { Button } from "./ui";

export function PortableInitGate({ onDone }: { onDone: () => void }) {
  const { t } = useLang();
  const [remaining, setRemaining] = useState(CLEAR_CONFIG_COUNTDOWN_SECONDS);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (remaining <= 0) return;
    const timer = setTimeout(() => setRemaining(stepCountdown(remaining)), 1000);
    return () => window.clearTimeout(timer);
  }, [remaining]);

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.confirmPortableInit();
      onDone();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const cancel = async () => {
    setBusy(true);
    setError(null);
    try {
      // 后端清理 WebView2 缓存后退出应用；invoke 层失败时恢复按钮
      // 并展示错误（否则双按钮永久锁死，用户只能杀进程）
      await api.cancelPortableInit();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const button = resolveConfirmButton(remaining);

  return (
    <div className="qt-portable-gate">
      <section className="qt-portable-gate-card" role="alertdialog" aria-modal="true">
        <header>
          <span className="qt-portable-gate-icon">
            <ShieldAlert size={26} aria-hidden="true" />
          </span>
          <h1>{t("portable.initTitle")}</h1>
        </header>
        <p className="qt-portable-gate-notice">{t("portable.notice")}</p>
        <p className="qt-portable-gate-hint">
          <KeyRound size={14} aria-hidden="true" />
          {t("portable.confirmHint")}
        </p>
        {error && <p className="qt-inline-error">{error}</p>}
        <footer>
          <Button disabled={busy} onClick={() => void cancel()}>
            {t("portable.cancel")}
          </Button>
          <Button
            variant="danger"
            disabled={button.locked || busy}
            onClick={() => void confirm()}
          >
            {button.locked
              ? t("portable.confirmCountdown", { seconds: String(button.seconds) })
              : t("portable.confirm")}
          </Button>
        </footer>
      </section>
    </div>
  );
}
