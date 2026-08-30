import { Check, X, type LucideIcon } from "lucide-react";
import { useEffect, useRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { parseInlineMd } from "./inlineMd";
import { shouldCloseDialogOnPop } from "../runtimeView";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";

export function Button({
  variant = "secondary",
  icon: Icon,
  children,
  className = "",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  icon?: LucideIcon;
}) {
  return (
    <button className={`qt-btn qt-btn-${variant} ${className}`} {...props}>
      {Icon && <Icon size={16} aria-hidden="true" />}
      {children}
    </button>
  );
}

export function IconButton({
  label,
  icon: Icon,
  danger = false,
  className = "",
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  label: string;
  icon?: LucideIcon;
  danger?: boolean;
}) {
  return (
    <button
      aria-label={label}
      data-tooltip={label}
      className={`qt-icon-btn ${danger ? "qt-icon-btn-danger" : ""} ${className}`}
      {...props}
    >
      {Icon ? <Icon size={16} aria-hidden="true" /> : children}
    </button>
  );
}

export function Badge({
  tone = "neutral",
  dot = false,
  children,
}: {
  tone?: "neutral" | "accent" | "success" | "warning" | "danger";
  dot?: boolean;
  children: ReactNode;
}) {
  return (
    <span className={`qt-badge qt-badge-${tone}`}>
      {dot && <span className="qt-badge-dot" />}
      {children}
    </span>
  );
}

export function SegmentedControl<T extends string>({
  value,
  options,
  onChange,
  compact = false,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (value: T) => void;
  compact?: boolean;
}) {
  return (
    <div className={`qt-segmented ${compact ? "qt-segmented-compact" : ""}`}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label className="qt-switch">
      <span className="sr-only">{label}</span>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="qt-switch-track" />
    </label>
  );
}

export function Tooltip({
  text,
  multiline = false,
  children,
}: {
  text: string;
  /** 长文本（错误详情、多句解释）：气泡允许换行并放宽限宽，见样式规范 T-001 */
  multiline?: boolean;
  children: ReactNode;
}) {
  return (
    <span
      className={`qt-tooltip-anchor ${multiline ? "is-multiline" : ""}`}
      data-tooltip={text}
      aria-label={text}
    >
      {children}
    </span>
  );
}

/** 把含 `**粗体**` 与 `` `代码` `` 标记的字典文案渲染为富文本（解析见 inlineMd.ts）。 */
export function InlineMd({ text }: { text: string }) {
  return (
    <>
      {parseInlineMd(text).map((token, i) =>
        token.kind === "strong" ? (
          <strong key={i}>{token.text}</strong>
        ) : token.kind === "code" ? (
          <code key={i}>{token.text}</code>
        ) : (
          <span key={i}>{token.text}</span>
        ),
      )}
    </>
  );
}

export function DropdownMenu({
  open,
  onClose,
  children,
  className = "",
}: {
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) onClose();
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div ref={ref} className={`qt-dropdown ${className}`}>
      {children}
    </div>
  );
}

export function MenuItem({
  checked,
  icon: Icon,
  children,
  onClick,
}: {
  checked?: boolean;
  icon?: LucideIcon;
  children: ReactNode;
  onClick: () => void;
}) {
  return (
    <button type="button" className="qt-menu-item" onClick={onClick}>
      <span className="qt-menu-item-main">
        {Icon && <Icon size={15} aria-hidden="true" />}
        {children}
      </span>
      <span className="qt-menu-check">{checked && <Check size={14} aria-hidden="true" />}</span>
    </button>
  );
}

export function DialogShell({
  title,
  description,
  onClose,
  children,
  footer,
  size = "md",
  closeLabel = "Close",
  className = "",
  backdropClassName = "",
  closeOnBackdrop = false,
}: {
  title: string;
  description?: string;
  onClose: () => void;
  children: ReactNode;
  footer: ReactNode;
  size?: "sm" | "md" | "lg";
  closeLabel?: string;
  className?: string;
  backdropClassName?: string;
  closeOnBackdrop?: boolean;
}) {
  const dialogRef = useRef<HTMLElement>(null);
  const onCloseRef = useRef(onClose);
  const historyIdRef = useRef(`qt-dialog-${crypto.randomUUID()}`);
  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);
  useEffect(() => {
    if (!document.body.classList.contains("qt-mobile-runtime")) return;
    const dialogId = historyIdRef.current;
    const previous =
      typeof window.history.state === "object" && window.history.state != null
        ? window.history.state
        : {};
    window.history.pushState({ ...previous, qtDialogId: dialogId }, "");
    const onPopState = (event: PopStateEvent) => {
      const nextDialogId =
        typeof event.state?.qtDialogId === "string" ? event.state.qtDialogId : null;
      if (shouldCloseDialogOnPop(dialogId, nextDialogId)) onCloseRef.current();
    };
    window.addEventListener("popstate", onPopState);
    return () => {
      window.removeEventListener("popstate", onPopState);
      if (window.history.state?.qtDialogId === dialogId) window.history.back();
    };
  }, []);
  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;
    const previouslyFocused = document.activeElement as HTMLElement | null;
    const focusableSelector = [
      "button:not([disabled])",
      "input:not([disabled])",
      "select:not([disabled])",
      "textarea:not([disabled])",
      "a[href]",
      "[tabindex]:not([tabindex='-1'])",
    ].join(",");
    const focusable = () =>
      Array.from(dialog.querySelectorAll<HTMLElement>(focusableSelector)).filter(
        (element) => !element.hidden && element.getAttribute("aria-hidden") !== "true",
      );
    (focusable()[0] ?? dialog).focus();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;
      const items = focusable();
      if (items.length === 0) {
        event.preventDefault();
        dialog.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    // 监听挂在弹窗根元素而非 document：焦点已被圈在弹窗内，事件必然
    // 冒泡经过此处；嵌套二级弹窗持有焦点时，下层弹窗不会收到 Esc/Tab
    dialog.addEventListener("keydown", onKeyDown);
    return () => {
      dialog.removeEventListener("keydown", onKeyDown);
      previouslyFocused?.focus();
    };
  }, []);

  return createPortal(
    <div
      className={`qt-dialog-backdrop ${backdropClassName}`}
      onClick={(event) => {
        if (closeOnBackdrop && event.target === event.currentTarget) onClose();
      }}
    >
      <section
        ref={dialogRef}
        className={`qt-dialog qt-dialog-${size} ${className}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
      >
        <header className="qt-dialog-header">
          <div>
            <h2>{title}</h2>
            {description && <p>{description}</p>}
          </div>
          <IconButton icon={X} label={closeLabel} onClick={onClose} />
        </header>
        <div className="qt-dialog-body">{children}</div>
        <footer className="qt-dialog-footer">{footer}</footer>
      </section>
    </div>,
    document.body,
  );
}

export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  cancelLabel,
  pending,
  onConfirm,
  onClose,
}: {
  open: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel: string;
  pending?: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  if (!open) return null;
  return (
    <DialogShell
      title={title}
      onClose={onClose}
      size="sm"
      closeLabel={cancelLabel}
      footer={
        <>
          <Button onClick={onClose}>{cancelLabel}</Button>
          <Button variant="danger" disabled={pending} onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      <p className="qt-confirm-message">{message}</p>
    </DialogShell>
  );
}

export function SettingRow({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="qt-setting-row">
      <div>
        <h3>{title}</h3>
        <p>{description}</p>
      </div>
      <div className="qt-setting-control">{children}</div>
    </div>
  );
}
