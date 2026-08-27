// 卡片拖拽排序的几何纯逻辑：拖拽状态机（useCardDragSort）每帧调用，
// 全部函数无 DOM 依赖，坐标取「列表内容顶部为原点」的静态槽位几何。
//
// 核心模型：拖拽期间 DOM 顺序不变，被拖卡片与让位卡片全靠 transform
// 模拟重排——其余卡片的让位偏移与新数组槽位一一对应（查表法），
// 因此高度参差的卡片也能精确落位，无需显式推导 gap。

/** 卡片静态几何：top 为相对列表内容顶部的纵坐标。 */
export interface DragItemRect {
  top: number;
  height: number;
}

/** 拖拽中心与其他卡片中心的跨越判定（dnd-kit sortable 同款语义）：
 *  目标槽位 = 中心低于拖拽中心的其他卡片数，天然夹取在 [0, n-1]。 */
export function computeTargetIndex(
  rects: DragItemRect[],
  dragIndex: number,
  dragCenterY: number,
): number {
  let target = 0;
  for (let i = 0; i < rects.length; i++) {
    if (i === dragIndex) continue;
    const center = rects[i].top + rects[i].height / 2;
    if (dragCenterY > center) target++;
  }
  return target;
}

/** 重排后每张卡片的新槽位（被拖卡片除外，其位移由跟手/落位逻辑自理）。 */
function remappedIndex(index: number, dragIndex: number, targetIndex: number): number {
  if (index === dragIndex) return index;
  if (targetIndex > dragIndex && index > dragIndex && index <= targetIndex) return index - 1;
  if (targetIndex < dragIndex && index >= targetIndex && index < dragIndex) return index + 1;
  return index;
}

/** 让位偏移（斥力表现）：shift(i) = 新槽位 top - 静态槽位 top。
 *  被拖卡片恒为 0（跟手 dy 与落位偏移单独计算）。 */
export function computeShifts(
  rects: DragItemRect[],
  dragIndex: number,
  targetIndex: number,
): number[] {
  return rects.map((rect, index) => {
    const next = remappedIndex(index, dragIndex, targetIndex);
    return rects[next].top - rect.top;
  });
}

/** 落位偏移：松手后被拖卡片从静态位滑向目标槽位的位移。 */
export function computeSettleOffset(
  rects: DragItemRect[],
  dragIndex: number,
  targetIndex: number,
): number {
  return rects[targetIndex].top - rects[dragIndex].top;
}

/** 数组重排：把 from 位元素移动到 to 位，返回新数组。非法索引抛错。 */
export function reorderIds<T>(items: T[], from: number, to: number): T[] {
  if (from < 0 || to < 0 || from >= items.length || to >= items.length || items.length === 0) {
    throw new RangeError(`reorderIds 索引越界：from=${from}, to=${to}, length=${items.length}`);
  }
  const next = [...items];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}

export const SETTLE_DURATION_MIN_MS = 200;
export const SETTLE_DURATION_MAX_MS = 340;

/** 落位动画时长：静止松手取基准，甩得越快滑行越久（惯性感），
 *  速度按绝对值计入（向上/向下甩同等待遇），夹取在 [MIN, MAX]。 */
export function settleDuration(velocityPxPerMs: number): number {
  const boost = Math.min(Math.abs(velocityPxPerMs) * 90, SETTLE_DURATION_MAX_MS - SETTLE_DURATION_MIN_MS);
  return Math.round(SETTLE_DURATION_MIN_MS + boost);
}

/** 键盘调序目标槽位：ArrowUp/Down 逐步、Home/End 跳首尾。
 *  无法移动（首卡 ↑、尾卡 ↓、已到位的 Home/End、其他键）返回 null。 */
export function nextKeyboardTarget(length: number, index: number, key: string): number | null {
  switch (key) {
    case "ArrowUp":
      return index > 0 ? index - 1 : null;
    case "ArrowDown":
      return index < length - 1 ? index + 1 : null;
    case "Home":
      return index > 0 ? 0 : null;
    case "End":
      return index < length - 1 ? length - 1 : null;
    default:
      return null;
  }
}

/** 样本窗口平均速度（px/ms）：拖拽指针轨迹 {t, y} 序列的首尾斜率，
 *  样本不足或时间跨度为零返回 0（无法估计即视为静止松手）。 */
export function velocityFromSamples(samples: readonly { t: number; y: number }[]): number {
  if (samples.length < 2) return 0;
  const first = samples[0];
  const last = samples[samples.length - 1];
  const dt = last.t - first.t;
  return dt > 0 ? (last.y - first.y) / dt : 0;
}
