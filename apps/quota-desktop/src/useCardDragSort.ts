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
  /** 会话开始时的 id 顺序快照：commit 前核对未变，防并发增删/键盘重排
   *  导致的索引语义错位与越界（对不上即放弃提交，由变更来源恢复秩序）。 */
  idsSnapshot: string[];
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

  // ids/onCommit 在事件回调里取最新值，避免闭包悬挂旧列表；phase 镜像
  // 供原生事件回调读取当前会话阶段（settle/cancel 期间须拒绝新会话）
  const idsRef = useRef(ids);
  idsRef.current = ids;
  const onCommitRef = useRef(options.onCommit);
  onCommitRef.current = options.onCommit;
  const phaseRef = useRef<DragPhase>("idle");
  phaseRef.current = phase;

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
      // 会话期间列表被并发修改：放弃提交（索引语义已错位），由变更来源
      // 自身的渲染/事件恢复真实顺序；清直写与全局复位照常执行
      const idsUnchanged =
        session.idsSnapshot.length === idsRef.current.length &&
        session.idsSnapshot.every((id, index) => id === idsRef.current[index]);
      if (commit && idsUnchanged && session.targetIndex !== session.dragIndex) {
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
      const byId = new Map(
        Array.from(container.querySelectorAll<HTMLElement>("[data-card-id]"), (el) => [el.dataset.cardId ?? "", el]),
      );
      const card = byId.get(arm.dragId);
      const dragIndex = idsRef.current.indexOf(arm.dragId);
      // 渲染与数据未对齐（罕见的中间态）：放弃本次拖拽，等下一帧再来。
      // 全部校验必须先于直写 is-drag-active，否则失败路径不触发任何
      // setState，React 不会重写 className，直写 class 将永久残留。
      if (!card || dragIndex < 0 || !idsRef.current.every((id) => byId.has(id))) return;
      // 测量前直写 is-drag-active：hover 的次级区展开/卡片上浮必须在本帧
      // 同步失效（次级区在拖拽态下无过渡瞬收），否则被拖卡片的展开高度
      // （数百 px）会污染 rects，让位偏移把其他卡片推出卡片区。
      // 后续 React 渲染同值 class 幂等；会话结束时 React 全量替换 className
      // 一并清掉此直写。
      container.classList.add("is-drag-active");
      const scroller = container.closest<HTMLElement>(".qt-main-content");
      scrollerRef.current = scroller;
      const containerTop = container.getBoundingClientRect().top;
      const rects: DragItemRect[] = idsRef.current.map((id) => {
        const rect = byId.get(id)!.getBoundingClientRect();
        return { top: rect.top - containerTop, height: rect.height };
      });
      const session: DragSession = {
        pointerId: arm.pointerId,
        dragId: arm.dragId,
        dragIndex,
        // startY 与 startScrollTop 同取激活时刻（armed 期间列表可能滚动，
        // 混用 pointerdown 纪元会让激活瞬间跳变该滚动量）
        startY: clientY,
        startScrollTop: scroller?.scrollTop ?? 0,
        rects,
        targetIndex: dragIndex,
        card,
        idsSnapshot: [...idsRef.current],
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
      cancelAnimationFrame(rafRef.current);
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
      // 只清本指针的 armed：多指场景下另一指针的 pointercancel 不应误杀
      if (armRef.current?.pointerId === event.pointerId) {
        armRef.current = null;
      }
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

  // 按 id 缓存把手 props：ProviderCard 走 memo，稳定的对象引用让未位移
  // 卡片在让位重渲染中整体跳过；仅单/多卡边界翻转（disabled 变化）时失效
  const handlePropsCacheRef = useRef({ single: false, map: new Map<string, DragHandleProps>() });

  const handleProps = useCallback(
    (id: string): DragHandleProps => {
      const cache = handlePropsCacheRef.current;
      const single = idsRef.current.length < 2;
      if (cache.single !== single) {
        cache.single = single;
        cache.map.clear();
      }
      let props = cache.map.get(id);
      if (!props) {
        props = {
          onPointerDown: (event) => {
            if (event.button !== 0) return;
            // 会话互斥：既有会话（含 settle/cancel 动画期）或另一指针 armed
            // 期间拒绝开启新会话——旧会话 finalize 会复位全局拖拽状态并可能
            // flushSync 重排 DOM，摧毁进行中新会话的几何与直写
            if (sessionRef.current || armRef.current || phaseRef.current !== "idle") return;
            armRef.current = { pointerId: event.pointerId, startY: event.clientY, dragId: id };
            event.currentTarget.setPointerCapture(event.pointerId);
          },
          onKeyDown: (event) => {
            // 拖拽会话进行中忽略键盘调序：idsRef 已非本会话快照，索引错位
            if (sessionRef.current || phaseRef.current !== "idle") return;
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
          disabled: single,
        };
        cache.map.set(id, props);
      }
      return props;
    },
    [containerRef],
  );

  return { dragId, active: phase !== "idle", shifts, handleProps };
}
