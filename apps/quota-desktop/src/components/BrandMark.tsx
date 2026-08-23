import type { ImgHTMLAttributes } from "react";
import brandMarkUrl from "../assets/brand-mark.png";

/** 静态品牌标志；运行时托盘圆环仍由 Rust 按余额动态绘制。 */
export function BrandMark(props: Omit<ImgHTMLAttributes<HTMLImageElement>, "src" | "alt">) {
  return <img {...props} src={brandMarkUrl} alt="" aria-hidden="true" draggable={false} />;
}
