import { useMutation, useQueryClient } from "@tanstack/react-query";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
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
import { invalidateDisplaySettingsCache, useSettings } from "../queries";
import { resolveSetting, useTheme } from "../theme";
import { applyThemeTransition, shouldAnimate, themeTriggerOrigin } from "../themeTransition";
import type { SettingsPatch } from "../types";
import { BrandMark } from "./BrandMark";
import { DropdownMenu, IconButton, MenuItem } from "./ui";

type MenuKind = "language" | "theme" | null;

// 仓库主页（与 core `update.rs` 的 GITHUB_REPO 常量及 capability scope 同源对齐）。
const GITHUB_REPO_URL = "https://github.com/ONEGAYI/QuotaTray";

// GitHub 官方 octicon（mark-github 16），fill 跟随 currentColor，
// 借助主题令牌在明暗两套主题下自动换色。
function GithubMark() {
  return (
    <svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}

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

  // 快切入口走后端 patch 合并（读现值→覆盖单字段），不基于前端缓存
  // 全量提交——缓存陈旧时会把手未提交的设置整体抹回默认
  const save = useMutation({
    mutationFn: (patch: SettingsPatch) => api.patchSettings(patch),
    onSuccess: () => invalidateDisplaySettingsCache(qc),
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

  // 选定主题：实际主题有变化时从主题按钮位置圆形扩散过渡，否则直接保存
  const pickTheme = (value: string) => {
    setMenu(null);
    if (settingTheme === value) return;
    const next = resolveSetting(value);
    if (!shouldAnimate(resolvedTheme, next)) {
      save.mutate({ theme: value });
      return;
    }
    applyThemeTransition(next, themeTriggerOrigin(), () => save.mutate({ theme: value }));
  };

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
        <IconButton
          label={t("titlebar.github")}
          onClick={() => void openUrl(GITHUB_REPO_URL).catch(() => {})}
        >
          <GithubMark />
        </IconButton>

        <div className="qt-titlebar-menu-anchor">
          <IconButton
            icon={Languages}
            label={t("titlebar.language")}
            disabled={save.isPending}
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

        {/* data-theme-trigger：主题按钮锚点，扩散动效圆心来源（themeTransition.ts） */}
        <div data-theme-trigger className="qt-titlebar-menu-anchor">
          <IconButton
            icon={ThemeTriggerIcon}
            label={t("titlebar.theme")}
            disabled={save.isPending}
            aria-expanded={menu === "theme"}
            onClick={() => setMenu(menu === "theme" ? null : "theme")}
          />
          <DropdownMenu open={menu === "theme"} onClose={() => setMenu(null)}>
            {themeOptions.map((option) => (
              <MenuItem
                key={option.value}
                icon={option.icon}
                checked={settingTheme === option.value}
                onClick={() => pickTheme(option.value)}
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
