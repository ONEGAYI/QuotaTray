// 添加/编辑对话框：native / template / script(M4 预留) 三形态。
// 红线 3：key 框初始为空（占位符「已配置/未配置」），空 = 保持不变，永不回显明文。
// CodeMirror 编辑器为第三方亮色主题，dark 模式下仅调整容器边框（后续版本可换主题）。
import CodeMirror from "@uiw/react-codemirror";
import { json } from "@codemirror/lang-json";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { api, newEntryId } from "../api";
import { dataSummary } from "../display";
import { useLang } from "../i18n";
import { useNativeMetas } from "../queries";
import type { ProviderEntry, ProviderKind, QueryOutcome, TemplateErrorDto } from "../types";

interface Props {
  open: boolean;
  initial: ProviderEntry | null; // null = 新增
  onClose: () => void;
}

const DEFAULT_TEMPLATE = `{
  "request": {
    "url": "{{baseUrl}}/v1/user/info",
    "headers": { "Authorization": "Bearer {{apiKey}}" }
  },
  "extract": {
    "remaining": "$.data.totalBalance",
    "unit": { "const": "CNY" }
  }
}`;

type Tab = "native" | "template" | "script";

/** invoke 抛出的错误若为后端 reject 的 TemplateErrorDto 则还原形状，否则 null。 */
function toTemplateError(e: unknown): TemplateErrorDto | null {
  if (typeof e === "object" && e != null && "field" in e && "reason" in e) {
    return e as TemplateErrorDto;
  }
  return null;
}

const inputCls =
  "mt-1 w-full rounded border border-slate-300 bg-white px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none dark:border-slate-600 dark:bg-slate-900 dark:text-slate-100";
const labelCls = "text-sm text-slate-600 dark:text-slate-300";

