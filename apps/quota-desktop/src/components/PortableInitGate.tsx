// 便携首启门控：portable.key 缺失时接管整个窗口，取得显式确认后才由
// 后端建钥（AGENTS.md 红线 §5）。正文只放「为什么 + 不要做什么」两行，
// 完整固定安全提示收进问号悬停展开（InlineMd 渲染，字典值保持文档原文）；
// 倒计时锁定与清空配置确认共用同一交互语言（clearConfigView 纯函数）。
import { CircleHelp, KeyRound, ShieldAlert } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import {
  CLEAR_CONFIG_COUNTDOWN_SECONDS,
  resolveConfirmButton,
  stepCountdown,
} from "./clearConfigView";
import { Button, InlineMd } from "./ui";

/** 便携门控正文块：右上角问号，点击展开/收起完整说明，Esc 也可收起。
 * 只用点击不用 hover 展开：面板占布局使卡片变高、居中布局令卡片上移，
 * hover 展开会把图标移出鼠标热区而反复收起展开（闪烁）。 */
function InfoDisclosure({
  label,
  content,
  children,
}: {
  label: string;
  content: ReactNode;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="qt-portable-gate-notice">
      <button
        type="button"
        className="qt-gate-info-btn"
        aria-label={label}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={(e) => {
          if (e.key === "Escape") setOpen(false);
        }}
      >
        <CircleHelp size={16} aria-hidden="true" />
      </button>
      {children}
      {open && (
        <div className="qt-gate-info-panel" role="note">
          {content}
        </div>
      )}
    </div>
  );
}

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
        <InfoDisclosure
          label={t("portable.infoLabel")}
          content={<InlineMd text={t("portable.noticeFull")} />}
        >
          <p>
            <InlineMd text={t("portable.noticeWhy")} />
          </p>
          <p>
            <InlineMd text={t("portable.noticeRule")} />
          </p>
        </InfoDisclosure>
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
