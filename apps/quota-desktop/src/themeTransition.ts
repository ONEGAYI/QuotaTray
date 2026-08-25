// 主题切换圆形扩散动效（View Transitions API）：
// 切换时浏览器对整页新旧两帧截图，在 ::view-transition-new(root) 伪元素上
// 以 clip-path circle 从触发按钮位置扩散揭示新主题——波前经过处即新主题
// 真实界面（切到暗色即暗色底向外扩散，无需额外着色层）。
// WebView2（常青 Chromium ≥111）原生支持；不支持或用户偏好减少动效时
// 退化为瞬时切换。
import type { ResolvedTheme } from "./theme";

/** 扩散终态半径：点击点到视口四角的最大距离（保证圆形完全覆盖）。 */
export function expandRadius(x: number, y: number, width: number, height: number): number {
  return Math.max(
    Math.hypot(x, y),
    Math.hypot(width - x, y),
    Math.hypot(x, height - y),
    Math.hypot(width - x, height - y),
  );
}

/** 扩散参数 → CSS 变量集：供 index.css 的 ::view-transition-new(root) 关键帧消费。
 *  动画由样式表声明而非 JS 挂载 WAAPI——伪元素首帧即带 circle(0px) 起始裁剪，
 *  消除「过渡已渲染、动画未挂载」空窗导致的整屏新色闪现与起跑半径漂移。 */
export function themeOriginVars(
  origin: { x: number; y: number },
  radius: number,
): Record<"--qt-theme-x" | "--qt-theme-y" | "--qt-theme-r", string> {
  return {
    "--qt-theme-x": `${origin.x}px`,
    "--qt-theme-y": `${origin.y}px`,
    // 向上取整防最远角留边缘缝隙
    "--qt-theme-r": `${Math.ceil(radius)}px`,
  };
}

/** 是否值得做扩散：resolved 前后相同（如 dark→system 且系统同为暗色）无视觉变化。 */
export function shouldAnimate(from: ResolvedTheme, to: ResolvedTheme): boolean {
  return from !== to;
}

/** 扩散圆心：有效矩形（主题按钮锚点）取中心；缺失或零尺寸回退视口中心。 */
export function originFromRect(
  rect: { left: number; top: number; width: number; height: number } | null,
  viewport: { width: number; height: number },
): { x: number; y: number } {
  if (rect && rect.width > 0 && rect.height > 0) {
    return { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 };
  }
  return { x: viewport.width / 2, y: viewport.height / 2 };
}

/** 主题按钮中心（TitleBar 锚点 data-theme-trigger）；按钮不在（如异常时序）回退视口中心。
 *  用户点击与 system 跟随系统变色共用此圆心，保证扩散始终从按钮位置开始。 */
export function themeTriggerOrigin(): { x: number; y: number } {
  const rect = document.querySelector<HTMLElement>("[data-theme-trigger]")?.getBoundingClientRect() ?? null;
  return originFromRect(rect, { width: window.innerWidth, height: window.innerHeight });
}

/** 以圆形扩散切换到 next 主题；onApply 持久化设置（React 链路照常走）。 */
export function applyThemeTransition(
  next: ResolvedTheme,
  origin: { x: number; y: number },
  onApply: () => void,
): void {
  const applyDom = () => document.documentElement.classList.toggle("dark", next === "dark");
  if (
    typeof document.startViewTransition !== "function" ||
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  ) {
    applyDom();
    onApply();
    return;
  }
  // 回调内同步挂/摘 .dark class 立即产生新帧，不等 React 异步链路——
  // ThemeProvider effect 稍后到达相同 resolved 值，toggle 幂等不冲突
  // 过渡激活前先落扩散参数：CSS 关键帧（index.css）随伪元素首帧自动起跑，
  // 快速连点触发 skipTransition 时动画随伪元素销毁终止，无需额外处理
  const vars = themeOriginVars(
    origin,
    expandRadius(origin.x, origin.y, window.innerWidth, window.innerHeight),
  );
  const root = document.documentElement;
  for (const [name, value] of Object.entries(vars)) {
    root.style.setProperty(name, value);
  }
  document.startViewTransition(() => {
    applyDom();
    onApply();
  });
}
