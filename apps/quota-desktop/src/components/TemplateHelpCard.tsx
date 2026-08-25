// 模板编写说明折叠卡：「设置模板」子页底部，变量、字段速查与最小示例。
// 默认收起——内容较长，避免拉高编辑对话框；标题常驻保证可发现性。
import { ChevronDown } from "lucide-react";
import { useId, useState } from "react";
import { useLang } from "../i18n";
import { presetJsonOf } from "./presetTemplates";

export function TemplateHelpCard() {
  const { t } = useLang();
  const [open, setOpen] = useState(false);
  const bodyId = useId();

  return (
    <div className="qt-template-help">
      <button
        type="button"
        className="qt-template-help-toggle"
        aria-expanded={open}
        aria-controls={bodyId}
        onClick={() => setOpen(!open)}
      >
        <ChevronDown size={14} className={open ? "is-open" : ""} aria-hidden="true" />
        {t("edit.helpTitle")}
      </button>
      {open && (
        <div id={bodyId} className="qt-template-help-body">
          <p className="qt-template-help-lead">{t("edit.helpLead")}</p>
          <dl className="qt-template-help-grid">
            <dt><code>{"{{apiKey}}"}</code></dt>
            <dd>{t("edit.helpVarKey")}</dd>
            <dt><code>{"{{baseUrl}}"}</code></dt>
            <dd>{t("edit.helpVarUrl")}</dd>
            <dt><code>request</code></dt>
            <dd>{t("edit.helpRequest")}</dd>
            <dt><code>extract</code></dt>
            <dd>{t("edit.helpExtract")}</dd>
            <dt><code>{"$.data.balance"}</code></dt>
            <dd>{t("edit.helpPath")}</dd>
            <dt><code>transforms</code></dt>
            <dd>{t("edit.helpTransforms")}</dd>
            <dt><code>windowsFrom</code></dt>
            <dd>{t("edit.helpWindows")}</dd>
            <dt><code>allowInsecure</code></dt>
            <dd>{t("edit.helpInsecure")}</dd>
          </dl>
          <pre>{presetJsonOf("custom")}</pre>
          <p className="qt-template-help-tip">{t("edit.helpTip")}</p>
        </div>
      )}
    </div>
  );
}
