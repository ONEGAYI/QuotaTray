// 展示工具：相对时间与用量文案（纯函数，双语参数化）。
// 语义与 Rust 侧 tray.rs / i18n.rs 纯函数成对——分档边界、剩余/已用措辞
// 两端保持一致，修改任一侧须同步另一侧。
import type { UiLang } from "./i18n/zh";
import type { UsageData } from "./types";

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
