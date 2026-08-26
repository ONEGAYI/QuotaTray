// 外部 AI/Agent 调试桥：生成脱敏求助包并暴露 CLI 指引；QuotaTray 本身不提供 Agent。
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Check, Copy, FileDown, Terminal, X } from "lucide-react";
import { useMemo, useState } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import { Button } from "./ui";
import {
  buildAiAssistPackage,
  buildAiAssistPrompt,
  buildLocalAgentPrompt,
  defaultAssistFileName,
  type AiAssistMode,
} from "./aiAssistPack";

interface Props {
  mode: AiAssistMode;
  providerName: string;
  baseUrl: string;
  draft: string;
  /** 已保存条目 id（新增草稿为 null）：携带进诊断包供 assist test 端测 */
  entryId?: string | null;
  validationMessage?: string | null;
  testError?: string | null;
  onClose: () => void;
}

function ensureAssistExtension(path: string): string {
  return path.toLowerCase().endsWith(".qtray-assist.json")
    ? path
    : `${path}.qtray-assist.json`;
}

export function AiAssistPanel(props: Props) {
  const { t, lang } = useLang();
  const [docsUrl, setDocsUrl] = useState("");
  const [goal, setGoal] = useState("");
  const [responseSample, setResponseSample] = useState("");
  const [savedPath, setSavedPath] = useState<string | null>(null);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [copiedAction, setCopiedAction] = useState<"prompt" | "agent" | null>(null);
  const [saving, setSaving] = useState(false);

  const input = useMemo(
    () => ({
      mode: props.mode,
      providerName: props.providerName,
      baseUrl: props.baseUrl,
      docsUrl,
      goal,
      draft: props.draft,
      entryId: props.entryId,
      validationMessage: props.validationMessage,
      testError: props.testError,
      responseSample,
    }),
    [
      props.mode,
      props.providerName,
      props.baseUrl,
      props.draft,
      props.entryId,
      props.validationMessage,
      props.testError,
      docsUrl,
      goal,
      responseSample,
    ],
  );
  const prompt = useMemo(() => buildAiAssistPrompt(input, lang), [input, lang]);

  const copyText = async (
    text: string,
    success: string,
    action: "prompt" | "agent",
  ) => {
    try {
      await navigator.clipboard.writeText(text);
      setFeedback(success);
      setCopiedAction(action);
    } catch {
      setFeedback(t("edit.ai.copyFailed"));
      setCopiedAction(null);
    }
  };

  const savePackage = async (): Promise<string | null> => {
    setSaving(true);
    try {
      const selected = await saveDialog({
        title: t("edit.ai.saveTitle"),
        defaultPath: defaultAssistFileName(props.providerName),
        filters: [{ name: "QuotaTray AI assist", extensions: ["json"] }],
      });
      if (!selected) return null;
      const path = ensureAssistExtension(selected);
      const pkg = JSON.stringify(buildAiAssistPackage(input), null, 2);
      await api.writeAssistPackage(path, pkg);
      setSavedPath(path);
      setFeedback(t("edit.ai.saved", { path }));
      return path;
    } catch (error) {
      setFeedback(t("edit.ai.saveFailed", { error: String(error) }));
      return null;
    } finally {
      setSaving(false);
    }
  };

  const copyLocalAgentPrompt = async () => {
    const path = savedPath ?? (await savePackage());
    if (!path) return;
    try {
      const quotaCliPath = await api.resolveQuotaCliPath();
      await copyText(
        buildLocalAgentPrompt(path, props.mode, quotaCliPath, lang, Boolean(props.entryId)),
        t("edit.ai.agentPromptCopied"),
        "agent",
      );
    } catch (error) {
      setCopiedAction(null);
      setFeedback(t("edit.ai.cliMissing", { error: String(error) }));
    }
  };

  return (
    <section className="qt-ai-assist" aria-label={t("edit.ai.title")}>
      <header className="qt-ai-assist-head">
        <div className="qt-ai-assist-heading">
          <span className="qt-ai-placeholder-icon" aria-hidden="true">AI</span>
          <div>
            <h3>{t("edit.ai.title")}</h3>
            <p>{t("edit.ai.description")}</p>
          </div>
        </div>
        <button type="button" className="qt-ai-assist-close" onClick={props.onClose} aria-label={t("titlebar.close")}>
          <X size={15} aria-hidden="true" />
        </button>
      </header>

      <p className="qt-ai-assist-safety">{t("edit.ai.safety")}</p>

      <div className="qt-ai-assist-fields">
        <label className="qt-field">
          <span>{t("edit.ai.docsUrl")}</span>
          <input
            className="qt-input"
            value={docsUrl}
            onChange={(event) => setDocsUrl(event.target.value)}
            placeholder="https://example.com/docs"
          />
        </label>
        <label className="qt-field">
          <span>{t("edit.ai.goal")}</span>
          <input
            className="qt-input"
            value={goal}
            onChange={(event) => setGoal(event.target.value)}
            placeholder={t("edit.ai.goalPlaceholder")}
          />
        </label>
      </div>

      <label className="qt-field">
        <span>{t("edit.ai.responseSample")}</span>
        <textarea
          className="qt-input qt-ai-response-sample"
          value={responseSample}
          onChange={(event) => setResponseSample(event.target.value)}
          placeholder={t("edit.ai.responsePlaceholder")}
        />
        <small className="qt-field-hint">{t("edit.ai.responseHint")}</small>
      </label>

      <label className="qt-field">
        <span>{t("edit.ai.preview")}</span>
        <textarea className="qt-input qt-ai-prompt-preview" readOnly value={prompt} />
      </label>

      <div className="qt-ai-assist-actions">
        <Button type="button" onClick={() => void copyText(prompt, t("edit.ai.promptCopied"), "prompt")}>
          {copiedAction === "prompt" ? <Check size={14} aria-hidden="true" /> : <Copy size={14} aria-hidden="true" />}
          {copiedAction === "prompt" ? t("edit.ai.copied") : t("edit.ai.copyPrompt")}
        </Button>
        <Button type="button" disabled={saving} onClick={() => void savePackage()}>
          <FileDown size={14} aria-hidden="true" />
          {saving ? t("edit.ai.saving") : t("edit.ai.savePackage")}
        </Button>
        <Button type="button" disabled={saving} onClick={() => void copyLocalAgentPrompt()}>
          {copiedAction === "agent" ? <Check size={14} aria-hidden="true" /> : <Terminal size={14} aria-hidden="true" />}
          {copiedAction === "agent" ? t("edit.ai.copied") : t("edit.ai.localAgentPrompt")}
        </Button>
      </div>
      <p className="qt-ai-assist-service-note">{t("edit.ai.serviceNote")}</p>
      {feedback && <p className="qt-ai-assist-feedback" role="status">{feedback}</p>}
    </section>
  );
}
