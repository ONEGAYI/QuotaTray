/** 供应商头像：统一渲染 + 可选的悬浮编辑入口。
 *
 * 用于卡片（42px）与将来其他需要「可编辑头像」的位置。悬停面板的
 * `AccountAvatar` 不走本组件：它要在图标外套一圈进度环，结构与卡片不同，
 * 但**展示形态判定共用** `avatarView.resolveAvatar`（避免两套 is-custom 分叉）。
 *
 * 交互（2026-11 定案）：
 * - 桌面：hover/focus 时头像底层变灰并浮出齿轮，点齿轮开设置弹窗
 * - Android：无 hover，改为长按头像直接开弹窗
 *
 * 可编辑时外层是 `<button>`（键盘与触屏可达），内层 chip + 遮罩 + 齿轮
 * 浮层；不传 onEdit 时退化为纯展示——只渲染内层 chip，不带遮罩与齿轮。
 * 注意不能把 button 嵌进 button：悬停面板的列表行本身是 button。
 */

import { Settings } from "lucide-react";
import { useEffect, useRef } from "react";
import { useLang } from "../i18n";
import { resolveAvatar } from "./avatarView";
import type { AvatarSource } from "./avatarView";
import { providerInitials } from "./providerIcon";

/** 长按触发阈值（ms）：低于此值的按压按普通点击处理（Android 用）。 */
const LONG_PRESS_MS = 500;

export interface ProviderAvatarProps {
  /** 头像覆盖值（`brand:<slug>` / data URL / 缺省）。 */
  avatar?: string | null;
  /** native 平台 id；模板/脚本条目传 null。 */
  nativeId: string | null;
  /** 条目名（模板图名启发用）。 */
  entryName: string;
  /** 平台显示名（首字母回退用）；缺省回退到 entryName。
   *  与悬停面板同口径——同一条目在两处显示同一个首字母。 */
  platformName?: string;
  /** 点击齿轮/长按头像时调用；不传 = 纯展示（不显示齿轮）。 */
  onEdit?: () => void;
  /** 是否触屏形态（长按代替 hover）。 */
  mobile?: boolean;
  /** 外层 class（尺寸/圆角由调用方决定）。 */
  className?: string;
}

/** 头像解析结果的渲染：custom/platform 走 `<img>`，initials 走文本。 */
function AvatarImage({ source, label }: { source: AvatarSource; label: string }) {
  if (source.kind === "initials") return <>{providerInitials(label)}</>;
  return <img src={source.url} alt="" aria-hidden="true" draggable={false} />;
}

export function ProviderAvatar({
  avatar,
  nativeId,
  entryName,
  platformName,
  onEdit,
  mobile = false,
  className,
}: ProviderAvatarProps) {
  const { t } = useLang();
  const longPressTimer = useRef<number | null>(null);
  // 长按已触发过：用来吞掉紧随其后的那次 click。与定时器分开两个 ref——
  // 若共用同一个标志，pointerUp 先清掉它，onClick 就永远读不到 true。
  const suppressClick = useRef(false);

  const source = resolveAvatar({ avatar, nativeId, entryName });
  const editable = Boolean(onEdit);

  const clearTimer = () => {
    if (longPressTimer.current !== null) {
      window.clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
  };

  // Android：长按头像开弹窗。用 pointerdown/up 自己计时而非 contextmenu——
  // 长按在触屏上是稳定手势，contextmenu 在 WebView 里行为不一致。
  const handlePointerDown = () => {
    if (!editable || !mobile) return;
    suppressClick.current = false;
    clearTimer();
    longPressTimer.current = window.setTimeout(() => {
      suppressClick.current = true;
      onEdit?.();
    }, LONG_PRESS_MS);
  };
  const handlePointerCancel = () => clearTimer();
  const handlePointerLeave = () => clearTimer();

  // 卸载时清掉在途长按定时器：按压中卡片被移除（刷新/删除/切页签）时，
  // 残留的 setTimeout 会在组件消失后触发 onEdit，打开一个无主弹窗
  useEffect(() => clearTimer, []);

  // 纯展示分支（悬停面板的列表行本身是 button，不能内嵌 button）：
  // 外层不另套壳，把调用方的尺寸类直接落到内层 chip 上——intrinsic 尺寸
  // 与边框圆角都在 `.qt-avatar-inner` 上，多套一层只会把尺寸类悬空。
  if (!editable) {
    return (
      <span className={`qt-avatar-inner${source.className}${className ? ` ${className}` : ""}`}>
        <AvatarImage source={source} label={platformName ?? entryName} />
      </span>
    );
  }

  return (
    <button
      type="button"
      className={`qt-avatar-edit${mobile ? " is-touch" : ""}${className ? ` ${className}` : ""}`}
      aria-label={t("avatar.open")}
      title={t("avatar.open")}
      onPointerDown={handlePointerDown}
      onPointerUp={() => clearTimer()}
      onPointerCancel={handlePointerCancel}
      onPointerLeave={handlePointerLeave}
      onClick={() => {
        // 长按刚触发过弹窗：这次 click 是抬起时的副产物，丢掉避免弹两次。
        // 标志在这里才清——它是「下一次普通点击」的起点。
        if (suppressClick.current) {
          suppressClick.current = false;
          return;
        }
        onEdit?.();
      }}
    >
      <span className={`qt-avatar-inner${source.className}`}>
        <AvatarImage source={source} label={platformName ?? entryName} />
      </span>
      <span className="qt-avatar-scrim" aria-hidden="true" />
      <span className="qt-avatar-gear" aria-hidden="true">
        <Settings size={14} />
      </span>
    </button>
  );
}
