export interface Point {
  x: number;
  y: number;
}

/**
 * 鼠标只负责显现以文字中心为原点的柔光。
 * 返回 0..1：中心最强、有效半径外不可见，中间使用 smoothstep 平滑衰减。
 */
export function proximityGlowStrength(pointer: Point, labelCenter: Point, radius: number): number {
  if (radius <= 0) throw new Error("glow radius must be positive");

  const distance = Math.hypot(pointer.x - labelCenter.x, pointer.y - labelCenter.y);
  const proximity = Math.max(0, Math.min(1, 1 - distance / radius));
  return proximity * proximity * (3 - 2 * proximity);
}
