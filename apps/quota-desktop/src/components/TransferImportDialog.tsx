import { AlertTriangle, FileCheck2, FileUp } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import type { ImportOptions, ImportOutcome, TransferContainerInfo } from "../types";
import { transferErrorMessage } from "./configTransferView";
import { stepCountdown } from "./clearConfigView";
import {
  IMPORT_OVERWRITE_COUNTDOWN_SECONDS,
  buildImportOptions,
  resolveImportSubmitState,
  validateImportInput,
  type ImportStrategyChoice,
} from "./transferImportView";
import { Button, DialogShell, SegmentedControl } from "./ui";

/** 从路径截取文件名（Windows/Unix 分隔符兼容；无分隔符时原样返回）。 */
function fileNameOf(path: string): string {
  const segments = path.split(/[\\/]/);
  return segments[segments.length - 1] || path;
}

// 导入模态（T-17，替代原系统 confirm 流程）：文件选择 → inspect 信息卡
// （版本 + 档位，不解密）→ 密码档口令必填 → 策略分段（合并默认）→ 选
// 覆盖展开三重防线（警示卡 + 5 秒阅读倒计时 + 风险勾选，倒计时归零前
// 勾选与确认一并禁用，确认钮转 danger）→ 执行。成功反馈（新增/跳过
// 计数）落在数据页 transferFeedback 后关闭；失败（含错密码）就地
// qt-inline-error 展示、不关弹窗；重选的文件 inspect 失败时旧文件即
// 失效（信息卡清空，防错位提交）。pending 与 inspect 期间双钮禁用、
// 关闭路径锁定，关闭即清空口令（与 T-16 导出模态同一惯例）；
// Esc/焦点圈定/移动端全屏由 DialogShell 既有机制提供。
export function TransferImportDialog({
  open,
  onClose,
  onImport,
  mobile = false,
}: {
  open: boolean;
  onClose: () => void;
  onImport: (path: string, options: ImportOptions) => Promise<ImportOutcome>;
  /** Android SAF 文件选择按 MIME 过滤（与导出同口径）。 */
  mobile?: boolean;
}) {
  const { t } = useLang();
  const [file, setFile] = useState<{ path: string; name: string } | null>(null);
  const [info, setInfo] = useState<TransferContainerInfo | null>(null);
  const [password, setPassword] = useState("");
  const [strategy, setStrategy] = useState<ImportStrategyChoice>("merge");
  const [remaining, setRemaining] = useState(0);
  const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [inspecting, setInspecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) return;
    // 关闭即重置：口令不留前端状态，重开回到未选文件 + 合并默认
    setFile(null);
    setInfo(null);
    setPassword("");
    setStrategy("merge");
    setRemaining(0);
    setAcknowledged(false);
    setBusy(false);
    setInspecting(false);
    setError(null);
  }, [open]);

  // 覆盖防线阅读倒计时：仅覆盖模且弹窗打开时步进（惯例同清空配置）
  useEffect(() => {
    if (!open || strategy !== "overwrite" || remaining <= 0) return;
    const timer = setTimeout(() => setRemaining(stepCountdown(remaining)), 1000);
    return () => clearTimeout(timer);
  }, [open, strategy, remaining]);

  const pickFile = async () => {
    setError(null);
    // 与导出同口径：Android SAF 需要 MIME，桌面文件选择器需要扩展名。
    const path = await openDialog({
      title: t("settings.importDialogTitle"),
      multiple: false,
      directory: false,
      filters: [{
        name: t("settings.transferDialogFilter"),
        extensions: mobile ? ["application/octet-stream"] : ["qtray-export"],
      }],
    });
    if (!path) return;
    setInspecting(true);
    try {
      const inspected = await api.inspectTransferPackage(path);
      setFile({ path, name: fileNameOf(path) });
      setInfo(inspected);
      // 换文件即换口令域：上一文件的口令与覆盖防线不跨文件残留
      setPassword("");
      setAcknowledged(false);
      setRemaining(
        strategy === "overwrite" ? IMPORT_OVERWRITE_COUNTDOWN_SECONDS : 0,
      );
    } catch (e) {
      // 重选坏文件时旧文件即失效：清空信息卡防「错误横幅指向新文件、
      // 确认钮却提交旧文件」的错位（首次选择失败时本就是空，清空幂等）
      setFile(null);
      setInfo(null);
      setPassword("");
      setError(transferErrorMessage(e));
    } finally {
      setInspecting(false);
    }
  };

  const submit = async () => {
    if (!file || !info) return;
    const problem = validateImportInput(info.mode, password);
    if (problem) {
      setError(t(problem.key));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await onImport(file.path, buildImportOptions(info.mode, password, strategy));
      onClose();
    } catch (e) {
      // 失败（含错密码）就地展示，弹窗保留供调整后重试
      setError(transferErrorMessage(e));
      setBusy(false);
    }
  };

  if (!open) return null;

  const inputError = info ? validateImportInput(info.mode, password) : null;
  const state = resolveImportSubmitState({
    hasFile: file != null && info != null,
    passwordError: inputError != null,
    strategy,
    acknowledged,
    countdownRemaining: remaining,
  });
  const confirmLabel = busy
    ? t("settings.importing")
    : state.countdownSeconds != null
      ? t("settings.importConfirmOverwriteCountdown", {
          seconds: state.countdownSeconds,
        })
      : state.danger
        ? t("settings.importConfirmOverwrite")
        : t("settings.importModalConfirm");

  return (
    <DialogShell
      title={t("settings.importModalTitle")}
      description={t("settings.importModalDescription")}
      // busy/inspecting 期间锁定 Esc/右上角 X：导入写入进行中关闭会让
      // 结果不可见；inspect 未返回时关闭会让迟到回填命中已重置的模态
      onClose={busy || inspecting ? () => {} : onClose}
      closeLabel={t("common.cancel")}
      size="sm"
      footer={
        <>
          <Button disabled={busy || inspecting} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant={state.danger ? "danger" : "primary"}
            icon={FileUp}
            disabled={busy || inspecting || !state.canSubmit}
            onClick={() => void submit()}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      <div className="qt-transfer-import-body">
        {file && info ? (
          <div className="qt-transfer-import-file">
            <FileCheck2 size={18} aria-hidden="true" />
            <div className="qt-transfer-import-file-meta">
              <p className="qt-transfer-import-file-name">{file.name}</p>
              <p>
                {t("settings.importContainerVersion", { version: String(info.version) })}
                {" · "}
                {info.mode === "Password"
                  ? t("settings.importContainerPasswordMode")
                  : t("settings.importContainerConvenientMode")}
              </p>
            </div>
            <Button
              variant="ghost"
              disabled={busy || inspecting}
              onClick={() => void pickFile()}
            >
              {t("settings.importPickAnother")}
            </Button>
          </div>
        ) : (
          <Button
            icon={FileUp}
            disabled={busy || inspecting}
            onClick={() => void pickFile()}
          >
            {inspecting ? t("settings.importInspecting") : t("settings.importPickFile")}
          </Button>
        )}
        {info?.mode === "Password" && (
          <label className="qt-field">
            <span>{t("settings.importPasswordLabel")}</span>
            <input
              className="qt-input"
              type="password"
              autoComplete="new-password"
              disabled={busy}
              value={password}
              onChange={(event) => setPassword(event.target.value)}
            />
          </label>
        )}
        <div className="qt-field">
          <span>{t("settings.importStrategyLabel")}</span>
          <SegmentedControl
            value={strategy}
            options={[
              { value: "merge", label: t("settings.importStrategyMerge") },
              { value: "overwrite", label: t("settings.importStrategyOverwrite") },
            ]}
            onChange={(next) => {
              setStrategy(next);
              // 策略切换重置覆盖防线（重新进入需重读重勾），错误文案
              // 只描述当前选择
              setRemaining(
                next === "overwrite" ? IMPORT_OVERWRITE_COUNTDOWN_SECONDS : 0,
              );
              setAcknowledged(false);
              setError(null);
            }}
          />
          <p className="qt-field-hint">
            {strategy === "merge"
              ? t("settings.importMergeHint")
              : t("settings.importOverwriteHint")}
          </p>
        </div>
        {strategy === "overwrite" && (
          <>
            <div className="qt-transfer-intro">
              <span className="qt-transfer-intro-icon">
                <AlertTriangle size={18} aria-hidden="true" />
              </span>
              <div>
                <p>{t("settings.importOverwriteWarning")}</p>
              </div>
            </div>
            <label className="qt-transfer-import-ack">
              <input
                type="checkbox"
                checked={acknowledged}
                disabled={!state.ackEnabled || busy}
                onChange={(event) => setAcknowledged(event.target.checked)}
              />
              <span>{t("settings.importOverwriteAck")}</span>
            </label>
          </>
        )}
        {error && (
          <p className="qt-inline-error" role="alert">
            {error}
          </p>
        )}
      </div>
    </DialogShell>
  );
}
