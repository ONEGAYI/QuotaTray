// 试查回退警告 toast（spec #137 T-23 / issue #141）：编辑对话框试查
// 反馈区的回退提示——右对齐、带关闭按钮、不自动消失（用户要对照试查
// 结果逐窗口看，何时关由用户决定，实现内无任何自动隐藏定时器）。
// 回退判定收敛于 display.ts 的 metricFallbackWindows 纯函数（契约测试
// 锁三态矩阵）；auto 或清单空时组件自判不呈现，调用方只喂偏好与窗口数据。
import { useState } from "react";
import { metricFallbackWindows } from "../display";
import { useLang } from "../i18n";
import type { PrimaryMetric, UsageData } from "../types";

export function MetricFallbackToast(props: {
  /** 条目主度量偏好（auto 恒不警告，组件内自判）。 */
  preference: PrimaryMetric;
  /** 试查成功的各窗口数据。 */
  windows: UsageData[];
  onClose: () => void;
}) {
  const { t, lang } = useLang();
  const names = metricFallbackWindows(props.preference, props.windows, lang);
  if (names.length === 0) return null;
  return (
    <div className="qt-fallback-toast-row" role="status">
      <div className="qt-fallback-toast">
        <span>
          {t(
            props.preference === "percent"
              ? "edit.metricFallbackToAmount"
              : "edit.metricFallbackToPercent",
            { windows: names.join(lang === "zh" ? "、" : ", ") },
          )}
        </span>
        <button
          type="button"
          className="qt-fallback-toast-close"
          aria-label={t("edit.metricFallbackClose")}
          onClick={props.onClose}
        >
          ×
        </button>
      </div>
    </div>
  );
}

/** 试查回退 toast 的门控 hook（PR #146 review 抽取，TemplateForm 与
 *  ScriptForm 共用一份）：关闭态由用户点关闭驱动（不自动消失），新一轮
 *  试查调用 reopen 重开（旧结论随旧数据作废）。windows 传试查成功的各
 *  窗口数据（未出结果时传空数组——节点不渲染）。行为与原先两表单各自
 *  内联的 fallbackClosed state + 重置 + 条件 JSX 完全一致。 */
export function useMetricFallbackToast(preference: PrimaryMetric, windows: UsageData[]) {
  const [closed, setClosed] = useState(false);
  return {
    /** 新一轮试查重置关闭态（在 test() 开头调用）。 */
    reopen: () => setClosed(false),
    /** 条件渲染的 toast 节点（已关闭为 null；未关闭时由组件自判清单）。 */
    node: closed ? null : (
      <MetricFallbackToast
        preference={preference}
        windows={windows}
        onClose={() => setClosed(true)}
      />
    ),
  };
}