export function EditDialog({ open, initial, onClose }: Props) {
  const qc = useQueryClient();
  const { t } = useLang();
  const natives = useNativeMetas();
  const [tab, setTab] = useState<Tab>(initial?.kind.type === "template" ? "template" : "native");
  const [name, setName] = useState(initial?.name ?? "");
  const [nativeProvider, setNativeProvider] = useState(
    initial?.kind.type === "native" ? initial.kind.provider : "",
  );
  const [templateJson, setTemplateJson] = useState(() => {
    if (initial?.kind.type !== "template") return DEFAULT_TEMPLATE;
    const { type: _type, ...rest } = initial.kind;
    return JSON.stringify(rest, null, 2);
  });
  const [baseUrl, setBaseUrl] = useState(initial?.base_url ?? "");
  const [apiKey, setApiKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  // id 在打开期间保持稳定（新增时生成一次）
  const id = useMemo(() => initial?.id ?? newEntryId(), [initial]);
  const configured = Boolean(initial?.api_key_enc);

  const invalidateAll = () => {
    void qc.invalidateQueries({ queryKey: ["providers"] });
    void qc.invalidateQueries({ queryKey: ["provider"] });
  };

  const save = useMutation({
    mutationFn: async () => {
      setError(null);
      const trimmedName = name.trim();
      if (!trimmedName) throw new Error(t("edit.nameRequired"));
      let kind: ProviderKind;
      if (tab === "native") {
        if (!nativeProvider) throw new Error(t("edit.nativeRequired"));
        kind = { type: "native", provider: nativeProvider };
      } else {
        // 保存前先静态校验（结构 + 规则），错误带字段定位
        try {
          await api.validateTemplate(templateJson);
        } catch (e) {
          const dto = toTemplateError(e);
          throw new Error(
            dto
              ? dto.field === "(json)"
                ? t("edit.jsonError", { msg: dto.reason })
                : t("edit.fieldError", { field: dto.field, reason: dto.reason })
              : String(e),
          );
        }
        let parsed: unknown;
        try {
          parsed = JSON.parse(templateJson);
        } catch {
          throw new Error(t("edit.templateJsonError"));
        }
        kind = { type: "template", ...(parsed as object) } as ProviderKind;
      }
      const entry: ProviderEntry = {
        id,
        name: trimmedName,
        kind,
        enabled: initial?.enabled ?? true,
        api_key_enc: undefined, // 后端忽略；密文由 key 策略维护
        // template 采表单值；native 条目无表单，保留既有值（native 查询不消费它）
        base_url:
          tab === "template" ? (baseUrl.trim() || undefined) : initial?.base_url,
      };
      await api.upsertProvider(entry, apiKey.trim() ? apiKey : null);
    },
    onSuccess: () => {
      invalidateAll();
      onClose();
    },
    onError: (e) => setError(e.message),
  });

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/30 p-4">
      <div className="flex max-h-full w-full max-w-xl flex-col overflow-hidden rounded-lg bg-white shadow-xl dark:bg-slate-800">
        <div className="border-b border-slate-200 px-5 py-3 dark:border-slate-700">
          <h2 className="font-medium">{initial ? t("edit.titleEdit") : t("edit.titleAdd")}</h2>
        </div>

        <div className="flex gap-1 border-b border-slate-200 px-5 pt-2 dark:border-slate-700">
          {(["native", "template", "script"] as Tab[]).map((tabId) => (
            <button
              key={tabId}
              disabled={tabId === "script"}
              onClick={() => setTab(tabId)}
              className={`rounded-t px-3 py-1.5 text-sm disabled:cursor-not-allowed disabled:opacity-40 ${
                tab === tabId
                  ? "border border-b-white border-slate-200 bg-white font-medium dark:border-slate-700 dark:border-b-slate-800 dark:bg-slate-800"
                  : "text-slate-500 hover:bg-slate-50 dark:text-slate-400 dark:hover:bg-slate-700/60"
              }`}
            >
              {tabId === "native"
                ? t("edit.tabNative")
                : tabId === "template"
                  ? t("edit.tabTemplate")
                  : t("edit.tabScript")}
            </button>
          ))}
        </div>

        <form
          className="flex-1 space-y-4 overflow-y-auto px-5 py-4"
          onSubmit={(e) => {
            e.preventDefault();
            save.mutate();
          }}
        >
          <label className="block">
            <span className={labelCls}>{t("edit.name")}</span>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("edit.namePlaceholder")}
              className={inputCls}
            />
          </label>

          {tab === "native" && (
            <label className="block">
              <span className={labelCls}>{t("edit.platform")}</span>
              <select
                value={nativeProvider}
                onChange={(e) => setNativeProvider(e.target.value)}
                className={inputCls}
              >
                <option value="">{t("edit.platformPlaceholder")}</option>
                {(natives.data ?? []).map((m) => (
                  <option key={m.id} value={m.id}>
                    {t("edit.platformOption", { name: m.name, id: m.id })}
                  </option>
                ))}
              </select>
            </label>
          )}

          {tab === "template" && (
            <TemplateForm
              templateJson={templateJson}
              setTemplateJson={setTemplateJson}
              baseUrl={baseUrl}
              setBaseUrl={setBaseUrl}
              apiKey={apiKey}
              setApiKey={setApiKey}
            />
          )}

          <label className="block">
            <span className={labelCls}>{t("edit.apiKey")}</span>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              autoComplete="new-password"
              placeholder={configured ? t("edit.keyConfigured") : t("edit.keyMissing")}
              className={inputCls}
            />
          </label>

          {error && (
            <p className="rounded bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-950/40 dark:text-red-400">
              {error}
            </p>
          )}
        </form>

        <div className="flex justify-end gap-2 border-t border-slate-200 px-5 py-3 dark:border-slate-700">
          <button
            onClick={onClose}
            className="rounded px-4 py-1.5 text-sm text-slate-600 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-700"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={() => save.mutate()}
            disabled={save.isPending}
            className="rounded bg-indigo-600 px-4 py-1.5 text-sm text-white hover:bg-indigo-500 disabled:opacity-50 dark:bg-indigo-500 dark:hover:bg-indigo-400"
          >
            {save.isPending ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** 模板形态：JSON 编辑器 + baseUrl + 校验/试查。 */
function TemplateForm(props: {
  templateJson: string;
  setTemplateJson: (v: string) => void;
  baseUrl: string;
  setBaseUrl: (v: string) => void;
  apiKey: string;
  setApiKey: (v: string) => void;
}) {
  const { t, lang } = useLang();
  const [validateMsg, setValidateMsg] = useState<string | null>(null);
  const [validateOk, setValidateOk] = useState(false);
  const [testResult, setTestResult] = useState<QueryOutcome | null>(null);
  const [testing, setTesting] = useState(false);

  const validate = useMutation({
    mutationFn: () => api.validateTemplate(props.templateJson),
    onSuccess: () => {
      setValidateMsg(null);
      setValidateOk(true);
    },
    onError: (e) => {
      const dto = toTemplateError(e);
      setValidateOk(false);
      setValidateMsg(
        dto
          ? dto.field === "(json)"
            ? t("edit.jsonError", { msg: dto.reason })
            : t("edit.fieldError", { field: dto.field, reason: dto.reason })
          : String(e),
      );
    },
  });

  const test = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const r = await api.testTemplate(
        props.templateJson,
        props.apiKey.trim() || null,
        props.baseUrl.trim() || null,
      );
      setTestResult(r);
    } catch (e) {
      setTestResult({
        ok: false,
        data: null,
        error: { kind: "deterministic", message: String(e) },
        at: null,
      });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="space-y-3">
      {/"allowInsecure"\s*:\s*true/.test(props.templateJson) && (
        <p className="rounded bg-amber-50 px-3 py-2 text-xs text-amber-700 dark:bg-amber-950/40 dark:text-amber-300">
          {t("edit.insecureWarn")}
        </p>
      )}
      <label className="block">
        <span className={labelCls}>{t("edit.templateJson")}</span>
        <div className="mt-1 overflow-hidden rounded border border-slate-300 dark:border-slate-600">
          <CodeMirror
            value={props.templateJson}
            onChange={(v) => {
              props.setTemplateJson(v);
              // 内容已变，旧校验结论作废
              setValidateOk(false);
              setValidateMsg(null);
            }}
            extensions={[json()]}
            height="180px"
            basicSetup={{ foldGutter: false, autocompletion: false }}
          />
        </div>
      </label>

      <label className="block">
        <span className={labelCls}>{t("edit.baseUrl")}</span>
        <input
          value={props.baseUrl}
          onChange={(e) => props.setBaseUrl(e.target.value)}
          placeholder="https://api.example.com"
          className={inputCls}
        />
      </label>

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => validate.mutate()}
          disabled={validate.isPending}
          className="rounded border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-50 disabled:opacity-50 dark:border-slate-600 dark:text-slate-200 dark:hover:bg-slate-700"
        >
          {t("edit.validate")}
        </button>
        <button
          type="button"
          onClick={() => void test()}
          disabled={testing}
          className="rounded border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-50 disabled:opacity-50 dark:border-slate-600 dark:text-slate-200 dark:hover:bg-slate-700"
        >
          {testing ? t("edit.testing") : t("edit.test")}
        </button>
        {validateOk && !validateMsg && (
          <span className="text-xs text-green-600 dark:text-green-400">{t("edit.validated")}</span>
        )}
      </div>

      {validateMsg && (
        <p className="rounded bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-950/40 dark:text-red-400">
          {validateMsg}
        </p>
      )}

      {testResult && (
        <div className="rounded bg-slate-50 p-3 text-sm dark:bg-slate-900/60 dark:text-slate-200">
          {testResult.ok ? (
            <div>
              <p className="mb-1 text-xs text-green-600 dark:text-green-400">{t("edit.testOk")}</p>
              {(testResult.data ?? []).map((d, i) => (
                <p key={i}>
                  {d.plan_name ? `${d.plan_name} · ` : ""}
                  {dataSummary(d, lang)}
                </p>
              ))}
            </div>
          ) : (
            <p
              className={
                testResult.error?.kind === "transient"
                  ? "text-slate-600 dark:text-slate-300"
                  : "text-red-600 dark:text-red-400"
              }
            >
              {testResult.error?.kind === "transient" ? "⟳" : "⚠"} {testResult.error?.message}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
