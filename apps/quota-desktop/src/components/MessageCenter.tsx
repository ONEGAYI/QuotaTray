// 标题栏消息中心：铃铛 + 未读红点 + 点击展开面板（样式契约见
// frontend-style-spec T-009）。「现在安装」直调后端静默安装——卡片文案
// 已明示「退出并自动重启」后果，点击即确认，不叠加系统 confirm。
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Bell } from "lucide-react";
import { useState } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import type { CenterMessage } from "./messageCenterView";
import { hasUnread } from "./messageCenterView";
import { DropdownMenu, IconButton, Button } from "./ui";

export function MessageCenter({
  messages,
  seen,
  onSeenAll,
}: {
  messages: CenterMessage[];
  seen: ReadonlySet<string>;
  /** 打开面板时回调（红点全量清除）。 */
  onSeenAll: () => void;
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
    setOpen((prev) => {
      const next = !prev;
      if (next) {
        setInstallFailed(false);
        onSeenAll();
      }
      return next;
    });
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
            <div key={`${message.kind}:${message.version}`} className="qt-msg-card">
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
            </div>
          ))
        )}
      </DropdownMenu>
    </div>
  );
}
