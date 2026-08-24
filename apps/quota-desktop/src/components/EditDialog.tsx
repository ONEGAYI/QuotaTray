// 添加/编辑对话框：native / template / script 三形态。
// 红线 3：key 框初始为空（占位符「已配置/未配置」），空 = 保持不变，永不回显明文。
// CodeMirror 编辑器为第三方亮色主题，dark 模式下仅调整容器边框（后续版本可换主题）。
import CodeMirror from "@uiw/react-codemirror";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo, useRef, useState } from "react";
import { api, newEntryId } from "../api";
import { dataSummary } from "../display";
import { useLang } from "../i18n";
import { useNativeMetas } from "../queries";
import type {
  PlanVariant,
  PricingConfig,
  ProviderEntry,
  ProviderKind,
  QueryOutcome,
  TemplateErrorDto,
} from "../types";
import { NativeProviderPicker } from "./NativeProviderPicker";
import { PricingSection } from "./PricingSection";
import { Button, DialogShell } from "./ui";

interface Props {
  open: boolean;
  initial: ProviderEntry | null; // null = 新增
  usageCurrency?: string;
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

/** 默认示例脚本：与 examples/scripts/basic.js 同款最小闭环。 */
const DEFAULT_SCRIPT = `function request() {
  return {
    url: "{{baseUrl}}/v1/user/info",
    headers: { "Authorization": "Bearer {{apiKey}}" }
  };
}

function extract(resp) {
  return { remaining: resp.data.balance, unit: "CNY" };
}`;

type Tab = "native" | "template" | "script";

/** invoke 抛出的错误若为后端 reject 的 TemplateErrorDto 则还原形状，否则 null。 */
function toTemplateError(e: unknown): TemplateErrorDto | null {
  if (typeof e === "object" && e != null && "field" in e && "reason" in e) {
    return e as TemplateErrorDto;
  }
  return null;
}

const inputCls = "qt-input";
const labelCls = "qt-field-label";

export function EditDialog({ open, initial, usageCurrency, onClose }: Props) {
  const qc = useQueryClient();
  const { t } = useLang();
  const natives = useNativeMetas();
  const [tab, setTab] = useState<Tab>(
    initial?.kind.type === "template"
      ? "template"
      : initial?.kind.type === "script"
        ? "script"
        : "native",
  );
  const [name, setName] = useState(initial?.name ?? "");
  const [nativeProvider, setNativeProvider] = useState(
    initial?.kind.type === "native" ? initial.kind.provider : "",
  );
  const [planVariant, setPlanVariant] = useState<PlanVariant>(
    initial?.plan_variant ?? "auto",
  );
  const [templateJson, setTemplateJson] = useState(() => {
    if (initial?.kind.type !== "template") return DEFAULT_TEMPLATE;
    const { type: _type, ...rest } = initial.kind;
    return JSON.stringify(rest, null, 2);
  });
  const [scriptCode, setScriptCode] = useState(
    initial?.kind.type === "script" ? initial.kind.code : DEFAULT_SCRIPT,
  );
  const [scriptInsecure, setScriptInsecure] = useState(
    initial?.kind.type === "script" ? (initial.kind.allowInsecure ?? false) : false,
  );
  const [baseUrl, setBaseUrl] = useState(initial?.base_url ?? "");
  const [apiKey, setApiKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  // id 在打开期间保持稳定（新增时生成一次）
  const id = useMemo(() => initial?.id ?? newEntryId(), [initial]);
  const configured = Boolean(initial?.api_key_enc);

  // 峰谷定价：ref 收集（PricingSection 内聚草稿态，mount/变更时上报）
  const pricingRef = useRef<PricingConfig | undefined>(undefined);
  const initialTab =
    initial?.kind.type === "template"
      ? "template"
      : initial?.kind.type === "script"
        ? "script"
        : "native";
  const selectedNativeMeta = useMemo(() => {
    if (tab !== "native") return null;
    return natives.data?.find((m) => m.id === nativeProvider) ?? null;
  }, [tab, nativeProvider, natives.data]);
  const selectedPreset = (
    usageCurrency
      ? selectedNativeMeta?.pricing_by_currency?.[usageCurrency.trim().toUpperCase()]
      : undefined
  ) ?? selectedNativeMeta?.pricing ?? null;

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
      } else if (tab === "script") {
        // 保存前干跑校验（假变量替换 + request() 产物形状），错误带字段定位
        const configJson = JSON.stringify({
          code: scriptCode,
          allowInsecure: scriptInsecure,
        });
        try {
          await api.validateScript(configJson);
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
        kind = { type: "script", code: scriptCode, allowInsecure: scriptInsecure };
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
        // template/script 采表单值；native 条目无表单，保留既有值（native 查询不消费它）
        base_url:
          tab === "template" || tab === "script"
            ? (baseUrl.trim() || undefined)
            : initial?.base_url,
        pricing: pricingRef.current,
        // 智谱系订阅套餐的限额窗口声明；非订阅平台后端忽略
        plan_variant: planVariant,
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
    <DialogShell
      title={initial ? t("edit.titleEdit") : t("edit.titleAdd")}
      description={initial?.name ?? t("edit.descriptionAdd")}
      onClose={onClose}
      closeLabel={t("titlebar.close")}
      size="lg"
      footer={
        <>
          <Button onClick={onClose}>{t("common.cancel")}</Button>
          <Button
            variant="primary"
            type="submit"
            form="provider-edit-form"
            disabled={save.isPending}
          >
            {save.isPending ? t("common.saving") : t("common.save")}
          </Button>
        </>
      }
    >
      <div className="qt-edit-tabs">
        {(["native", "template", "script"] as Tab[]).map((tabId) => (
          <button
            type="button"
            key={tabId}
            aria-pressed={tab === tabId}
            onClick={() => setTab(tabId)}
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
        id="provider-edit-form"
        className="qt-edit-form"
        onSubmit={(event) => {
          event.preventDefault();
          save.mutate();
        }}
      >
        <div className="qt-edit-basics">
          <label className="qt-field">
            <span>{t("edit.name")}</span>
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t("edit.namePlaceholder")}
              className={inputCls}
            />
          </label>

          {tab === "native" && (
            <div className="qt-field">
              <span>{t("edit.platform")}</span>
              <NativeProviderPicker
                metas={natives.data ?? []}
                value={nativeProvider}
                ariaLabel={t("edit.platform")}
                placeholder={t("edit.platformPlaceholder")}
                groupLabels={{
                  deepseek: "DeepSeek",
                  siliconflow: "SiliconFlow",
                  openrouter: "OpenRouter",
                  kimi: "Kimi",
                  zhipu: t("edit.platformGroupZhipu"),
                  zai: "Z.ai",
                }}
                onChange={setNativeProvider}
              />
            </div>
          )}

          {tab === "native" && selectedNativeMeta?.supports_plan_variant && (
            <label className="qt-field">
              <span>{t("edit.planVariant")}</span>
              <select
                value={planVariant}
                onChange={(event) => setPlanVariant(event.target.value as PlanVariant)}
                className={inputCls}
              >
                <option value="auto">{t("edit.planVariantAuto")}</option>
                <option value="no_weekly">{t("edit.planVariantNoWeekly")}</option>
                <option value="weekly">{t("edit.planVariantWeekly")}</option>
              </select>
            </label>
          )}
        </div>

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

        {tab === "script" && (
          <ScriptForm
            code={scriptCode}
            setCode={setScriptCode}
            allowInsecure={scriptInsecure}
            setAllowInsecure={setScriptInsecure}
            baseUrl={baseUrl}
            setBaseUrl={setBaseUrl}
            apiKey={apiKey}
          />
        )}

        <PricingSection
          key={`${tab}:${nativeProvider}`}
          preset={selectedPreset}
          customModels={selectedNativeMeta?.custom_models ?? []}
          initial={tab === initialTab ? initial?.pricing : undefined}
          onChange={(pricing) => {
            pricingRef.current = pricing;
          }}
        />

        <label className="qt-field qt-credential-field">
          <span>{t("edit.apiKey")}</span>
          <small>{t("edit.apiKeyHint")}</small>
          <input
            type="password"
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
            autoComplete="new-password"
            placeholder={configured ? t("edit.keyConfigured") : t("edit.keyMissing")}
            className={inputCls}
          />
        </label>

        {error && <p className="qt-inline-error">{error}</p>}
      </form>
    </DialogShell>
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
    <div className="qt-template-form">
      {/"allowInsecure"\s*:\s*true/.test(props.templateJson) && (
        <p className="qt-inline-warning">{t("edit.insecureWarn")}</p>
      )}
      <label className="qt-field">
        <span className={labelCls}>{t("edit.templateJson")}</span>
        <div className="qt-code-editor">
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

      <label className="qt-field">
        <span className={labelCls}>{t("edit.baseUrl")}</span>
        <input
          value={props.baseUrl}
          onChange={(e) => props.setBaseUrl(e.target.value)}
          placeholder="https://api.example.com"
          className={inputCls}
        />
      </label>

      <div className="qt-template-actions">
        <Button type="button" onClick={() => validate.mutate()} disabled={validate.isPending}>
          {t("edit.validate")}
        </Button>
        <Button type="button" onClick={() => void test()} disabled={testing}>
          {testing ? t("edit.testing") : t("edit.test")}
        </Button>
        {validateOk && !validateMsg && (
          <span className="qt-text-success">{t("edit.validated")}</span>
        )}
      </div>

      {validateMsg && <p className="qt-inline-error">{validateMsg}</p>}

      {testResult && (
        <div className="qt-template-result">
          {testResult.ok ? (
            <div>
              <p className="qt-text-success">{t("edit.testOk")}</p>
              {(testResult.data ?? []).map((d, i) => (
                <p key={i}>
                  {d.plan_name ? `${d.plan_name} · ` : ""}
                  {dataSummary(d, lang)}
                </p>
              ))}
            </div>
          ) : (
            <p className={testResult.error?.kind === "transient" ? "qt-text-subdued" : "qt-text-danger"}>
              {testResult.error?.message}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/** 脚本形态：JS 编辑器 + baseUrl + allowInsecure + 校验/试查（镜像 TemplateForm 骨架）。 */
function ScriptForm(props: {
  code: string;
  setCode: (v: string) => void;
  allowInsecure: boolean;
  setAllowInsecure: (v: boolean) => void;
  baseUrl: string;
  setBaseUrl: (v: string) => void;
  apiKey: string;
}) {
  const { t, lang } = useLang();
  const [validateMsg, setValidateMsg] = useState<string | null>(null);
  const [validateOk, setValidateOk] = useState(false);
  const [testResult, setTestResult] = useState<QueryOutcome | null>(null);
  const [testing, setTesting] = useState(false);

  // 后端 IPC 形状：ScriptConfig JSON（code + allowInsecure）
  const configJson = JSON.stringify({
    code: props.code,
    allowInsecure: props.allowInsecure,
  });

  const validate = useMutation({
    mutationFn: () => api.validateScript(configJson),
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
      const r = await api.testScript(
        configJson,
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
    <div className="qt-template-form">
      {props.allowInsecure && (
        <p className="qt-inline-warning">{t("edit.insecureWarn")}</p>
      )}
      <label className="qt-field">
        <span className={labelCls}>{t("edit.scriptCode")}</span>
        <div className="qt-code-editor">
          <CodeMirror
            value={props.code}
            onChange={(v) => {
              props.setCode(v);
              // 内容已变，旧校验结论作废
              setValidateOk(false);
              setValidateMsg(null);
            }}
            extensions={[javascript()]}
            height="180px"
            basicSetup={{ foldGutter: false, autocompletion: false }}
          />
        </div>
      </label>

      <label className="qt-field">
        <span className={labelCls}>{t("edit.baseUrl")}</span>
        <input
          value={props.baseUrl}
          onChange={(e) => props.setBaseUrl(e.target.value)}
          placeholder="https://api.example.com"
          className={inputCls}
        />
      </label>

      <label className="qt-field">
        <span className={labelCls}>{t("edit.allowInsecure")}</span>
        <input
          type="checkbox"
          checked={props.allowInsecure}
          onChange={(e) => props.setAllowInsecure(e.target.checked)}
        />
      </label>

      <div className="qt-template-actions">
        <Button type="button" onClick={() => validate.mutate()} disabled={validate.isPending}>
          {t("edit.validate")}
        </Button>
        <Button type="button" onClick={() => void test()} disabled={testing}>
          {testing ? t("edit.testing") : t("edit.test")}
        </Button>
        {validateOk && !validateMsg && (
          <span className="qt-text-success">{t("edit.validated")}</span>
        )}
      </div>

      {validateMsg && <p className="qt-inline-error">{validateMsg}</p>}

      {testResult && (
        <div className="qt-template-result">
          {testResult.ok ? (
            <div>
              <p className="qt-text-success">{t("edit.testOk")}</p>
              {(testResult.data ?? []).map((d, i) => (
                <p key={i}>
                  {d.plan_name ? `${d.plan_name} · ` : ""}
                  {dataSummary(d, lang)}
                </p>
              ))}
            </div>
          ) : (
            <p className={testResult.error?.kind === "transient" ? "qt-text-subdued" : "qt-text-danger"}>
              {testResult.error?.message}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
