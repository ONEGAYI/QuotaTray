import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import type { MainPanel } from "../mainPanelView";
import { proximityGlowStrength } from "./mainPanelTabsView";

interface Props {
  selected: MainPanel;
  accountsLabel: string;
  usageLabel: string;
  ariaLabel: string;
  onSelect: (panel: MainPanel) => void;
}

const GLOW_REVEAL_RADIUS = 104;

export function MainPanelTabs({
  selected,
  accountsLabel,
  usageLabel,
  ariaLabel,
  onSelect,
}: Props) {
  const accountsRef = useRef<HTMLButtonElement>(null);
  const usageRef = useRef<HTMLButtonElement>(null);

  const setGlowStrength = (tab: HTMLButtonElement | null, strength: number) => {
    if (!tab) return;
    tab.style.setProperty("--qt-tab-glow-strength", strength.toFixed(3));
    tab.style.setProperty("--qt-tab-glow-brightness", (1 + strength * 0.24).toFixed(3));
    tab.style.setProperty("--qt-tab-glow-shadow-alpha", (strength * 0.34).toFixed(3));
  };

  const revealGlows = (event: PointerEvent<HTMLDivElement>) => {
    const pointer = { x: event.clientX, y: event.clientY };
    for (const tab of [accountsRef.current, usageRef.current]) {
      if (!tab) continue;
      const bounds = tab.getBoundingClientRect();
      const strength = proximityGlowStrength(
        pointer,
        { x: bounds.left + bounds.width / 2, y: bounds.top + bounds.height / 2 },
        GLOW_REVEAL_RADIUS,
      );
      setGlowStrength(tab, strength);
    }
  };

  const hideGlows = () => {
    setGlowStrength(accountsRef.current, 0);
    setGlowStrength(usageRef.current, 0);
  };

  const handleArrowKey = (event: KeyboardEvent<HTMLButtonElement>, panel: MainPanel) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next = panel === "accounts" ? usageRef.current : accountsRef.current;
    next?.focus();
  };

  return (
    <div
      className="qt-page-tabs"
      role="tablist"
      aria-label={ariaLabel}
      onPointerEnter={revealGlows}
      onPointerMove={revealGlows}
      onPointerLeave={hideGlows}
    >
      <button
        ref={accountsRef}
        id="qt-tab-accounts"
        className={`qt-page-tab ${selected === "accounts" ? "is-active" : ""}`}
        type="button"
        role="tab"
        aria-selected={selected === "accounts"}
        aria-controls="qt-main-panel"
        onKeyDown={(event) => handleArrowKey(event, "accounts")}
        onClick={() => onSelect("accounts")}
      >
        {accountsLabel}
      </button>
      <button
        ref={usageRef}
        id="qt-tab-usage"
        className={`qt-page-tab ${selected === "usage" ? "is-active" : ""}`}
        type="button"
        role="tab"
        aria-selected={selected === "usage"}
        aria-controls="qt-main-panel"
        onKeyDown={(event) => handleArrowKey(event, "usage")}
        onClick={() => onSelect("usage")}
      >
        {usageLabel}
      </button>
    </div>
  );
}
