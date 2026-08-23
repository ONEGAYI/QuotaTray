import { Check, ChevronDown, ChevronRight } from "lucide-react";
import {
  useEffect,
  useId,
  useCallback,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { createPortal } from "react-dom";
import type { NativeMeta } from "../types";
import { groupNativeProviders, type NativeProviderGroup } from "./nativeProviderGroups";
import { providerIconUrl } from "./providerIcon";

interface Props {
  metas: NativeMeta[];
  value: string;
  ariaLabel: string;
  placeholder: string;
  groupLabels: Readonly<Record<string, string>>;
  onChange: (providerId: string) => void;
}

interface MenuPosition {
  top: number;
  left: number;
  width: number;
  maxHeight: number;
}

const POPOVER_WIDTH = 568;
const VIEWPORT_GAP = 10;

export function NativeProviderPicker({
  metas,
  value,
  ariaLabel,
  placeholder,
  groupLabels,
  onChange,
}: Props) {
  const menuId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const groupRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const optionRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const [open, setOpen] = useState(false);
  const [activeGroupIndex, setActiveGroupIndex] = useState<number | null>(null);
  const [menuPosition, setMenuPosition] = useState<MenuPosition | null>(null);
  const groups = useMemo(() => groupNativeProviders(metas), [metas]);
  const selected = metas.find((meta) => meta.id === value) ?? null;
  const reservedMenuHeight = open && menuPosition
    ? Math.min(groups.length * 41 + 12, menuPosition.maxHeight) + 6
    : 0;

  const measureMenu = useCallback(() => {
    const trigger = triggerRef.current;
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const width = Math.min(POPOVER_WIDTH, window.innerWidth - VIEWPORT_GAP * 2);
    const left = Math.min(
      Math.max(VIEWPORT_GAP, rect.left),
      Math.max(VIEWPORT_GAP, window.innerWidth - width - VIEWPORT_GAP),
    );
    // 聚合菜单固定从选择框下方展开；空间不足时由双栏各自滚动，
    // 不再向上/向左翻转遮住当前表单。
    const top = rect.bottom + 6;
    setMenuPosition({
      top,
      left,
      width,
      maxHeight: Math.max(120, window.innerHeight - top - VIEWPORT_GAP),
    });
  }, []);

  const closeMenu = useCallback((restoreFocus = false) => {
    setOpen(false);
    setActiveGroupIndex(null);
    if (restoreFocus) requestAnimationFrame(() => triggerRef.current?.focus());
  }, []);

  const openMenu = (focusGroup: boolean) => {
    if (groups.length === 0) return;
    measureMenu();
    setOpen(true);
    if (focusGroup) {
      const selectedIndex = groups.findIndex((group) =>
        group.providers.some((provider) => provider.id === value),
      );
      const index = selectedIndex >= 0 ? selectedIndex : 0;
      setActiveGroupIndex(index);
      requestAnimationFrame(() => groupRefs.current[index]?.focus());
    } else {
      const selectedIndex = groups.findIndex((group) =>
        group.providers.some((provider) => provider.id === value),
      );
      setActiveGroupIndex(selectedIndex >= 0 ? selectedIndex : 0);
    }
  };

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (!rootRef.current?.contains(target) && !menuRef.current?.contains(target)) closeMenu();
    };
    const onViewportChange = () => measureMenu();
    document.addEventListener("mousedown", onPointerDown);
    window.addEventListener("resize", onViewportChange);
    window.addEventListener("scroll", onViewportChange, true);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("resize", onViewportChange);
      window.removeEventListener("scroll", onViewportChange, true);
    };
  }, [open, measureMenu, closeMenu]);

  const focusGroup = (index: number) => {
    const next = (index + groups.length) % groups.length;
    setActiveGroupIndex(next);
    groupRefs.current[next]?.focus();
  };

  const focusOption = (group: NativeProviderGroup, index: number) => {
    const next = (index + group.providers.length) % group.providers.length;
    optionRefs.current[group.providers[next].id]?.focus();
  };

  const onTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) return;
    event.preventDefault();
    if (!open) openMenu(true);
  };

  const onGroupKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      focusGroup(index + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      focusGroup(index - 1);
    } else if (["ArrowRight", "Enter", " "].includes(event.key)) {
      event.preventDefault();
      setActiveGroupIndex(index);
      requestAnimationFrame(() => focusOption(groups[index], 0));
    } else if (event.key === "Escape") {
      event.preventDefault();
      closeMenu(true);
    }
  };

  const onOptionKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    group: NativeProviderGroup,
    groupIndex: number,
    optionIndex: number,
  ) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      focusOption(group, optionIndex + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      focusOption(group, optionIndex - 1);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      groupRefs.current[groupIndex]?.focus();
    } else if (event.key === "Escape") {
      event.preventDefault();
      closeMenu(true);
    }
  };

  const choose = (providerId: string) => {
    onChange(providerId);
    closeMenu(true);
  };

  const groupLabel = (group: NativeProviderGroup) => {
    return groupLabels[group.key] ?? group.label;
  };

  const renderIcon = (providerId: string, className: string) => {
    const iconUrl = providerIconUrl(providerId);
    return iconUrl
      ? <span className={className}><img src={iconUrl} alt="" aria-hidden="true" /></span>
      : <span className={`${className} is-fallback`} aria-hidden="true">{providerId.slice(0, 1).toUpperCase()}</span>;
  };

  const menu = open && menuPosition && createPortal(
    <div
      ref={menuRef}
      id={menuId}
      role="menu"
      aria-label={ariaLabel}
      className="qt-native-picker-menu"
      style={{
        top: menuPosition.top,
        left: menuPosition.left,
        width: menuPosition.width,
        maxHeight: menuPosition.maxHeight,
      } as CSSProperties}
    >
      <div className="qt-native-picker-groups">
        {groups.map((group, groupIndex) => {
          const active = activeGroupIndex === groupIndex;
          return (
            <div
              key={group.key}
              className="qt-native-picker-group"
              onMouseEnter={() => setActiveGroupIndex(groupIndex)}
            >
              <button
                ref={(node) => { groupRefs.current[groupIndex] = node; }}
                type="button"
                role="menuitem"
                aria-haspopup="menu"
                aria-expanded={active}
                className={`qt-native-picker-group-button ${active ? "is-active" : ""}`}
                onClick={() => setActiveGroupIndex(groupIndex)}
                onFocus={() => setActiveGroupIndex(groupIndex)}
                onKeyDown={(event) => onGroupKeyDown(event, groupIndex)}
              >
                <span className="qt-native-picker-group-main">
                  {renderIcon(group.iconProviderId, "qt-native-picker-group-icon")}
                  <span>{groupLabel(group)}</span>
                </span>
                <ChevronRight size={15} aria-hidden="true" />
              </button>
            </div>
          );
        })}
      </div>

      {activeGroupIndex != null && groups[activeGroupIndex] && (
        <div
          role="menu"
          aria-label={groupLabel(groups[activeGroupIndex])}
          className="qt-native-picker-submenu"
        >
          {groups[activeGroupIndex].providers.map((provider, optionIndex) => (
            <button
              key={provider.id}
              ref={(node) => { optionRefs.current[provider.id] = node; }}
              type="button"
              role="menuitemradio"
              aria-checked={provider.id === value}
              className="qt-native-picker-option"
              onClick={() => choose(provider.id)}
              onKeyDown={(event) => onOptionKeyDown(
                event,
                groups[activeGroupIndex],
                activeGroupIndex,
                optionIndex,
              )}
            >
              <span>
                <strong>{provider.name}</strong>
                <small>{provider.id}</small>
              </span>
              {provider.id === value && <Check size={15} aria-hidden="true" />}
            </button>
          ))}
        </div>
      )}
    </div>,
    document.body,
  );

  return (
    <div
      ref={rootRef}
      className={`qt-native-picker ${open ? "is-open" : ""}`}
      style={{ paddingBottom: reservedMenuHeight || undefined }}
    >
      <button
        ref={triggerRef}
        type="button"
        className="qt-native-picker-trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        aria-label={ariaLabel}
        disabled={groups.length === 0}
        onClick={() => open ? closeMenu() : openMenu(false)}
        onKeyDown={onTriggerKeyDown}
      >
        <span className={`qt-native-picker-value ${selected ? "" : "is-placeholder"}`}>
          {selected && renderIcon(selected.id, "qt-native-picker-value-icon")}
          <span>{selected ? selected.name : placeholder}</span>
        </span>
        <ChevronDown size={16} aria-hidden="true" />
      </button>
      {menu}
    </div>
  );
}
