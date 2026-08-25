// 主题上下文：settings.theme 三态（light/dark/system）解析为实际主题。
// - system 时监听 prefers-color-scheme 实时跟随；
// - 解析结果挂/摘根 <html> 的 .dark class（Tailwind 4 custom variant，见 index.css）；
// - 联动原生 titlebar：getCurrentWindow().setTheme('light'|'dark')，
//   system 传 null 跟随系统（Tauri 2 API 契约，需 capability 放行）；
// - 同步推送后端 set_resolved_theme——托盘圆环图标配色随解析后主题刷新
//   （选前端推送而非 Rust 监听 WindowEvent::ThemeChanged 的理由见
//   src-tauri/src/commands.rs 的 set_resolved_theme 注释）。
// - 变色均走圆形扩散动效，圆心统一取主题按钮位置（themeTriggerOrigin）：
//   用户在 TitleBar 切换与 system 跟随系统变色同源（themeTransition.ts）。
import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";
import { useSettings } from "./queries";
import { applyThemeTransition, themeTriggerOrigin } from "./themeTransition";

export type ResolvedTheme = "light" | "dark";

function systemTheme(): ResolvedTheme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** 三态解析：显式 light/dark 直取，system（及未知值）跟随系统。
 *  导出供 TitleBar 在切换前算出目标实际主题，驱动圆形扩散动效（themeTransition.ts）。 */
export function resolveSetting(setting: string): ResolvedTheme {
  if (setting === "light" || setting === "dark") return setting;
  return systemTheme();
}

const ThemeContext = createContext<ResolvedTheme>("light");

export function ThemeProvider({ children }: { children: ReactNode }) {
  const settings = useSettings();
  const setting = settings.data?.theme ?? "system";
  const [resolved, setResolved] = useState<ResolvedTheme>(systemTheme);

  // 三态解析；system 时跟随系统变化（mq change）——系统变色同样从主题按钮
  // 位置圆形扩散（themeTriggerOrigin，按钮不在才回退视口中心）
  useEffect(() => {
    const apply = () => setResolved(resolveSetting(setting));
    apply();
    if (setting !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      const next = systemTheme();
      // 回声防御：setTheme(null) 经 tao ThemeChanged → WebView2
      // PreferredColorScheme 翻转 prefers-color-scheme，本监听器会收到
      // 自己动作的回声。DOM 实际主题已等于系统色（用户刚主动切换、
      // 过渡正在播放）时跳过——否则第二次 startViewTransition 会按
      // 规范 skip 掉进行中的动画，表现为瞬间变色。比对用 DOM class
      // 而非 resolved 状态：class 在过渡回调内同步写入，回声链路
      // （IPC→tao→wry→COM→媒体查询）必然晚于该帧，无竞态。
      const applied = document.documentElement.classList.contains("dark") ? "dark" : "light";
      if (next === applied) {
        setResolved(next);
        return;
      }
      applyThemeTransition(next, themeTriggerOrigin(), () => {});
      setResolved(next);
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [setting]);

  // 根 class + 原生 titlebar + 托盘图标主题推送（缺失通知不阻断 UI）
  useEffect(() => {
    document.documentElement.classList.toggle("dark", resolved === "dark");
    const win = getCurrentWindow();
    void win
      .setTheme(setting === "system" ? null : resolved)
      .catch((e) => console.error("setTheme 调用失败：", e));
    void api.setResolvedTheme(resolved).catch((e) =>
      console.error("托盘主题推送失败：", e),
    );
  }, [resolved, setting]);

  const value = useMemo(() => resolved, [resolved]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ResolvedTheme {
  return useContext(ThemeContext);
}
