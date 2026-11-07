/** 头像设置模态窗：内置品牌图挑选 + 上传本地 SVG/PNG + 恢复默认。
 *
 * 数据流（两段式，为的是「先看到预览再决定」）：
 * 1. 选内置图 → 立即得到 `brand:<slug>`；上传 → 交给后端
 *    `prepareProviderAvatar` 规整（解码/缩放/重编码），拿到 data URL
 * 2. 用户点「保存」才调 `setProviderAvatar` 落盘
 *
 * 后端是唯一校验口：前端只做形态判别与预览，不做体积/白名单判断，
 * 避免两侧规则漂移（core 那套规则 CLI 也要用）。
 */

import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { RotateCcw, Upload } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import { invalidateProviderCaches } from "../queries";
import {
  avatarBrandKey,
  avatarValueKind,
  builtinAvatarChoices,
  canResetAvatar,
  resolveAvatar,
} from "./avatarView";
import { isLightLogo, providerInitials } from "./providerIcon";
import { Button, DialogShell } from "./ui";

/** 上传对话框的文件过滤：桌面按扩展名，Android SAF 用 MIME。 */
const UPLOAD_FILTER_DESKTOP = ["png", "svg"];
const UPLOAD_FILTER_MOBILE = ["image/png", "image/svg+xml"];

export interface AvatarDialogProps {
  open: boolean;
  entryId: string;
  entryName: string;
  /** native 平台 id；模板/脚本条目为 null。 */
  nativeId: string | null;
  /** 平台显示名（首字母回退用）；缺省回退到 entryName。
   *  与卡片、悬停面板同口径——四处显示同一个首字母。 */
  platformName?: string;
  /** 当前已保存的头像值（`brand:` / data URL / 缺省）。 */
  currentAvatar?: string | null;
  mobile?: boolean;
  onClose: () => void;
}

export function AvatarDialog({
  open,
  entryId,
  entryName,
  platformName,
  nativeId,
  currentAvatar,
  mobile = false,
  onClose,
}: AvatarDialogProps) {
  const { t } = useLang();
  const qc = useQueryClient();
  // 草稿：null = 未改动（沿用 currentAvatar）；"" = 显式恢复默认
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 每次打开重置草稿与错误：上次取消的半成品不得泄漏到本次
  useEffect(() => {
    if (!open) return;
    setDraft(null);
    setError(null);
    setBusy(false);
  }, [open]);

  const effective = draft ?? currentAvatar ?? "";
  const preview = useMemo(
    () => resolveAvatar({ avatar: effective, nativeId, entryName }),
    [effective, nativeId, entryName],
  );
  const builtin = useMemo(() => builtinAvatarChoices(), []);
  const selectedBrand = avatarBrandKey(effective);
  const isCustomUpload = avatarValueKind(effective) === "data";
  const canReset = canResetAvatar(currentAvatar) || draft !== null;

  // 关闭态不渲染：DialogShell 的遮罩是 fixed 全屏 + blur，若在关闭态挂上
  // 会把整个应用首屏盖住（与 EditDialog 同口径）。守卫必须在所有 hooks
  // 之后——提前 return 会改变 hook 调用顺序。
  if (!open) return null;

  const pickUpload = async () => {
    setError(null);
    const path = await openDialog({
      title: t("avatar.pickTitle"),
      multiple: false,
      directory: false,
      filters: [
        {
          name: t("avatar.filterName"),
          extensions: mobile ? UPLOAD_FILTER_MOBILE : UPLOAD_FILTER_DESKTOP,
        },
      ],
    });
    if (!path) return;
    setBusy(true);
    try {
      // 后端读文件 + 规整（桌面直读，Android 走 SAF）
      const normalized = await api.prepareProviderAvatar(path);
      setDraft(normalized);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      // draft 为 null 表示未改动：仍按当前值提交（幂等，后端对相同值直接返回）
      await api.setProviderAvatar(entryId, effective);
      // 后端已广播 providers-changed，卡片经既有监听重取列表；
      // 这里补一次本窗口的立即失效，避免等待事件往返造成视觉延迟
      invalidateProviderCaches(qc, entryId);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogShell
      title={t("avatar.title")}
      description={t("avatar.description", { name: entryName })}
      onClose={busy ? () => {} : onClose}
      closeLabel={t("common.cancel")}
      size="md"
      className="qt-dialog-avatar"
      footer={
        <>
          <Button disabled={busy} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button variant="primary" disabled={busy} onClick={() => void save()}>
            {t("common.save")}
          </Button>
        </>
      }
    >
      <div className="qt-avatar-body">
        <div className="qt-avatar-preview">
          <span className={`qt-avatar-preview-chip${preview.className}`}>
            {preview.kind === "initials" ? (
              providerInitials(platformName ?? entryName)
            ) : (
              <img src={preview.url} alt="" aria-hidden="true" draggable={false} />
            )}
          </span>
          <div className="qt-avatar-preview-meta">
            <strong>{t("avatar.preview")}</strong>
            <small>
              {isCustomUpload
                ? t("avatar.sourceUpload")
                : selectedBrand
                  ? t("avatar.sourceBrand", { slug: selectedBrand })
                  : t("avatar.sourceDefault")}
            </small>
          </div>
          <div className="qt-avatar-preview-actions">
            <Button icon={Upload} disabled={busy} onClick={() => void pickUpload()}>
              {busy ? t("avatar.preparing") : t("avatar.upload")}
            </Button>
            <Button
              icon={RotateCcw}
              disabled={busy || !canReset}
              onClick={() => setDraft("")}
            >
              {t("avatar.reset")}
            </Button>
          </div>
        </div>

        <section className="qt-avatar-builtin" aria-label={t("avatar.builtinHeading")}>
          <h3>{t("avatar.builtinHeading")}</h3>
          <p className="qt-avatar-hint">{t("avatar.builtinHint")}</p>
          <div className="qt-avatar-grid" role="radiogroup" aria-label={t("avatar.builtinHeading")}>
            {builtin.map((choice) => (
              <button
                key={choice.slug}
                type="button"
                role="radio"
                aria-checked={selectedBrand === choice.slug}
                aria-label={choice.slug}
                className={`qt-avatar-choice${
                  selectedBrand === choice.slug ? " is-active" : ""
                }${isLightLogo(choice.slug) ? " is-light-logo" : ""}`}
                disabled={busy}
                onClick={() => setDraft(`brand:${choice.slug}`)}
              >
                <img src={choice.url} alt="" aria-hidden="true" draggable={false} />
              </button>
            ))}
          </div>
        </section>

        {error && (
          <p className="qt-avatar-error" role="alert">
            {error}
          </p>
        )}
        <p className="qt-avatar-hint">{t("avatar.formatHint")}</p>
      </div>
    </DialogShell>
  );
}
