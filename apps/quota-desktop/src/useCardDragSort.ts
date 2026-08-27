// 卡片拖拽排序状态机：几何计算复用 components/dragSortView 纯函数，
// 本 hook 只负责指针事件、rAF 循环与 DOM 直写的编排。
//
// 渲染分轨（性能关键）：让位卡片偏移走 React state（低频，目标槽位变化
// 才更新）；被拖卡片跟手 transform 每帧直写 style 绕过 React——否则
// 整列表每帧重渲染。直写残留由此带来一个约束：commit 必须用 flushSync
// 同步重排 DOM 后手动清除直写值，保证「动画终点 == 新 DOM 位置」在
// 同一帧对齐，视觉零跳变（React 不清除它未曾设过的 inline style）。
import { useCallback, useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import {
  computeSettleOffset,
  computeShifts,
  computeTargetIndex,
  reorderIds,
  settleDuration,
  type DragItemRect,
} from "./components/dragSortView";

/** 超过该位移才从 armed 进入拖拽（避免点按误触）。 */
const ACTIVATE_THRESHOLD_PX = 4;
/** pointer 距滚动容器视口边小于该值时启动边缘自动滚动。 */
const EDGE_SCROLL_MARGIN_PX = 56;
const MAX_EDGE_SCROLL_PER_FRAME = 14;
/** 速度样本窗口：只用最近样本估计松手速度（px/ms）。 */
const VELOCITY_WINDOW_MS = 120;
/** 落位/回弹曲线：快出慢收（easeOutQuint 近似），承载惯性感。 */
const SETTLE_CURVE = "cubic-bezier(0.22, 1, 0.36, 1)";
/** 落位动画兜底超时余量（transitionend 可能因值未变而不触发）。 */
const SETTLE_TIMEOUT_SLACK_MS = 80;

type DragPhase = "idle" | "drag" | "settle" | "cancel";

interface ArmedState {
  pointerId: number;
  startY: number;
  dragId: string;
}

interface DragSession {
  pointerId: number;
  dragId: string;
  dragIndex: number;
  startY: number;
  startScrollTop: number;
  rects: DragItemRect[];
  targetIndex: number;
  card: HTMLElement;
  samples: { t: number; y: number }[];
  latestY: number;
  reducedMotion: boolean;
}

export interface CardDragSortOptions {
  /** 列表容器（卡片直接父级，卡片带 data-card-id）。 */
  containerRef: React.RefObject<HTMLDivElement | null>;
  /** 当前列表 id 顺序（与渲染顺序一致）。 */
  ids: string[];
  /** 拖拽/键盘调序完成后提交新顺序（同步数据更新；异步落库由调用方自理）。 */
  onCommit: (orderedIds: string[]) => void;
}

export interface DragHandleProps {
  onPointerDown: (event: React.PointerEvent) => void;
  onKeyDown: (event: React.KeyboardEvent) => void;
  disabled: boolean;
}

export interface CardDragSort {
  /** 正在被拖拽的卡片 id（settle/cancel 阶段保留，idle 清空）。 */
  dragId: string | null;
  /** 拖拽会话进行中（含落位/回弹动画阶段），列表容器据此抑制 hover。 */
  active: boolean;
  /** 让位偏移（id → px）。仅 active 期间有意义。 */
  shifts: Record<string, number>;
  handleProps: (id: string) => DragHandleProps;
}

export function useCardDragSort(options: CardDragSortOptions): CardDragSort {
  const { containerRef, ids } = options;

  const [dragId, setDragId] = useState<string | null>(null);
  const [phase, setPhase] = useState<DragPhase>("idle");
  const [shifts, setShifts] = useState<Record<string, number>>({});

  // ids/onCommit 在事件回调里取最新值，避免闭包悬挂旧列表
  const idsRef = useRef(ids);
  idsRef.current = ids;
  const onCommitRef = useRef(options.onCommit);
  onCommitRef.current = options.onCommit;

  const armRef = useRef<ArmedState | null>(null);
  const sessionRef = useRef<DragSession | null>(null);
  const rafRef = useRef(0);
  const scrollerRef = useRef<HTMLElement | null>(null);

  /** 拖拽会话期间常驻 rAF 循环：边缘滚动 + 跟手 transform + 目标槽位。 */
  const tick = useCallback(() => {
    const session = sessionRef.current;
    if (!session) return;
    // 边缘自动滚动：越深入边缘速度越快，scrollTop 变化经 dy 补偿进坐标
    const scroller = scrollerRef.current;
    if (scroller) {
      const bounds = scroller.getBoundingClientRect();
      const speed = (depth: number) =>
        Math.min((depth / EDGE_SCROLL_MARGIN_PX) * MAX_EDGE_SCROLL_PER_FRAME + 2, MAX_EDGE_SCROLL_PER_FRAME);
      if (session.latestY < bounds.top + EDGE_SCROLL_MARGIN_PX) {
        const depth = bounds.top + EDGE_SCROLL_MARGIN_PX - session.latestY;
        scroller.scrollTop = Math.max(0, scroller.scrollTop - speed(depth));
      } else if (session.latestY > bounds.bottom - EDGE_SCROLL_MARGIN_PX) {
        const depth = session.latestY - (bounds.bottom - EDGE_SCROLL_MARGIN_PX);
        scroller.scrollTop = Math.min(scroller.scrollHeight, scroller.scrollTop + speed(depth));
      }
    }
    const dy =
      session.latestY - session.startY + ((scroller?.scrollTop ?? session.startScrollTop) - session.startScrollTop);
    session.card.style.transform = `translateY(${dy}px) scale(1.012)`;
    const center = session.rects[session.dragIndex].top + session.rects[session.dragIndex].height / 2 + dy;
    const target = computeTargetIndex(session.rects, session.dragIndex, center);
    if (target !== session.targetIndex) {
      session.targetIndex = target;
      const next = computeShifts(session.rects, session.dragIndex, target);
      setShifts(Object.fromEntries(idsRef.current.map((id, index) => [id, next[index]])));
    }
    rafRef.current = requestAnimationFrame(tick);
  }, []);

  /** 最近样本窗口内的平均速度（px/ms）。 */
  const velocity = useCallback(() => {
    const session = sessionRef.current;
    if (!session || session.samples.length < 2) return 0;
    const first = session.samples[0];
    const last = session.samples[session.samples.length - 1];
    const dt = last.t - first.t;
    return dt > 0 ? (last.y - first.y) / dt : 0;
  }, []);

  const clearCardInline = useCallback((card: HTMLElement) => {
    card.style.transform = "";
    card.style.transition = "";
    card.style.zIndex = "";
  }, []);

  /** 动画终点收尾：同步重排数据、清除直写残留、回到 idle。 */
  const finalize = useCallback(
    (session: DragSession, commit: boolean) => {
      if (commit && session.targetIndex !== session.dragIndex) {
        flushSync(() => {
          onCommitRef.current(reorderIds(idsRef.current, session.dragIndex, session.targetIndex));
        });
      }
      clearCardInline(session.card);
      setDragId(null);
      setShifts({});
      setPhase("idle");
    },
    [clearCardInline],
  );

  /** 结束拖拽：commit 落位到目标槽位，否则整体动画回原位（ESC/中断）。 */
  const endDrag = useCallback(
    (commit: boolean) => {
      const session = sessionRef.current;
      if (!session) return;
      const v = velocity();
      sessionRef.current = null;
      armRef.current = null;
      cancelAnimationFrame(rafRef.current);
      if (session.reducedMotion) {
        finalize(session, commit);
        return;
      }
      const duration = settleDuration(commit ? v : 0);
      const offset = commit
        ? computeSettleOffset(session.rects, session.dragIndex, session.targetIndex)
        : 0;
      session.card.style.transition = `transform ${duration}ms ${SETTLE_CURVE}`;
      session.card.style.transform = `translateY(${offset}px) scale(1)`;
      if (!commit) setShifts({}); // 让位卡片同步动画回原位
      setPhase(commit ? "settle" : "cancel");
      let done = false;
      const finish = () => {
        if (done) return;
        done = true;
        window.clearTimeout(timer);
        session.card.removeEventListener("transitionend", onEnd);
        finalize(session, commit);
      };
      const onEnd = (event: TransitionEvent) => {
        if (event.target === session.card && event.propertyName === "transform") finish();
      };
      const timer = window.setTimeout(finish, duration + SETTLE_TIMEOUT_SLACK_MS);
      session.card.addEventListener("transitionend", onEnd);
    },
    [finalize, velocity],
  );

  /** armed 超过激活阈值：测量槽位几何，进入拖拽会话。 */
  const beginDrag = useCallback(
    (arm: ArmedState, clientY: number) => {
      const container = containerRef.current;
      if (!container) return;
      const scroller = container.closest<HTMLElement>(".qt-main-content");
      scrollerRef.current = scroller;
      const byId = new Map(
        Array.from(container.querySelectorAll<HTMLElement>("[data-card-id]"), (el) => [el.dataset.cardId ?? "", el]),
      );
      const containerTop = container.getBoundingClientRect().top;
      const rects: DragItemRect[] = [];
      let card: HTMLElement | null = null;
      for (const id of idsRef.current) {
        const el = byId.get(id);
        // 渲染与数据未对齐（罕见的中间态）：放弃本次拖拽，等下一帧再来
        if (!el) return;
        if (id === arm.dragId) card = el;
        const rect = el.getBoundingClientRect();
        rects.push({ top: rect.top - containerTop, height: rect.height });
      }
      if (!card) return;
      const dragIndex = idsRef.current.indexOf(arm.dragId);
      const session: DragSession = {
        pointerId: arm.pointerId,
        dragId: arm.dragId,
        dragIndex,
        startY: arm.startY,
        startScrollTop: scroller?.scrollTop ?? 0,
        rects,
        targetIndex: dragIndex,
        card,
        samples: [{ t: performance.now(), y: clientY }],
        latestY: clientY,
        reducedMotion: window.matchMedia("(prefers-reduced-motion: reduce)").matches,
      };
      sessionRef.current = session;
      // 跟手卡片 inline 直写（React 不知道这些 style，收尾负责清除）
      card.style.transition = "none";
      card.style.zIndex = "10";
      setDragId(arm.dragId);
      setShifts(Object.fromEntries(idsRef.current.map((id) => [id, 0])));
      setPhase("drag");
      rafRef.current = requestAnimationFrame(tick);
    },
    [containerRef, tick],
  );

  useEffect(() => {
    const onMove = (event: PointerEvent) => {
      const session = sessionRef.current;
      if (session && session.pointerId === event.pointerId) {
        session.latestY = event.clientY;
        const now = performance.now();
        session.samples.push({ t: now, y: event.clientY });
        while (session.samples.length > 1 && now - session.samples[0].t > VELOCITY_WINDOW_MS) {
          session.samples.shift();
        }
        return;
      }
      const arm = armRef.current;
      if (arm && arm.pointerId === event.pointerId) {
        if (Math.abs(event.clientY - arm.startY) > ACTIVATE_THRESHOLD_PX) {
          beginDrag(arm, event.clientY);
        }
      }
    };
    const onUp = (event: PointerEvent) => {
      if (sessionRef.current?.pointerId === event.pointerId) {
        endDrag(true);
      } else if (armRef.current?.pointerId === event.pointerId) {
        armRef.current = null; // 未超过激活阈值的轻点
      }
    };
    const onCancel = (event: PointerEvent) => {
      if (sessionRef.current?.pointerId === event.pointerId) {
        endDrag(false);
      }
      armRef.current = null;
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && sessionRef.current) {
        event.preventDefault();
        endDrag(false);
      }
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("keydown", onKey, true);
      // 卸载兜底：正在拖拽时中止会话并清直写残留（不 commit）
      cancelAnimationFrame(rafRef.current);
      const session = sessionRef.current;
      if (session) {
        sessionRef.current = null;
        clearCardInline(session.card);
      }
      armRef.current = null;
    };
  }, [beginDrag, clearCardInline, endDrag]);

  const handleProps = useCallback(
    (id: string): DragHandleProps => ({
      onPointerDown: (event) => {
        if (event.button !== 0) return;
        armRef.current = { pointerId: event.pointerId, startY: event.clientY, dragId: id };
        event.currentTarget.setPointerCapture(event.pointerId);
      },
      onKeyDown: (event) => {
        const index = idsRef.current.indexOf(id);
        if (index < 0) return;
        let to: number | null = null;
        if (event.key === "ArrowUp") to = Math.max(0, index - 1);
        else if (event.key === "ArrowDown") to = Math.min(idsRef.current.length - 1, index + 1);
        else if (event.key === "Home") to = 0;
        else if (event.key === "End") to = idsRef.current.length - 1;
        if (to == null || to === index) return;
        event.preventDefault();
        // 键盘调序无直写残留，走普通提交即可
        onCommitRef.current(reorderIds(idsRef.current, index, to));
        // commit 后卡片 DOM 换位，把手元素重建——下一帧找回焦点保持键盘操作连续
        requestAnimationFrame(() => {
          // id 由 newEntryId 生成的 base32 字母数字构成，可安全拼进选择器
          containerRef.current
            ?.querySelector<HTMLElement>(`[data-card-id="${id}"] .qt-drag-handle`)
            ?.focus();
        });
      },
      disabled: idsRef.current.length < 2,
    }),
    [containerRef],
  );

  return { dragId, active: phase !== "idle", shifts, handleProps };
}
