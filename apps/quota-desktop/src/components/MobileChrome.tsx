import { BarChart3, Plus, Settings, WalletCards } from "lucide-react";
import type { MainPanel } from "../mainPanelView";
import { BrandMark } from "./BrandMark";
import { IconButton } from "./ui";

export function MobileTopBar({
  addLabel,
  settingsLabel,
  onAdd,
  onSettings,
}: {
  addLabel: string;
  settingsLabel: string;
  onAdd: () => void;
  onSettings: () => void;
}) {
  return (
    <header className="qt-mobile-topbar">
      <div className="qt-mobile-brand">
        <BrandMark />
        <strong>QuotaTray</strong>
      </div>
      <div className="qt-mobile-topbar-actions">
        <IconButton icon={Settings} label={settingsLabel} onClick={onSettings} />
        <IconButton icon={Plus} label={addLabel} onClick={onAdd} className="is-primary" />
      </div>
    </header>
  );
}

export function MobileBottomNavigation({
  selected,
  accountsLabel,
  usageLabel,
  ariaLabel,
  onSelect,
}: {
  selected: MainPanel;
  accountsLabel: string;
  usageLabel: string;
  ariaLabel: string;
  onSelect: (panel: MainPanel) => void;
}) {
  return (
    <nav className="qt-mobile-bottom-nav" aria-label={ariaLabel} role="tablist">
      <button
        id="qt-tab-accounts"
        type="button"
        role="tab"
        aria-selected={selected === "accounts"}
        className={selected === "accounts" ? "is-active" : ""}
        onClick={() => onSelect("accounts")}
      >
        <WalletCards size={21} aria-hidden="true" />
        <span>{accountsLabel}</span>
      </button>
      <button
        id="qt-tab-usage"
        type="button"
        role="tab"
        aria-selected={selected === "usage"}
        className={selected === "usage" ? "is-active" : ""}
        onClick={() => onSelect("usage")}
      >
        <BarChart3 size={21} aria-hidden="true" />
        <span>{usageLabel}</span>
      </button>
    </nav>
  );
}
