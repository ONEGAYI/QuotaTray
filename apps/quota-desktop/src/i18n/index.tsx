// 语言上下文：settings.language 三态（zh/en/system）解析为实际渲染语言，
// 提供 t(key, params) 插值取词。字典见 zh.ts（类型基准）/ en.ts。
// 与 Rust 侧 `src-tauri/src/i18n.rs` 是平行双实现，文案语义成对维护。
import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";
import { en } from "./en";
import { zh, type TextKey, type UiLang } from "./zh";
import { useSettings } from "../queries";

/** system 时按系统语言检测（zh 前缀 → 中文，其余含检测失败 → 英文）。 */
export function resolveUiLang(setting: string | undefined): UiLang {
  if (setting === "zh" || setting === "en") return setting;
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

/** {key} 插值：只替换 params 中出现的占位符（未提供的原样保留）。 */
export function interpolate(
  template: string,
  params?: Record<string, string | number>,
): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, key: string) =>
    key in params ? String(params[key]) : match,
  );
}

interface I18n {
  lang: UiLang;
  t: (key: TextKey, params?: Record<string, string | number>) => string;
}

const LangContext = createContext<I18n | null>(null);

export function LangProvider({ children }: { children: ReactNode }) {
  const settings = useSettings();
  const lang = resolveUiLang(settings.data?.language);

  // <html lang> 属性同步（无障碍与输入法提示）
  useEffect(() => {
    document.documentElement.lang = lang === "zh" ? "zh-CN" : "en";
  }, [lang]);

  const value = useMemo<I18n>(() => {
    const dict = lang === "zh" ? zh : en;
    return {
      lang,
      t: (key, params) => interpolate(dict[key], params),
    };
  }, [lang]);

  return <LangContext.Provider value={value}>{children}</LangContext.Provider>;
}

export function useLang(): I18n {
  const ctx = useContext(LangContext);
  if (!ctx) throw new Error("useLang must be used within LangProvider");
  return ctx;
}
