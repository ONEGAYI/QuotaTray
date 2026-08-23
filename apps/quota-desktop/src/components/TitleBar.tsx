// 自定义标题栏（无装饰窗口，decorations:false 见 tauri.conf.json）：
// - 左侧应用标识 + 空白区可拖动窗口（data-tauri-drag-region），双击切换最大化（Windows 惯例）；
// - 语言 / 主题为图标按钮 + 下拉三选，点击即走 save_settings（磁盘权威）即时生效——
//   两个设置已从设置对话框移入此处；
// - 右侧三个系统窗口控制：最小化 / 最大化还原 / 关闭。关闭走 close()，
//   触发 Rust 侧 CloseRequested → 隐藏收托盘（语义与原生标题栏一致）。
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../api";
import { useLang, type TextKey } from "../i18n";
import { useSettings } from "../queries";
import type { Settings } from "../types";

/** 当前展开的下拉：语言 / 主题 / 无。同一时间至多一个。 */
type MenuKind = "lang" | "theme" | null;

const ctrlBtn =
  "flex h-9 w-11 items-center justify-center text-slate-500 transition-colors " +
  "hover:bg-slate-200/80 dark:text-slate-400 dark:hover:bg-slate-700/80";
const quickBtn =
  "flex h-7 w-8 items-center justify-center rounded text-slate-500 transition-colors " +
  "hover:bg-slate-200/80 disabled:opacity-40 dark:text-slate-400 dark:hover:bg-slate-700/80";

// ---- 内联图标（线框风格，随 currentColor 着色） ----
const iconProps = {
  width: 14,
  height: 14,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round",
  strokeLinejoin: "round",
} as const;

const Globe = () => (
  <svg {...iconProps}>
    <circle cx="12" cy="12" r="9" />
    <path d="M3 12h18M12 3c2.7 2.6 4 5.7 4 9s-1.3 6.4-4 9c-2.7-2.6-4-5.7-4-9s1.3-6.4 4-9z" />
  </svg>
);
const Sun = () => (
  <svg {...iconProps}>
    <circle cx="12" cy="12" r="4.2" />
    <path d="M12 2.5v2.4M12 19.1v2.4M2.5 12h2.4M19.1 12h2.4M5 5l1.7 1.7M17.3 17.3 19 19M19 5l-1.7 1.7M6.7 17.3 5 19" />
  </svg>
);
const Moon = () => (
  <svg {...iconProps}>
    <path d="M20.5 14.2A8.5 8.5 0 0 1 9.8 3.5a8.5 8.5 0 1 0 10.7 10.7z" />
  </svg>
);
const Monitor = () => (
  <svg {...iconProps}>
    <rect x="3" y="4" width="18" height="13" rx="1.5" />
    <path d="M9 21h6M12 17.5V21" />
  </svg>
);
const MinIcon = () => (
  <svg {...iconProps} strokeWidth={1.4}>
    <path d="M5 12h14" />
  </svg>
);
const MaxIcon = () => (
  <svg {...iconProps} strokeWidth={1.4}>
    <rect x="6" y="6" width="12" height="12" rx="1" />
  </svg>
);
const RestoreIcon = () => (
  <svg {...iconProps} strokeWidth={1.4}>
    <rect x="5" y="9" width="10" height="10" rx="1" />
    <path d="M9 6.5V5.8A.8.8 0 0 1 9.8 5h8.4a.8.8 0 0 1 .8.8v8.4a.8.8 0 0 1-.8.8h-.7" />
  </svg>
);
const CloseIcon = () => (
  <svg {...iconProps} strokeWidth={1.4}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);
const Check = () => (
  <svg {...iconProps} width={13} height={13}>
    <path d="M4.5 12.5l5 5L19.5 7" />
  </svg>
);

/** 下拉菜单项：当前值打勾。 */
function MenuItem({
  checked,
  label,
  onClick,
}: {
  checked: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="flex w-full items-center gap-1.5 rounded px-2.5 py-1.5 text-left text-sm text-slate-700 hover:bg-slate-100 dark:text-slate-200 dark:hover:bg-slate-700"
    >
      <span className="w-3.5 shrink-0">{checked && <Check />}</span>
      {label}
    </button>
  );
}

/** 三选下拉（语言/主题共用外壳）：锚定在触发按钮下方。 */
function Dropdown({
  children,
  onClose,
}: {
  children: ReactNode;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // 点击菜单外（含另一个快捷按钮）即关闭
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [onClose]);
  return (
    <div
      ref={ref}
      className="absolute right-0 top-8 z-20 w-36 rounded-md border border-slate-200 bg-white p-1 shadow-lg dark:border-slate-600 dark:bg-slate-800"
    >
      {children}
    </div>
  );
}

