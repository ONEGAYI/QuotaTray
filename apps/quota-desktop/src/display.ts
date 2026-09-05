// 展示工具：相对时间与用量文案（纯函数，双语参数化）。
// 语义与 Rust 侧 tray.rs / i18n.rs 纯函数成对——分档边界、剩余/已用措辞
// 两端保持一致，修改任一侧须同步另一侧。
import type { UiLang } from "./i18n/zh";
import type { ProviderEntry, UsageData } from "./types";

/** 条目类型标签（平台副标题）：native 用平台名，模板/脚本各归各
 *  （与 CLI render.rs kind_label 成对，script 不得落入模板文案）。 */
export function kindLabel(
  kind: ProviderEntry["kind"],
  nativeName: string | undefined,
  lang: UiLang,
): string {
  switch (kind.type) {
    case "native":
      return nativeName ?? kind.provider;
    case "template":
      return lang === "zh" ? "模板" : "template";
    case "script":
      return lang === "zh" ? "脚本" : "script";
  }
}

/** 相对时间："刚刚 / N 秒前 / …"（分档与 tray.rs relative_time 一致）。 */
export function relativeTime(atMs: number | null | undefined, lang: UiLang): string {
  if (!atMs) return "—";
  const secs = Math.floor((Date.now() - atMs) / 1000);
  const zh = lang === "zh";
  if (secs < 10) return zh ? "刚刚" : "just now";
  if (secs < 60) return zh ? `${secs} 秒前` : `${secs}s ago`;
  if (secs < 3600) {
    const n = Math.floor(secs / 60);
    return zh ? `${n} 分钟前` : `${n}m ago`;
  }
  if (secs < 86_400) {
    const n = Math.floor(secs / 3600);
    return zh ? `${n} 小时前` : `${n}h ago`;
  }
  const n = Math.floor(secs / 86_400);
  return zh ? `${n} 天前` : `${n}d ago`;
}

/** 最后成功时刻的精确本地时间（Tooltip 使用）。 */
export function exactTime(atMs: number, lang: UiLang): string {
  return new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(atMs));
}

/** 已用百分比（0-100）：unit="%" 直读，否则 used/total 换算；数据不足返回 null。 */
export function usedPercent(d: UsageData): number | null {
  if (d.unit === "%") return d.used ?? null;
  if (d.used != null && d.total != null && d.total > 0) {
    return (d.used / d.total) * 100;
  }
  return null;
}

/** 余额文案："62.97 CNY" / "62.97"。 */
export function amountText(v: number): string {
  return v.toFixed(2);
}

/** 单窗口数据的主文案（与 tray.rs 行体措辞成对：已用/剩余/已获取）。 */
export function dataSummary(d: UsageData, lang: UiLang): string {
  const zh = lang === "zh";
  const pct = usedPercent(d);
  if (pct != null) {
    const p = `${Math.round(pct)}%`;
    return zh ? `已用 ${p}` : `Used ${p}`;
  }
  if (d.remaining != null) {
    const amount = amountText(d.remaining) + (d.unit ? ` ${d.unit}` : "");
    return zh ? `剩余 ${amount}` : `Left ${amount}`;
  }
  return zh ? "已获取" : "Fetched";
}

/** 额度重置倒计时（语言中性缩写，与 CLI fmt_reset_countdown 成对）：
 *  "21m" / "3h21m" / "4d17h"；缺省或已到期返回 null（无展示意义）。
 *  跨入天级后丢弃分钟粒度（周/月窗口小时精度已足够）。 */
export function resetCountdown(resetAtMs: number | null | undefined, nowMs: number = Date.now()): string | null {
  if (resetAtMs == null) return null;
  const totalMin = Math.floor((resetAtMs - nowMs) / 60_000);
  if (totalMin <= 0) return null;
  if (totalMin < 60) return `${totalMin}m`;
  const hours = Math.floor(totalMin / 60);
  if (hours < 24) {
    return totalMin % 60 === 0 ? `${hours}h` : `${hours}h${totalMin % 60}m`;
  }
  const days = Math.floor(hours / 24);
  return hours % 24 === 0 ? `${days}d` : `${days}d${hours % 24}h`;
}

/** 定位线时间差（"2天3小时15分" / "2d 3h 15m"）：分钟向下取整且至少
 *  1 分钟（毫秒级差异对测量无意义）；天/时为 0 的段省略，分钟恒显。
 *  与 resetCountdown 的缩写风格不同——定位线差值是主读数，用完整词。 */
export function markerSpanText(diffMs: number, lang: UiLang): string {
  const totalMin = Math.max(1, Math.floor(diffMs / 60_000));
  const days = Math.floor(totalMin / 1_440);
  const hours = Math.floor((totalMin % 1_440) / 60);
  const minutes = totalMin % 60;
  if (lang === "zh") {
    return `${days > 0 ? `${days}天` : ""}${hours > 0 ? `${hours}小时` : ""}${minutes}分`;
  }
  return [days > 0 ? `${days}d` : "", hours > 0 ? `${hours}h` : "", `${minutes}m`]
    .filter(Boolean)
    .join(" ");
}

/** 多窗口短标签：取 plan_name 全角括号内的窗口标注
 *  （"GLM Coding Plan（5h）" → "5h"；week 映射双语"周限"/"weekly"）。
 *  无括号用全名（template 窗口名），无名回退"窗口 N"。 */
export function windowShortLabel(
  planName: string | undefined,
  index: number,
  lang: UiLang,
): string {
  const zh = lang === "zh";
  const paren = planName?.match(/[（(]([^（）()]+)[)）]\s*$/)?.[1];
  const raw = paren ?? planName;
  if (!raw) return zh ? `窗口 ${index + 1}` : `window ${index + 1}`;
  if (raw === "week") return zh ? "周限" : "weekly";
  return raw;
}
