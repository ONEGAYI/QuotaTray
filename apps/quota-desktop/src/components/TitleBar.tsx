import { useMutation, useQueryClient } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Copy,
  Languages,
  Minus,
  Monitor,
  Moon,
  Square,
  Sun,
  X,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useLang, type TextKey } from "../i18n";
import { useSettings } from "../queries";
import { useTheme } from "../theme";
import type { Settings } from "../types";
import { BrandMark } from "./BrandMark";
import { DropdownMenu, IconButton, MenuItem } from "./ui";

type MenuKind = "language" | "theme" | null;

export function TitleBar() {
  const { t } = useLang();
  const resolvedTheme = useTheme();
  const qc = useQueryClient();
  const settings = useSettings();
  const [menu, setMenu] = useState<MenuKind>(null);
  const [maximized, setMaximized] = useState(false);
  const win = getCurrentWindow();

  useEffect(() => {
    const sync = () => void win.isMaximized().then(setMaximized).catch(() => {});
    sync();
    window.addEventListener("resize", sync);
    return () => window.removeEventListener("resize", sync);
  }, [win]);

  const save = useMutation({
    mutationFn: (patch: Partial<Settings>) => {
      if (!settings.data) throw new Error("settings not loaded");
      return api.saveSettings({ ...settings.data, ...patch });
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["settings"] });
      void qc.invalidateQueries({ queryKey: ["provider"] });
    },
  });

  const languageOptions: Array<{ value: string; key: TextKey }> = [
    { value: "zh", key: "settings.langZh" },
    { value: "en", key: "settings.langEn" },
    { value: "system", key: "settings.langSystem" },
  ];
  const themeOptions: Array<{ value: string; key: TextKey; icon: LucideIcon }> = [
    { value: "light", key: "settings.themeLight", icon: Sun },
    { value: "dark", key: "settings.themeDark", icon: Moon },
    { value: "system", key: "settings.themeSystem", icon: Monitor },
  ];

  const settingLanguage = settings.data?.language ?? "system";
  const settingTheme = settings.data?.theme ?? "system";
  const ThemeTriggerIcon =
    settingTheme === "system" ? Monitor : resolvedTheme === "dark" ? Sun : Moon;

  return (
    <header className="qt-titlebar">
      <div
        data-tauri-drag-region
        onDoubleClick={() => void win.toggleMaximize()}
        className="qt-titlebar-drag"
      >
        <BrandMark data-tauri-drag-region className="qt-app-mark" />
        <span data-tauri-drag-region className="qt-titlebar-name">QuotaTray</span>
        <span data-tauri-drag-region className="qt-titlebar-subtitle">
          {t("app.subtitle")}
        </span>
      </div>

      <div className="qt-titlebar-actions">
        <div className="qt-titlebar-menu-anchor">
          <IconButton
            icon={Languages}
            label={t("titlebar.language")}
            disabled={!settings.data || save.isPending}
            aria-expanded={menu === "language"}
            onClick={() => setMenu(menu === "language" ? null : "language")}
          />
          <DropdownMenu open={menu === "language"} onClose={() => setMenu(null)}>
            {languageOptions.map((option) => (
              <MenuItem
                key={option.value}
                checked={settingLanguage === option.value}
                onClick={() => {
                  setMenu(null);
                  if (settingLanguage !== option.value) save.mutate({ language: option.value });
                }}
              >
                {t(option.key)}
              </MenuItem>
            ))}
          </DropdownMenu>
        </div>

        <div className="qt-titlebar-menu-anchor">
          <IconButton
            icon={ThemeTriggerIcon}
            label={t("titlebar.theme")}
            disabled={!settings.data || save.isPending}
            aria-expanded={menu === "theme"}
            onClick={() => setMenu(menu === "theme" ? null : "theme")}
          />
          <DropdownMenu open={menu === "theme"} onClose={() => setMenu(null)}>
            {themeOptions.map((option) => (
              <MenuItem
                key={option.value}
                icon={option.icon}
                checked={settingTheme === option.value}
                onClick={() => {
                  setMenu(null);
                  if (settingTheme !== option.value) save.mutate({ theme: option.value });
                }}
              >
                {t(option.key)}
              </MenuItem>
            ))}
          </DropdownMenu>
        </div>

        <span className="qt-titlebar-divider" />
        <button
          className="qt-window-control"
          aria-label={t("titlebar.minimize")}
          onClick={() => void win.minimize()}
        >
          <Minus size={15} aria-hidden="true" />
        </button>
        <button
          className="qt-window-control"
          aria-label={maximized ? t("titlebar.restore") : t("titlebar.maximize")}
          onClick={() => void win.toggleMaximize()}
        >
          {maximized ? <Copy size={14} aria-hidden="true" /> : <Square size={14} aria-hidden="true" />}
        </button>
        <button
          className="qt-window-control qt-window-close"
          aria-label={t("titlebar.close")}
          onClick={() => void win.close()}
        >
          <X size={16} aria-hidden="true" />
        </button>
      </div>
    </header>
  );
}
