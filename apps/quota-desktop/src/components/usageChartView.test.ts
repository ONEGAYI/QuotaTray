import { describe, expect, it } from "vitest";
import {
  addUsageMarker,
  advanceUsageViewDomain,
  buildLineGeometry,
  buildHistorySeries,
  historyPointValues,
  isolatedUsageSamples,
  moveUsageMarker,
  nearestUsageSample,
  niceAbsoluteScale,
  pressUsageMarkerToggle,
  shouldZoomUsageChart,
  snapUsageMarkerTimestamp,
  splitUsageSeries,
  usageMarkerBurnRate,
  usageMarkerNet,
  usageMarkerNetBreakdown,
  usageMarkerPeakBurn,
  USAGE_MARKER_LIMIT,
  USAGE_RANGES,
  USAGE_TOOLTIP_GAP,
  usageSmoothingRadius,
  usageTooltipPlacement,
  type UsageSample,
} from "./usageChartView";

const MINUTE = 60 * 1_000;
const HOUR = 60 * MINUTE;

function point(hour: number, value: number): UsageSample {
  return { timestamp: hour * HOUR, value };
}

describe("使用统计图表纯逻辑", () => {
  it("连续数据拟合为同一段，短缺失用虚线桥接，长缺失完全断开", () => {
    const series = splitUsageSeries(
      [point(0, 10), point(1, 14), point(4, 22), point(12, 38), point(13, 42)],
      HOUR,
    );

    expect(series.segments.map((segment) => segment.map((sample) => sample.timestamp / HOUR)))
      .toEqual([[0, 1], [4], [12, 13]]);
    expect(series.bridges).toEqual([
      { from: point(1, 14), to: point(4, 22), missingBuckets: 2 },
    ]);
    expect(series.gaps).toEqual([
      { from: point(4, 22), to: point(12, 38), missingBuckets: 7 },
    ]);
  });

  it("绝对值轴从零开始并生成覆盖最大值的整洁刻度", () => {
    expect(niceAbsoluteScale([12, 88, 131], 4)).toEqual({
      min: 0,
      max: 200,
      ticks: [0, 50, 100, 150, 200],
    });
    expect(niceAbsoluteScale([], 4)).toEqual({
      min: 0,
      max: 100,
      ticks: [0, 25, 50, 75, 100],
    });
  });

  it("连续段输出单调三次曲线路径，单点段保留为可悬停数据点", () => {
    const geometry = buildLineGeometry(
      [point(0, 10), point(1, 20), point(2, 15)],
      (timestamp) => timestamp / HOUR * 100,
      (value) => 100 - value,
    );

    expect(geometry.path).toMatch(/^M 0 90 C /);
    expect(geometry.path).not.toContain("NaN");
    expect(geometry.points).toEqual([
      { x: 0, y: 90, sample: point(0, 10) },
      { x: 100, y: 80, sample: point(1, 20) },
      { x: 200, y: 85, sample: point(2, 15) },
    ]);

    expect(buildLineGeometry([point(3, 7)], () => 18, () => 24).path).toBe("");
  });

  it("平台两端采用轻度局部平滑，但悬浮点仍保留真实值", () => {
    const geometry = buildLineGeometry(
      [point(0, 100), point(1, 80), point(2, 80), point(3, 80), point(4, 60)],
      (timestamp) => timestamp / HOUR * 100,
      (value) => value,
    );

    expect(geometry.points.map((item) => item.y)).toEqual([100, 80, 80, 80, 60]);
    expect(geometry.curvePoints[1].y).toBeGreaterThan(80);
    expect(geometry.curvePoints[2].y).toBeCloseTo(80);
    expect(geometry.curvePoints[3].y).toBeLessThan(80);
  });

  it("轻度平滑不跨越额度重置，重置段保持真实跳变", () => {
    const geometry = buildLineGeometry(
      [point(0, 80), point(1, 60), point(2, 100), point(3, 90)],
      (timestamp) => timestamp / HOUR * 100,
      (value) => 100 - value,
    );

    expect(geometry.path).toContain(" L 200 0");
    expect(geometry.curvePoints.map((item) => item.y)).toEqual([20, 40, 0, 10]);
  });

  it("仅 15 分钟及以下的细粒度桶启用轻度平滑，1 小时桶保留平台", () => {
    expect(usageSmoothingRadius(15 * MINUTE)).toBe(2);
    expect(usageSmoothingRadius(15 * MINUTE + 1)).toBe(0);
    expect(usageSmoothingRadius(HOUR)).toBe(0);

    const samples = [point(0, 100), point(1, 80), point(2, 80), point(3, 80), point(4, 60)];
    const geometry = buildLineGeometry(
      samples,
      (timestamp) => timestamp / HOUR * 100,
      (value) => value,
      { smoothingRadius: usageSmoothingRadius(HOUR) },
    );

    expect(geometry.curvePoints.map((item) => item.y)).toEqual([100, 80, 80, 80, 60]);
  });

  it("孤立单点从断线分段中单独提取用于圆点渲染", () => {
    const split = splitUsageSeries([point(0, 10), point(8, 20), point(9, 30)], HOUR);
    expect(isolatedUsageSamples(split)).toEqual([point(0, 10)]);
  });

  it("普通滚轮和非绘图区 Ctrl+滚轮留给页面，仅绘图区 Ctrl+滚轮缩放", () => {
    expect(shouldZoomUsageChart({ ctrlKey: false }, true)).toBe(false);
    expect(shouldZoomUsageChart({ ctrlKey: true }, false)).toBe(false);
    expect(shouldZoomUsageChart({ ctrlKey: true }, true)).toBe(true);
  });

  it("点位提示卡片锚定数据点：上半区翻到点下方，下半区翻到点上方，水平钳制在安全区", () => {
    const upper = usageTooltipPlacement({ x: 420, y: 82 }, 840, 410);
    expect(upper.below).toBe(true);
    expect(upper.topPct).toBeCloseTo(((82 + USAGE_TOOLTIP_GAP) / 410) * 100);
    expect(upper.leftPct).toBeCloseTo(50);

    const lower = usageTooltipPlacement({ x: 420, y: 306 }, 840, 410);
    expect(lower.below).toBe(false);
    expect(lower.topPct).toBeCloseTo(((306 - USAGE_TOOLTIP_GAP) / 410) * 100);

    expect(usageTooltipPlacement({ x: 40, y: 205 }, 840, 410).leftPct).toBe(10);
    expect(usageTooltipPlacement({ x: 900, y: 205 }, 840, 410).leftPct).toBe(82);
  });

  it("时间窗前进时跟随实时边缘，同时保留用户正在查看的历史区间", () => {
    const previousTotal: [number, number] = [0, 100];
    const nextTotal: [number, number] = [10, 110];

    expect(advanceUsageViewDomain([50, 100], previousTotal, nextTotal)).toEqual([60, 110]);
    expect(advanceUsageViewDomain([20, 60], previousTotal, nextTotal)).toEqual([20, 60]);
    expect(advanceUsageViewDomain([0, 30], previousTotal, nextTotal)).toEqual([10, 40]);
  });

  it("真实历史点按 Scope 与小时桶分组，桶内保留最后一点", () => {
    const points = [
      { window_key: "Codex（5h）", sampled_at: 0, used: 10, remaining: 90, total: 100, unit: "%" },
      { window_key: "Codex（5h）", sampled_at: HOUR / 2, used: 18, remaining: 82, total: 100, unit: "%" },
      { window_key: "Codex（5h）", sampled_at: HOUR, used: 24, remaining: 76, total: 100, unit: "%" },
      { window_key: "DeepSeek", sampled_at: 0, remaining: 61.5, unit: "CNY" },
      { window_key: "empty", sampled_at: 0 },
    ];

    expect(buildHistorySeries(points, HOUR)).toEqual([
      {
        windowKey: "Codex（5h）",
        metric: "percent",
        unit: "%",
        quantity: "remaining",
        samples: [point(0.5, 82), point(1, 76)],
      },
      {
        windowKey: "DeepSeek",
        metric: "absolute",
        unit: "CNY",
        quantity: "remaining",
        samples: [point(0, 61.5)],
      },
    ]);
  });

  it("曲线值方向显式化：百分比恒为剩余量，仅配 used 的模板为已用量", () => {
    // total + used（无 remaining）：百分比轨由 used 换算、金额轨为已用量
    expect(historyPointValues({
      window_key: "credits",
      sampled_at: 0,
      used: 25,
      total: 200,
      unit: "credits",
    })).toEqual({
      percent: { metric: "percent", value: 87.5, unit: "%", quantity: "remaining" },
      absolute: { metric: "absolute", value: 25, unit: "credits", quantity: "used" },
    });
    // unit "%"：仅百分比轨（无金额原材料），used 换算为剩余
    expect(historyPointValues({
      window_key: "percent",
      sampled_at: 0,
      used: 25,
      unit: "%",
    })).toEqual({
      percent: { metric: "percent", value: 75, unit: "%", quantity: "remaining" },
      absolute: null,
    });
    // 仅 remaining（无 total、非 % 单位）：仅金额轨
    expect(historyPointValues({
      window_key: "balance",
      sampled_at: 0,
      used: 4,
      remaining: 96,
      unit: "CNY",
    })).toEqual({
      percent: null,
      absolute: { metric: "absolute", value: 96, unit: "CNY", quantity: "remaining" },
    });
    // 仅配 used 的模板（used 独立可选，无 remaining/total）：值为已用量，
    // 方向与剩余量相反——净消耗/速率必须按此方向补偿（issue #135）
    expect(historyPointValues({
      window_key: "used-only",
      sampled_at: 0,
      used: 5,
      unit: "credits",
    })).toEqual({
      percent: null,
      absolute: { metric: "absolute", value: 5, unit: "credits", quantity: "used" },
    });
    expect(buildHistorySeries(
      [{ window_key: "used-only", sampled_at: 0, used: 5, unit: "credits" }],
      HOUR,
    )).toEqual([{
      windowKey: "used-only",
      metric: "absolute",
      unit: "credits",
      quantity: "used",
      samples: [point(0, 5)],
    }]);
  });

  it("同窗口双产：百分比与金额原材料齐备时产出两条候选，各自成轨", () => {
    const points = [
      { window_key: "GLM", sampled_at: 0, used: 30, remaining: 70, total: 100, unit: "CNY" },
      { window_key: "GLM", sampled_at: HOUR, used: 40, remaining: 60, total: 100, unit: "CNY" },
    ];

    expect(buildHistorySeries(points, HOUR)).toEqual([
      {
        windowKey: "GLM",
        metric: "percent",
        quantity: "remaining",
        unit: "%",
        samples: [point(0, 70), point(1, 60)],
      },
      {
        windowKey: "GLM",
        metric: "absolute",
        quantity: "remaining",
        unit: "CNY",
        samples: [point(0, 70), point(1, 60)],
      },
    ]);
  });

  it("双产轨各自保持方向过滤：金额轨样本方向漂移时丢弃旧方向样本", () => {
    // 模板从仅配 used 改为提供 remaining：金额轨旧样本是已用量语义，
    // 与剩余量混排会使速率/净消耗方向错乱，按最新样本方向过滤（#135）；
    // 百分比轨恒剩余方向，不受影响
    const points = [
      { window_key: "w", sampled_at: 0, used: 5, total: 100, unit: "credits" },
      { window_key: "w", sampled_at: HOUR, used: 8, remaining: 92, total: 100, unit: "credits" },
    ];

    expect(buildHistorySeries(points, HOUR)).toEqual([
      {
        windowKey: "w",
        metric: "percent",
        quantity: "remaining",
        unit: "%",
        samples: [point(0, 95), point(1, 92)],
      },
      {
        windowKey: "w",
        metric: "absolute",
        quantity: "remaining",
        unit: "credits",
        samples: [point(1, 92)],
      },
    ]);
  });

  it("视图范围档位锁定：24h 档 15 分钟桶、7d 档 1 小时桶，桶粒度整除跨度", () => {
    expect(Object.keys(USAGE_RANGES)).toEqual(["24h", "7d"]);
    expect(USAGE_RANGES["24h"]).toEqual({ spanMs: 24 * HOUR, bucketMs: 15 * MINUTE });
    expect(USAGE_RANGES["7d"]).toEqual({ spanMs: 7 * 24 * HOUR, bucketMs: HOUR });
    for (const config of Object.values(USAGE_RANGES)) {
      expect(config.spanMs % config.bucketMs).toBe(0);
    }
  });

  it("近 24 小时档按 15 分钟桶聚合：桶内保留最后一点，跨桶均保留", () => {
    const points = [
      { window_key: "Codex（5h）", sampled_at: 0, used: 10, remaining: 90, total: 100, unit: "%" },
      { window_key: "Codex（5h）", sampled_at: 10 * MINUTE, used: 16, remaining: 84, total: 100, unit: "%" },
      { window_key: "Codex（5h）", sampled_at: 15 * MINUTE, used: 24, remaining: 76, total: 100, unit: "%" },
    ];

    expect(buildHistorySeries(points, USAGE_RANGES["24h"].bucketMs)).toEqual([
      {
        windowKey: "Codex（5h）",
        metric: "percent",
        unit: "%",
        quantity: "remaining",
        samples: [
          { timestamp: 10 * MINUTE, value: 84 },
          { timestamp: 15 * MINUTE, value: 76 },
        ],
      },
    ]);
  });

  it("15 分钟桶下空档阈值收紧：90 分钟仍虚线桥接，约 100 分钟起完全断开", () => {
    const series = splitUsageSeries(
      [
        { timestamp: 0, value: 10 },
        { timestamp: 60 * MINUTE, value: 14 },
        { timestamp: 150 * MINUTE, value: 20 },
        { timestamp: 160 * MINUTE, value: 24 },
        { timestamp: 260 * MINUTE, value: 30 },
      ],
      USAGE_RANGES["24h"].bucketMs,
    );

    expect(series.segments.map((segment) => segment.map((sample) => sample.timestamp)))
      .toEqual([[0], [60 * MINUTE], [150 * MINUTE, 160 * MINUTE], [260 * MINUTE]]);
    expect(series.bridges).toEqual([
      { from: { timestamp: 0, value: 10 }, to: { timestamp: 60 * MINUTE, value: 14 }, missingBuckets: 3 },
      { from: { timestamp: 60 * MINUTE, value: 14 }, to: { timestamp: 150 * MINUTE, value: 20 }, missingBuckets: 5 },
    ]);
    expect(series.gaps).toEqual([
      { from: { timestamp: 160 * MINUTE, value: 24 }, to: { timestamp: 260 * MINUTE, value: 30 }, missingBuckets: 6 },
    ]);
  });

  it("定位线放置：追加新时间戳，重复幂等，满两条后丢最旧", () => {
    expect(USAGE_MARKER_LIMIT).toBe(2);
    expect(addUsageMarker([], 100)).toEqual([100]);
    expect(addUsageMarker([100], 200)).toEqual([100, 200]);
    expect(addUsageMarker([100, 200], 200)).toEqual([100, 200]);
    expect(addUsageMarker([100, 200], 300)).toEqual([200, 300]);
  });

  it("定位线拖动微调：更新自身时刻，与另一条重合或未移动时原地不动", () => {
    expect(moveUsageMarker([100, 300], 300, 200)).toEqual([100, 200]);
    expect(moveUsageMarker([100, 300], 300, 100)).toEqual([100, 300]);
    expect(moveUsageMarker([100, 300], 300, 300)).toEqual([100, 300]);
  });

  it("定位线按钮：模式中点击退出且保留已放置的定位线", () => {
    expect(pressUsageMarkerToggle(true, [100, 300])).toEqual({ mode: false, markers: [100, 300], cleared: false });
  });

  it("定位线按钮：有空缺时点击进入放置模式直接补位，已有定位线原样保留", () => {
    expect(pressUsageMarkerToggle(false, [])).toEqual({ mode: true, markers: [], cleared: false });
    expect(pressUsageMarkerToggle(false, [100])).toEqual({ mode: true, markers: [100], cleared: false });
  });

  it("定位线按钮：满两条后再点清空两条并从头定位", () => {
    expect(pressUsageMarkerToggle(false, [100, 300])).toEqual({ mode: true, markers: [], cleared: true });
    // 超限输入（正常交互不可达）走同一清空路径，纯函数防御分支
    expect(pressUsageMarkerToggle(false, [100, 200, 300])).toEqual({ mode: true, markers: [], cleared: true });
  });

  it("定位线按钮：保留路径返回副本，不改动入参数组", () => {
    const markers = [100];
    const outcome = pressUsageMarkerToggle(false, markers);
    expect(outcome.markers).not.toBe(markers);
    expect(markers).toEqual([100]);
  });

  it("定位线吸附：容差内吸附最近样本，容差外与空样本保留原始时刻", () => {
    const samples = [point(0, 10), point(2, 20)];
    expect(snapUsageMarkerTimestamp(0.6 * HOUR, samples, HOUR)).toBe(0);
    expect(snapUsageMarkerTimestamp(1.2 * HOUR, samples, HOUR)).toBe(2 * HOUR);
    // 等距时吸附先遍历到的样本（序列按时间升序输入，即较早的样本）
    expect(snapUsageMarkerTimestamp(HOUR, samples, HOUR)).toBe(0);
    expect(snapUsageMarkerTimestamp(4 * HOUR, samples, HOUR)).toBe(4 * HOUR);
    expect(snapUsageMarkerTimestamp(3 * HOUR, [], HOUR)).toBe(3 * HOUR);
  });

  it("定位线时刻恒为整数毫秒：未吸附时归整坐标换算的小数时刻（持久化契约）", () => {
    // 真机故障（2026-09-07）：图表坐标换算得的时刻带小数毫秒，吸附命中时
    // 取到整数样本时刻侥幸可用；空采集区域不吸附时浮点时刻进入
    // usage_marker_lines，后端 Vec<u64> 反序列化失败 → 保存回退 → 放不上。
    // snap 是放置与拖动的唯一时刻出口，在此处归整（真机复现：空区域
    // tap 后读数行回到空态；样本区域 tap 可放置）。
    const fractional = 3.7 * HOUR + 0.25; // 13320000.25
    expect(Number.isInteger(fractional)).toBe(false);
    expect(snapUsageMarkerTimestamp(fractional, [], HOUR)).toBe(13_320_000);
    // 容差外有样本同样归整保留（不走样本时刻）
    expect(snapUsageMarkerTimestamp(fractional, [point(0, 10)], HOUR)).toBe(13_320_000);
    // 吸附路径本就返回整数样本时刻，不受影响
    expect(snapUsageMarkerTimestamp(0.4 * HOUR, [point(0, 10)], HOUR)).toBe(0);
  });

  it("定位线平均消耗速率：剩余量下降换算每小时，按 marker 时间差取值样本", () => {
    const scope = { samples: [point(0, 31), point(8, 4)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerBurnRate(scope, [0, 8 * HOUR])).toBeCloseTo(3.375);
    // 值取自容差内最近样本，时间差按 marker 时刻（0.2h 处吸附 0h 样本）
    const drifted = { samples: [point(0, 10), point(4, 20)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerBurnRate(drifted, [0.2 * HOUR, 4 * HOUR])).toBeCloseTo(-10 / 3.8);
    // markers 乱序传入（拖动交叉后的真实形态）与升序同结果，且不突变入参
    const shuffled = [8 * HOUR, 0];
    expect(usageMarkerBurnRate(scope, shuffled)).toBeCloseTo(3.375);
    expect(shuffled).toEqual([8 * HOUR, 0]);
  });

  it("定位线平均消耗速率：回升为负值，余额序列同样适用", () => {
    const refill = { samples: [point(0, 10), point(2, 16)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerBurnRate(refill, [0, 2 * HOUR])).toBeCloseTo(-3);
    const balance = { samples: [point(0, 100), point(4, 86)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerBurnRate(balance, [0, 4 * HOUR])).toBeCloseTo(3.5);
  });

  it("定位线平均消耗速率：样本缺失或时间差不足一分钟时无可测值", () => {
    const scope = { samples: [point(0, 31), point(8, 4)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerBurnRate(scope, [0, 20 * HOUR])).toBeNull();
    expect(usageMarkerBurnRate({ samples: [], bucketMs: HOUR, quantity: "remaining" }, [0, HOUR])).toBeNull();
    expect(usageMarkerBurnRate(scope, [0])).toBeNull();
    expect(usageMarkerBurnRate(scope, [4 * HOUR, 4 * HOUR])).toBeNull();
    // 时间差不足 1 分钟：与时间差文案「至少 1 分钟」口径对齐，不显示速率
    expect(usageMarkerBurnRate(scope, [0, 30_000])).toBeNull();
  });

  it("定位线平均消耗速率：两条线落在同一样本容差内时速率为零（合法读数）", () => {
    expect(usageMarkerBurnRate({ samples: [point(0, 10)], bucketMs: HOUR, quantity: "remaining" }, [0.2 * HOUR, 0.8 * HOUR])).toBe(0);
  });

  it("定位线峰值消耗：区间内相邻段取最陡消耗段，乱序 markers 同结果", () => {
    const scope = {
      samples: [point(0, 100), point(1, 96), point(2, 80), point(3, 92), point(4, 90), point(5, 50)],
      bucketMs: HOUR,
      quantity: "remaining" as const,
    };
    // markers [1h,4h] → 端点样本 96/90，区间段斜率 16/h、-12/h（回升）、2/h；
    // 区间外段（90→50 = 40/h）更陡但必须排除
    expect(usageMarkerPeakBurn(scope, [HOUR, 4 * HOUR]))
      .toEqual({ ratePerHour: 16, from: point(1, 96), to: point(2, 80) });
    // markers 乱序传入（拖动交叉后的真实形态）与升序同结果，且不突变入参
    const shuffled = [4 * HOUR, HOUR];
    expect(usageMarkerPeakBurn(scope, shuffled)).toEqual({ ratePerHour: 16, from: point(1, 96), to: point(2, 80) });
    expect(shuffled).toEqual([4 * HOUR, HOUR]);
  });

  it("定位线峰值消耗：回升与平段不算消耗极值，区间无消耗段时为空", () => {
    // 纯回升（充值/额度重置是瞬间跳变，跨桶斜率无测量意义，不展示）与
    // 全平区间均无可测消耗段
    const recoveryOnly = { samples: [point(0, 80), point(1, 92), point(2, 94)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerPeakBurn(recoveryOnly, [0, 2 * HOUR])).toBeNull();
    const flat = { samples: [point(0, 50), point(1, 50), point(2, 50)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerPeakBurn(flat, [0, 2 * HOUR])).toBeNull();
  });

  it("定位线峰值消耗：斜率相同取较早段，非均匀间隔按真实时长归一", () => {
    const tie = { samples: [point(0, 100), point(1, 90), point(2, 80)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerPeakBurn(tie, [0, 2 * HOUR]))
      .toEqual({ ratePerHour: 10, from: point(0, 100), to: point(1, 90) });
    // 相邻样本间隔 2.5 小时：斜率按真实 Δt 归一（20 / 2.5 = 8/h）
    const sparse = { samples: [point(0, 100), point(2.5, 80)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerPeakBurn(sparse, [0.2 * HOUR, 2.5 * HOUR]))
      .toEqual({ ratePerHour: 8, from: point(0, 100), to: point(2.5, 80) });
  });

  it("定位线峰值消耗：无可测区间或含不可测段时返回空", () => {
    const scope = { samples: [point(0, 31), point(8, 4)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerPeakBurn(scope, [0])).toBeNull();
    // 20h 处无容差内样本（端点缺失）
    expect(usageMarkerPeakBurn(scope, [0, 20 * HOUR])).toBeNull();
    expect(usageMarkerPeakBurn({ samples: [], bucketMs: HOUR, quantity: "remaining" }, [0, HOUR])).toBeNull();
    // 时间差不足 1 分钟：与平均速率口径对齐
    expect(usageMarkerPeakBurn(scope, [0, 30_000])).toBeNull();
    // 两条线落在同一样本容差内：区间仅单样本，无段可测
    expect(usageMarkerPeakBurn({ samples: [point(0, 10)], bucketMs: HOUR, quantity: "remaining" }, [0.2 * HOUR, 0.8 * HOUR])).toBeNull();
    // 重复时间戳样本（Δt=0 防御）：零时长段必须跳过而非算出无穷斜率
    const duplicated = { samples: [point(0, 100), point(0, 60), point(1, 55)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerPeakBurn(duplicated, [0, HOUR]))
      .toEqual({ ratePerHour: 5, from: point(0, 60), to: point(1, 55) });
  });

  it("容差内最近样本取值：等距取先遍历到的较早样本，恰一个桶宽仍命中，容差外为空", () => {
    const samples = [point(0, 10), point(2, 20)];
    expect(nearestUsageSample(samples, 0.6 * HOUR, HOUR)).toEqual(point(0, 10));
    expect(nearestUsageSample(samples, 1.2 * HOUR, HOUR)).toEqual(point(2, 20));
    // 等距中点取较早样本（与 snapUsageMarkerTimestamp 口径一致）
    expect(nearestUsageSample(samples, HOUR, HOUR)).toEqual(point(0, 10));
    // 恰好一个桶宽命中（<= 边界），超出即无值
    expect(nearestUsageSample([point(0, 10)], HOUR, HOUR)).toEqual(point(0, 10));
    expect(nearestUsageSample([point(0, 10)], HOUR + 1, HOUR)).toBeNull();
    expect(nearestUsageSample([], HOUR, HOUR)).toBeNull();
  });

  it("已用量曲线方向补偿（#135 回归）：已用量上升为正消耗，下降为回升", () => {
    // 仅配 used 的模板曲线值为已用量：上升为消耗、下降为回升，
    // 与剩余量方向相反——速率/峰值历史上未补偿（代码注释自知），同票修正
    const usedScope = {
      samples: [point(0, 20), point(2, 30), point(4, 25), point(6, 45)],
      bucketMs: HOUR,
      quantity: "used" as const,
    };
    expect(usageMarkerBurnRate(usedScope, [0, 6 * HOUR])).toBeCloseTo(25 / 6);
    expect(usageMarkerBurnRate(usedScope, [2 * HOUR, 6 * HOUR])).toBeCloseTo(15 / 4);
    // 峰值消耗段取已用量最陡上升段（25→45 = 20/2h；30→25 是回升不参与）
    expect(usageMarkerPeakBurn(usedScope, [0, 6 * HOUR]))
      .toEqual({ ratePerHour: 10, from: point(4, 25), to: point(6, 45) });
    // 纯下降（已用量减少 = 回升）无消耗段，速率为负
    const declining = {
      samples: [point(0, 80), point(1, 60), point(2, 50)],
      bucketMs: HOUR,
      quantity: "used" as const,
    };
    expect(usageMarkerBurnRate(declining, [0, 2 * HOUR])).toBeCloseTo(-15);
    expect(usageMarkerPeakBurn(declining, [0, 2 * HOUR])).toBeNull();
  });

  it("定位线净消耗：端点净变化，剩余量曲线下降为正、负值为净恢复、零合法", () => {
    const scope = {
      samples: [point(0, 100), point(2, 70), point(4, 60)],
      bucketMs: HOUR,
      quantity: "remaining" as const,
    };
    expect(usageMarkerNet(scope, [0, 4 * HOUR])).toBe(40);
    expect(usageMarkerNet(scope, [2 * HOUR, 4 * HOUR])).toBe(10);
    // 净恢复为负
    const refill = { samples: [point(0, 50), point(3, 65)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerNet(refill, [0, 3 * HOUR])).toBe(-15);
    // 两条线落在同一样本容差内：净消耗为零（与平均速率同口径的合法读数）
    expect(usageMarkerNet({ samples: [point(0, 10)], bucketMs: HOUR, quantity: "remaining" }, [0.2 * HOUR, 0.8 * HOUR])).toBe(0);
  });

  it("定位线净消耗：已用量曲线按已用量方向补偿", () => {
    const usedScope = { samples: [point(0, 20), point(5, 50)], bucketMs: HOUR, quantity: "used" as const };
    expect(usageMarkerNet(usedScope, [0, 5 * HOUR])).toBe(30);
    const declined = { samples: [point(0, 50), point(5, 20)], bucketMs: HOUR, quantity: "used" as const };
    expect(usageMarkerNet(declined, [0, 5 * HOUR])).toBe(-30);
  });

  it("定位线净消耗：无可测值守卫与平均消耗速率一致", () => {
    const scope = { samples: [point(0, 31), point(8, 4)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerNet(scope, [0])).toBeNull();
    expect(usageMarkerNet(scope, [4 * HOUR, 4 * HOUR])).toBeNull();
    expect(usageMarkerNet(scope, [0, 30_000])).toBeNull();
    expect(usageMarkerNet(scope, [0, 20 * HOUR])).toBeNull();
    expect(usageMarkerNet({ samples: [], bucketMs: HOUR, quantity: "remaining" }, [0, HOUR])).toBeNull();
  });

  it("净消耗分解：连续段累计消耗与恢复，代数关系恒成立（纯消耗场景）", () => {
    const scope = {
      samples: [point(0, 100), point(1, 90), point(2, 75), point(3, 70)],
      bucketMs: HOUR,
      quantity: "remaining" as const,
    };
    const breakdown = usageMarkerNetBreakdown(scope, [0, 3 * HOUR]);
    expect(breakdown).toEqual({ observedBurn: 30, observedRecovery: 0, unobservedNet: 0, net: 30 });
  });

  it("净消耗分解：多次消耗/恢复交替与额度重置，消耗恢复分开累计", () => {
    // 额度重置表现为剩余量跳升（恢复），与普通回升同样按恢复累计
    const scope = {
      samples: [point(0, 100), point(1, 80), point(2, 95), point(3, 60), point(4, 90), point(5, 70)],
      bucketMs: HOUR,
      quantity: "remaining" as const,
    };
    const breakdown = usageMarkerNetBreakdown(scope, [0, 5 * HOUR]);
    expect(breakdown?.observedBurn).toBeCloseTo(75);
    expect(breakdown?.observedRecovery).toBeCloseTo(45);
    expect(breakdown?.unobservedNet).toBeCloseTo(0);
    expect(breakdown?.net).toBeCloseTo(30);
    // 已观测消耗 − 已观测恢复 + 未观测净变化 = 端点净消耗
    expect(breakdown!.observedBurn - breakdown!.observedRecovery + breakdown!.unobservedNet).toBeCloseTo(breakdown!.net);
  });

  it("净消耗分解：长断档不推断，变化计入带符号未观测净变化", () => {
    // 0→1 连续消耗 10；1→10 缺 8 桶完全断开；10→11 连续消耗 5
    const scope = {
      samples: [point(0, 100), point(1, 90), point(10, 60), point(11, 55)],
      bucketMs: HOUR,
      quantity: "remaining" as const,
    };
    const breakdown = usageMarkerNetBreakdown(scope, [0, 11 * HOUR]);
    expect(breakdown?.observedBurn).toBeCloseTo(15);
    expect(breakdown?.observedRecovery).toBeCloseTo(0);
    expect(breakdown?.unobservedNet).toBeCloseTo(30);
    expect(breakdown?.net).toBeCloseTo(45);
    expect(breakdown!.observedBurn - breakdown!.observedRecovery + breakdown!.unobservedNet).toBeCloseTo(breakdown!.net);
  });

  it("净消耗分解：虚线桥（缺 2-5 桶）同样不可观察，仅缺 1 桶（桶距 2）算连续", () => {
    // 与 splitUsageSeries 的 segments 边界对齐：视觉实线段即已观测，
    // 桥与断档一律入未观测
    const bridged = { samples: [point(0, 100), point(4, 90)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerNetBreakdown(bridged, [0, 4 * HOUR])).toEqual({
      observedBurn: 0, observedRecovery: 0, unobservedNet: 10, net: 10,
    });
    const continuous = { samples: [point(0, 100), point(2, 90)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerNetBreakdown(continuous, [0, 2 * HOUR])).toEqual({
      observedBurn: 10, observedRecovery: 0, unobservedNet: 0, net: 10,
    });
  });

  it("净消耗分解：已用量曲线按已用量方向拆分，断档同样入未观测", () => {
    const scope = {
      samples: [point(0, 20), point(1, 30), point(2, 25), point(8, 50)],
      bucketMs: HOUR,
      quantity: "used" as const,
    };
    const breakdown = usageMarkerNetBreakdown(scope, [0, 8 * HOUR]);
    expect(breakdown?.observedBurn).toBeCloseTo(10);
    expect(breakdown?.observedRecovery).toBeCloseTo(5);
    expect(breakdown?.unobservedNet).toBeCloseTo(25);
    expect(breakdown?.net).toBeCloseTo(30);
    expect(breakdown!.observedBurn - breakdown!.observedRecovery + breakdown!.unobservedNet).toBeCloseTo(breakdown!.net);
  });

  it("净消耗分解：百分比与金额曲线同口径，净恢复场景代数关系成立", () => {
    const percentScope = {
      samples: [point(0, 100), point(1, 60), point(2, 68)],
      bucketMs: HOUR,
      quantity: "remaining" as const,
    };
    expect(usageMarkerNetBreakdown(percentScope, [0, 2 * HOUR])).toEqual({
      observedBurn: 40, observedRecovery: 8, unobservedNet: 0, net: 32,
    });
    const balanceScope = {
      samples: [point(0, 61.5), point(1, 50.25), point(5, 80)],
      bucketMs: HOUR,
      quantity: "remaining" as const,
    };
    const breakdown = usageMarkerNetBreakdown(balanceScope, [0, 5 * HOUR]);
    expect(breakdown?.observedBurn).toBeCloseTo(11.25);
    expect(breakdown?.observedRecovery).toBeCloseTo(0);
    // 50.25→80 跨 3 个缺失桶（虚线桥）：剩余量上升是恢复方向，带符号入未观测
    expect(breakdown?.unobservedNet).toBeCloseTo(-29.75);
    expect(breakdown!.observedBurn - breakdown!.observedRecovery + breakdown!.unobservedNet).toBeCloseTo(breakdown!.net);
    expect(breakdown!.net).toBeCloseTo(61.5 - 80);
  });

  it("净消耗分解：无可测值守卫与净消耗主数值一致", () => {
    const scope = { samples: [point(0, 31), point(8, 4)], bucketMs: HOUR, quantity: "remaining" as const };
    expect(usageMarkerNetBreakdown(scope, [0])).toBeNull();
    expect(usageMarkerNetBreakdown(scope, [4 * HOUR, 4 * HOUR])).toBeNull();
    expect(usageMarkerNetBreakdown(scope, [0, 30_000])).toBeNull();
    expect(usageMarkerNetBreakdown(scope, [0, 20 * HOUR])).toBeNull();
    expect(usageMarkerNetBreakdown({ samples: [], bucketMs: HOUR, quantity: "remaining" }, [0, HOUR])).toBeNull();
    // 两条线落在同一样本容差内：区间仅单样本，全零且代数关系成立
    expect(usageMarkerNetBreakdown({ samples: [point(0, 10)], bucketMs: HOUR, quantity: "remaining" }, [0.2 * HOUR, 0.8 * HOUR]))
      .toEqual({ observedBurn: 0, observedRecovery: 0, unobservedNet: 0, net: 0 });
  });

  it("净消耗分解：断档内含额度重置的复合场景，代数关系仍成立且不伪称已知", () => {
    // 连续消耗 20 → 长断档（期间可能发生重置+再消耗，无法拆分）→ 连续恢复 3 → 连续消耗 6；
    // 断档两端 80→62 的净降 18 只能记为「未观测净变化 +18」，不伪称已知消耗
    const scope = {
      samples: [point(0, 100), point(1, 80), point(9, 62), point(10, 65), point(11, 59)],
      bucketMs: HOUR,
      quantity: "remaining" as const,
    };
    const breakdown = usageMarkerNetBreakdown(scope, [0, 11 * HOUR]);
    expect(breakdown?.observedBurn).toBeCloseTo(26);
    expect(breakdown?.observedRecovery).toBeCloseTo(3);
    expect(breakdown?.unobservedNet).toBeCloseTo(18);
    expect(breakdown?.net).toBeCloseTo(41);
    expect(breakdown!.observedBurn - breakdown!.observedRecovery + breakdown!.unobservedNet).toBeCloseTo(breakdown!.net);
  });
});
