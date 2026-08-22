// 展示工具：相对时间与用量文案（语义与 Rust 侧 tray.rs 纯函数一致）。
import type { UsageData } from "./types";

/** 相对时间："刚刚" / "N 秒前" / "N 分钟前" / "N 小时前" / "N 天前"。 */
export function relativeTime(atMs: number | null | undefined): string {
  if (!atMs) return "—";
  const secs = Math.floor((Date.now() - atMs) / 1000);
  if (secs < 10) return "刚刚";
  if (secs < 60) return `${secs} 秒前`;
  if (secs < 3600) return `${Math.floor(secs / 60)} 分钟前`;
  if (secs < 86_400) return `${Math.floor(secs / 3600)} 小时前`;
  return `${Math.floor(secs / 86_400)} 天前`;
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

/** 单窗口数据的主文案。 */
export function dataSummary(d: UsageData): string {
  const pct = usedPercent(d);
  if (pct != null) return `已用 ${Math.round(pct)}%`;
  if (d.remaining != null) {
    return `剩余 ${amountText(d.remaining)}${d.unit ? ` ${d.unit}` : ""}`;
  }
  return "已获取";
}
