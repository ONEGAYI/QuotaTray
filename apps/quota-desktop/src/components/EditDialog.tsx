// 添加/编辑对话框：native / template / script(M4 预留) 三形态。
// 红线 3：key 框初始为空（占位符「已配置/未配置」），空 = 保持不变，永不回显明文。
import CodeMirror from "@uiw/react-codemirror";
import { json } from "@codemirror/lang-json";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { api, newEntryId } from "../api";
import { dataSummary } from "../display";
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

export function EditDialog({ open, initial, onClose }: Props) {
  const qc = useQueryClient();
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
      if (!trimmedName) throw new Error("名称不能为空");
      let kind: ProviderKind;
      if (tab === "native") {
        if (!nativeProvider) throw new Error("请选择预置平台");
        kind = { type: "native", provider: nativeProvider };
      } else {
        // 保存前先静态校验（结构 + 规则），错误带字段定位
        try {
          await api.validateTemplate(templateJson);
        } catch (e) {
          const dto = toTemplateError(e);
          throw new Error(
            dto ? (dto.field === "(json)" ? `模板 JSON 解析失败：${dto.reason}` : `${dto.field}：${dto.reason}`) : String(e),
          );
        }
        let parsed: unknown;
        try {
          parsed = JSON.parse(templateJson);
        } catch {
          throw new Error("模板 JSON 解析失败");
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
      <div className="flex max-h-full w-full max-w-xl flex-col overflow-hidden rounded-lg bg-white shadow-xl">
        <div className="border-b border-slate-200 px-5 py-3">
          <h2 className="font-medium">{initial ? "编辑供应商" : "添加供应商"}</h2>
        </div>

        <div className="flex gap-1 border-b border-slate-200 px-5 pt-2">
          {(["native", "template", "script"] as Tab[]).map((t) => (
            <button
              key={t}
              disabled={t === "script"}
              onClick={() => setTab(t)}
              className={`rounded-t px-3 py-1.5 text-sm disabled:cursor-not-allowed disabled:opacity-40 ${
                tab === t ? "border border-b-white border-slate-200 bg-white font-medium" : "text-slate-500 hover:bg-slate-50"
              }`}
            >
              {t === "native" ? "预置平台" : t === "template" ? "模板" : "脚本（M4）"}
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
            <span className="text-sm text-slate-600">名称</span>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="如 DeepSeek 主号"
              className="mt-1 w-full rounded border border-slate-300 px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none"
            />
          </label>

          {tab === "native" && (
            <label className="block">
              <span className="text-sm text-slate-600">平台</span>
              <select
                value={nativeProvider}
                onChange={(e) => setNativeProvider(e.target.value)}
                className="mt-1 w-full rounded border border-slate-300 px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none"
              >
                <option value="">请选择…</option>
                {(natives.data ?? []).map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}（{m.id}）
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
            <span className="text-sm text-slate-600">API key</span>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              autoComplete="new-password"
              placeholder={configured ? "已配置（留空保持不变）" : "未配置"}
              className="mt-1 w-full rounded border border-slate-300 px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none"
            />
          </label>

          {error && (
            <p className="rounded bg-red-50 px-3 py-2 text-sm text-red-600">{error}</p>
          )}
        </form>

        <div className="flex justify-end gap-2 border-t border-slate-200 px-5 py-3">
          <button onClick={onClose} className="rounded px-4 py-1.5 text-sm text-slate-600 hover:bg-slate-100">
            取消
          </button>
          <button
            onClick={() => save.mutate()}
            disabled={save.isPending}
            className="rounded bg-indigo-600 px-4 py-1.5 text-sm text-white hover:bg-indigo-500 disabled:opacity-50"
          >
            {save.isPending ? "保存中…" : "保存"}
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
        dto ? (dto.field === "(json)" ? `JSON 解析失败：${dto.reason}` : `${dto.field}：${dto.reason}`) : String(e),
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
        <p className="rounded bg-amber-50 px-3 py-2 text-xs text-amber-700">
          ⚠ 模板启用了 allowInsecure：请求可经明文 http 传输，API key 存在被网络窃听的风险
        </p>
      )}
      <label className="block">
        <span className="text-sm text-slate-600">模板 JSON</span>
        <div className="mt-1 overflow-hidden rounded border border-slate-300">
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
        <span className="text-sm text-slate-600">baseUrl（模板 {'{{baseUrl}}'} 变量来源）</span>
        <input
          value={props.baseUrl}
          onChange={(e) => props.setBaseUrl(e.target.value)}
          placeholder="https://api.example.com"
          className="mt-1 w-full rounded border border-slate-300 px-2 py-1.5 text-sm focus:border-indigo-400 focus:outline-none"
        />
      </label>

      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => validate.mutate()}
          disabled={validate.isPending}
          className="rounded border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-50 disabled:opacity-50"
        >
          校验
        </button>
        <button
          type="button"
          onClick={() => void test()}
          disabled={testing}
          className="rounded border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-50 disabled:opacity-50"
        >
          {testing ? "试查中…" : "试查"}
        </button>
        {validateOk && !validateMsg && <span className="text-xs text-green-600">校验通过</span>}
      </div>

      {validateMsg && (
        <p className="rounded bg-red-50 px-3 py-2 text-sm text-red-600">{validateMsg}</p>
      )}

      {testResult && (
        <div className="rounded bg-slate-50 p-3 text-sm">
          {testResult.ok ? (
            <div>
              <p className="mb-1 text-xs text-green-600">试查成功</p>
              {(testResult.data ?? []).map((d, i) => (
                <p key={i}>
                  {d.plan_name ? `${d.plan_name} · ` : ""}
                  {dataSummary(d)}
                </p>
              ))}
            </div>
          ) : (
            <p className={testResult.error?.kind === "transient" ? "text-slate-600" : "text-red-600"}>
              {testResult.error?.kind === "transient" ? "⟳" : "⚠"} {testResult.error?.message}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
