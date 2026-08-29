// 消息中心（桌面标题栏 / 移动顶部应用栏共用）：铃铛 + 未读红点 +
// 点击展开面板（样式契约见 frontend-style-spec T-009）。卡片按消息
// kind 分支：update-ready 仅桌面产生（「现在安装」直调后端静默安装，
// 卡片文案已明示「退出并自动重启」后果，点击即确认，不叠加系统
// confirm）；update-available 仅移动端产生（无自动下载，引导到设置·
// 更新页）；low-balance 两端共用（纯展示）。
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Bell } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import { hasUnread, messageId, type CenterMessage } from "./messageCenterView";
import { DropdownMenu, IconButton, Button } from "./ui";

export function MessageCenter({
  messages,
  seen,
  onSeenAll,
  onViewUpdates,
}: {
  messages: CenterMessage[];
  seen: ReadonlySet<string>;
  /** 打开面板时回调（红点全量清除）。 */
  onSeenAll: () => void;
  /** 「查看更新」回调（移动端 update-available 卡片：打开设置·更新页）。 */
  onViewUpdates?: () => void;
}) {
  const { t } = useLang();
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);
  const [installFailed, setInstallFailed] = useState(false);
  const install = useMutation({
    mutationFn: api.installUpdate,
    onError: () => {
      setInstallFailed(true);
      // 状态刷新后设置页「更新」页签的错误展示同步可见（手动重试入口）
      void qc.invalidateQueries({ queryKey: ["update-state"] });
    },
  });
  const unread = hasUnread(messages, seen);

  const toggle = () => {
    if (open) {
      setOpen(false);
      return;
    }
    setInstallFailed(false);
    onSeenAll();
    setOpen(true);
  };

  const viewUpdates = () => {
    setOpen(false);
    onViewUpdates?.();
  };

  return (
    <div className="qt-titlebar-menu-anchor">
      <IconButton
        icon={Bell}
        label={t("titlebar.messages")}
        aria-expanded={open}
        onClick={toggle}
      />
      {unread && <span className="qt-msg-dot" aria-hidden="true" />}
      <DropdownMenu open={open} onClose={() => setOpen(false)} className="qt-msg-panel">
        {messages.length === 0 ? (
          <p className="qt-msg-empty">{t("msgCenter.empty")}</p>
        ) : (
          messages.map((message) => (
            <div key={messageId(message)} className="qt-msg-card">
              {message.kind === "update-ready" && (
                <>
                  <p className="qt-msg-card-title">{t("msgCenter.updateReadyTitle")}</p>
                  <p className="qt-msg-card-body">
                    {t("msgCenter.updateReadyBody", { version: `v${message.version}` })}
                  </p>
                  <Button
                    variant="secondary"
                    className="qt-msg-install"
                    disabled={install.isPending}
                    onClick={() => install.mutate()}
                  >
                    {t("msgCenter.installNow")}
                  </Button>
                  <p className="qt-msg-card-hint">
                    {installFailed ? t("msgCenter.installFailed") : t("msgCenter.autoRestartHint")}
                  </p>
                </>
              )}
              {message.kind === "update-available" && (
                <>
                  <p className="qt-msg-card-title">{t("msgCenter.updateAvailableTitle")}</p>
                  <p className="qt-msg-card-body">
                    {t("msgCenter.updateAvailableBody", { version: `v${message.version}` })}
                  </p>
                  <Button
                    variant="secondary"
                    className="qt-msg-view-update"
                    onClick={viewUpdates}
                  >
                    {t("msgCenter.viewUpdate")}
                  </Button>
                  <p className="qt-msg-card-hint">{t("msgCenter.updateGoToHint")}</p>
                </>
              )}
              {message.kind === "low-balance" && (
                <>
                  <p className="qt-msg-card-title">{t("msgCenter.lowBalanceTitle")}</p>
                  <p className="qt-msg-card-body">
                    {t("msgCenter.lowBalanceBody", {
                      name: message.name,
                      percent: `${Math.round(message.percent)}`,
                    })}
                  </p>
                </>
              )}
            </div>
          ))
        )}
      </DropdownMenu>
    </div>
  );
}