export function TitleBar() {
  const { t } = useLang();
  const qc = useQueryClient();
  const settings = useSettings();
  const [menu, setMenu] = useState<MenuKind>(null);
  const [maximized, setMaximized] = useState(false);
  const win = getCurrentWindow();

  // 最大化状态：初次查询 + 窗口尺寸变化时复查（双击标题栏/系统快捷键均覆盖）
  useEffect(() => {
    const sync = () => void win.isMaximized().then(setMaximized).catch(() => {});
    sync();
    window.addEventListener("resize", sync);
    return () => window.removeEventListener("resize", sync);
  }, [win]);

  // 快捷切换保存：patch 单字段，其余取当前 settings（磁盘权威链路）
  const save = useMutation({
    mutationFn: (patch: Partial<Settings>) => {
      const base = settings.data;
      if (!base) throw new Error("settings not loaded");
      return api.saveSettings({ ...base, ...patch });
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["settings"] });
      void qc.invalidateQueries({ queryKey: ["provider"] }); // 间隔等联动即时生效
    },
  });

  const langOptions: Array<{ value: string; key: TextKey }> = [
    { value: "zh", key: "settings.langZh" },
    { value: "en", key: "settings.langEn" },
    { value: "system", key: "settings.langSystem" },
  ];
  const themeOptions: Array<{ value: string; key: TextKey; icon: ReactNode }> = [
    { value: "light", key: "settings.themeLight", icon: <Sun /> },
    { value: "dark", key: "settings.themeDark", icon: <Moon /> },
    { value: "system", key: "settings.themeSystem", icon: <Monitor /> },
  ];

  const settingLang = settings.data?.language ?? "system";
  const settingTheme = settings.data?.theme ?? "system";
  const themeIcon =
    settingTheme === "light" ? <Sun /> : settingTheme === "dark" ? <Moon /> : <Monitor />;

  return (
    <header className="flex h-9 select-none items-center bg-white dark:bg-slate-800">
      {/* 拖动区：仅直接命中本元素触发拖动，按钮为其子元素不受影响 */}
      <div
        data-tauri-drag-region
        onDoubleClick={() => void win.toggleMaximize()}
        className="flex h-full min-w-0 flex-1 items-center gap-2.5 pl-3.5"
      >
        <span data-tauri-drag-region className="truncate text-sm font-semibold">
          QuotaTray
        </span>
        <span
          data-tauri-drag-region
          className="truncate text-xs text-slate-400 dark:text-slate-500"
        >
          {t("app.subtitle")}
        </span>
      </div>

      {/* 语言 / 主题快捷菜单 */}
      <div className="relative mr-1 flex items-center">
        <button
          className={quickBtn}
          disabled={!settings.data || save.isPending}
          title={t("titlebar.language")}
          onClick={() => setMenu(menu === "lang" ? null : "lang")}
        >
          <Globe />
        </button>
        {menu === "lang" && (
          <Dropdown onClose={() => setMenu(null)}>
            {langOptions.map((o) => (
              <MenuItem
                key={o.value}
                checked={settingLang === o.value}
                label={t(o.key)}
                onClick={() => {
                  setMenu(null);
                  if (settingLang !== o.value) save.mutate({ language: o.value });
                }}
              />
            ))}
          </Dropdown>
        )}

        <button
          className={quickBtn}
          disabled={!settings.data || save.isPending}
          title={t("titlebar.theme")}
          onClick={() => setMenu(menu === "theme" ? null : "theme")}
        >
          {themeIcon}
        </button>
        {menu === "theme" && (
          <Dropdown onClose={() => setMenu(null)}>
            {themeOptions.map((o) => (
              <MenuItem
                key={o.value}
                checked={settingTheme === o.value}
                label={t(o.key)}
                onClick={() => {
                  setMenu(null);
                  if (settingTheme !== o.value) save.mutate({ theme: o.value });
                }}
              />
            ))}
          </Dropdown>
        )}
      </div>

      {/* 分隔 + 系统窗口控制 */}
      <div className="h-4 w-px bg-slate-200 dark:bg-slate-600" />
      <button
        className={ctrlBtn}
        title={t("titlebar.minimize")}
        onClick={() => void win.minimize()}
      >
        <MinIcon />
      </button>
      <button
        className={ctrlBtn}
        title={maximized ? t("titlebar.restore") : t("titlebar.maximize")}
        onClick={() => void win.toggleMaximize()}
      >
        {maximized ? <RestoreIcon /> : <MaxIcon />}
      </button>
      <button
        className={`${ctrlBtn} hover:bg-[#e81123] hover:text-white dark:hover:bg-[#e81123]`}
        title={t("titlebar.close")}
        onClick={() => void win.close()}
      >
        <CloseIcon />
      </button>
    </header>
  );
}
