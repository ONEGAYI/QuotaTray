// 添加/编辑对话框：native / template / script(M4 预留) 三形态。
// 红线 3：key 框初始为空（占位符「已配置/未配置」），空 = 保持不变，永不回显明文。
// CodeMirror 编辑器为第三方亮色主题，dark 模式下仅调整容器边框（后续版本可换主题）。
import CodeMirror from "@uiw/react-codemirror";
import { json } from "@codemirror/lang-json";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, newEntryId } from "../api";
import { dataSummary } from "../display";
import { useLang } from "../i18n";
import { useNativeMetas } from "../queries";
import type {
  PeakWindow,
  PresetPricing,
  PriceTier,
  PricingConfig,
  ProviderEntry,
  ProviderKind,
  QueryOutcome,
  TemplateErrorDto,
  Weekday,
} from "../types";

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

// ---- 峰谷定价区块（草稿态 + 组装上报；空字段 = 回退预置） ------------------

const WEEKDAYS: Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/** 自定义草稿（价格/偏移以文本态暂存，组装时再转数值）。 */
interface PricingDraft {
  custom: boolean;
  model: string;
  tz: string;
  currency: string;
  windows: PeakWindow[];
  peakHit: string;
  peakMiss: string;
  peakOut: string;
  offHit: string;
  offMiss: string;
  offOut: string;
}

/** 「完整自定义」判定：model-only（预置平台的模型选择）不算自定义。 */
function isFullCustom(p: PricingConfig | undefined, preset: PresetPricing | null): boolean {
  if (p == null) return false;
  return (
    p.windows != null ||
    p.peak != null ||
    p.off_peak != null ||
    p.currency != null ||
    p.timezone_offset_minutes != null ||
    (!preset && p.model != null)
  );
}

function draftFrom(p: PricingConfig | undefined, preset: PresetPricing | null): PricingDraft {
  const tierStr = (t: PriceTier | undefined, k: keyof PriceTier) =>
    t?.[k] == null ? "" : String(t[k]);
  return {
    custom: isFullCustom(p, preset),
    model: p?.model ?? "",
    tz: p?.timezone_offset_minutes == null ? "" : String(p.timezone_offset_minutes),
    currency: p?.currency ?? "",
    windows: p?.windows?.map((w) => ({ days: [...w.days], start: w.start, end: w.end })) ?? [],
    peakHit: tierStr(p?.peak, "cache_hit_input"),
    peakMiss: tierStr(p?.peak, "cache_miss_input"),
    peakOut: tierStr(p?.peak, "output"),
    offHit: tierStr(p?.off_peak, "cache_hit_input"),
    offMiss: tierStr(p?.off_peak, "cache_miss_input"),
    offOut: tierStr(p?.off_peak, "output"),
  };
}

/** 草稿 → 保存的 PricingConfig：空字段省略（= 回退预置）；
 *  非自定义模式下仅模型选择（= 预置默认时整体省略）。 */
function buildPricing(d: PricingDraft, preset: PresetPricing | null): PricingConfig | undefined {
  const cfg: PricingConfig = {};
  const model = d.model.trim();
  if (model && !(preset != null && model.toLowerCase() === preset.default_model.toLowerCase())) {
    cfg.model = model;
  }
  if (!d.custom) {
    return Object.keys(cfg).length ? cfg : undefined;
  }
  const tz = d.tz.trim();
  if (tz !== "" && Number.isFinite(Number(tz))) cfg.timezone_offset_minutes = Number(tz);
  if (d.currency.trim()) cfg.currency = d.currency.trim();
  const wins = d.windows.filter((w) => w.days.length > 0 && w.start && w.end);
  if (wins.length > 0) cfg.windows = wins;
  const tier = (hit: string, miss: string, out: string): PriceTier | undefined => {
    const t: PriceTier = {};
    const entries: [keyof PriceTier, string][] = [
      ["cache_hit_input", hit],
      ["cache_miss_input", miss],
      ["output", out],
    ];
    for (const [k, s] of entries) {
      const v = s.trim();
      if (v !== "" && Number.isFinite(Number(v))) t[k] = Number(v);
    }
    return Object.keys(t).length ? t : undefined;
  };
  const peak = tier(d.peakHit, d.peakMiss, d.peakOut);
  const off = tier(d.offHit, d.offMiss, d.offOut);
  if (peak) cfg.peak = peak;
  if (off) cfg.off_peak = off;
  return Object.keys(cfg).length ? cfg : undefined;
}

/** 价格展示（与 core format_price 同规则：最多 2 位小数去尾零）。 */
function fmtPrice(v: number | undefined): string {
  return v == null ? "—" : String(parseFloat(v.toFixed(2)));
}

function PricingSection(props: {
  preset: PresetPricing | null;
  initial: PricingConfig | undefined;
  onChange: (p: PricingConfig | undefined) => void;
}) {
  const { t } = useLang();
  const [draft, setDraft] = useState(() => draftFrom(props.initial, props.preset));

  const patch = (p: Partial<PricingDraft>) => {
    const next = { ...draft, ...p };
    setDraft(next);
    props.onChange(buildPricing(next, props.preset));
  };
  // mount（含平台切换后重建）时上报一次初始组装，保证收集方与草稿一致
  useEffectOnce(() => props.onChange(buildPricing(draft, props.preset)));

  const presetModel =
    props.preset?.models.find(
      (m) => m.id.toLowerCase() === draft.model.trim().toLowerCase(),
    ) ?? props.preset?.models.find((m) => m.id === props.preset?.default_model);

  const fillFromPreset = () => {
    if (!props.preset) return;
    const num = (v: number | undefined) => (v == null ? "" : String(v));
    patch({
      custom: true,
      tz: String(props.preset.timezone_offset_minutes),
      currency: props.preset.currency,
      windows: props.preset.windows.map((w) => ({ days: [...w.days], start: w.start, end: w.end })),
      peakHit: num(presetModel?.peak.cache_hit_input),
      peakMiss: num(presetModel?.peak.cache_miss_input),
      peakOut: num(presetModel?.peak.output),
      offHit: num(presetModel?.off_peak.cache_hit_input),
      offMiss: num(presetModel?.off_peak.cache_miss_input),
      offOut: num(presetModel?.off_peak.output),
    });
  };

  const toggleDay = (wi: number, day: Weekday) => {
    const wins = draft.windows.map((w, i) => {
      if (i !== wi) return w;
      const days = w.days.includes(day)
        ? w.days.filter((d) => d !== day)
        : [...w.days, day];
      return { ...w, days };
    });
    patch({ windows: wins });
  };

  const smallInputCls =
    "rounded border border-slate-300 bg-white px-2 py-1 text-xs dark:border-slate-600 dark:bg-slate-900 dark:text-slate-100";

  return (
    <fieldset className="space-y-2 rounded border border-slate-200 px-3 py-2 dark:border-slate-700">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium">{t("pricing.section")}</span>
        <span className="text-xs text-slate-400">{t("pricing.unit")}</span>
      </div>

      {props.preset && (
        <div className="space-y-1 rounded bg-slate-50 px-3 py-2 text-sm dark:bg-slate-900/60">
          <p className="text-xs text-slate-500 dark:text-slate-400">{t("pricing.presetTitle")}</p>
          <label className="flex items-center gap-2">
            <span className={labelCls}>{t("pricing.presetModel")}</span>
            <select
              value={props.preset.models.some(
                (m) => m.id.toLowerCase() === draft.model.trim().toLowerCase(),
              )
                ? draft.model.trim()
                : ""}
              onChange={(e) => patch({ model: e.target.value })}
              className={smallInputCls}
            >
              {props.preset.models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.display}
                  {m.id === props.preset?.default_model ? t("pricing.presetDefault") : ""}
                </option>
              ))}
            </select>
          </label>
          {presetModel && (
            <div className="text-xs text-slate-600 dark:text-slate-300">
              <p>
                {t("pricing.peak")}：{t("pricing.hit")} {fmtPrice(presetModel.peak.cache_hit_input)} ·{" "}
                {t("pricing.miss")} {fmtPrice(presetModel.peak.cache_miss_input)} ·{" "}
                {t("pricing.out")} {fmtPrice(presetModel.peak.output)}
              </p>
              <p>
                {t("pricing.offPeak")}：{t("pricing.hit")}{" "}
                {fmtPrice(presetModel.off_peak.cache_hit_input)} · {t("pricing.miss")}{" "}
                {fmtPrice(presetModel.off_peak.cache_miss_input)} · {t("pricing.out")}{" "}
                {fmtPrice(presetModel.off_peak.output)}
              </p>
            </div>
          )}
        </div>
      )}

      {!props.preset && !draft.custom && (
        <p className="text-xs text-slate-500 dark:text-slate-400">{t("pricing.noPreset")}</p>
      )}

      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={draft.custom}
          onChange={(e) => patch({ custom: e.target.checked })}
        />
        <span className={labelCls}>{t("pricing.custom")}</span>
      </label>

      {draft.custom && (
        <div className="space-y-2">
          {props.preset && (
            <button
              type="button"
              onClick={fillFromPreset}
              className="rounded border border-slate-300 px-2 py-1 text-xs hover:bg-slate-50 dark:border-slate-600 dark:text-slate-200 dark:hover:bg-slate-700"
            >
              {t("pricing.fillPreset")}
            </button>
          )}

          <div className="space-y-1">
            <p className={labelCls}>{t("pricing.windows")}</p>
            {draft.windows.map((w, wi) => (
              <div
                key={wi}
                className="flex flex-wrap items-center gap-2 rounded border border-slate-200 px-2 py-1.5 text-xs dark:border-slate-700"
              >
                {WEEKDAYS.map((day) => (
                  <label key={day} className="flex items-center gap-0.5">
                    <input
                      type="checkbox"
                      checked={w.days.includes(day)}
                      onChange={() => toggleDay(wi, day)}
                    />
                    {t(`pricing.day.${day}`)}
                  </label>
                ))}
                <input
                  type="time"
                  value={w.start}
                  onChange={(e) =>
                    patch({
                      windows: draft.windows.map((x, i) =>
                        i === wi ? { ...x, start: e.target.value } : x,
                      ),
                    })
                  }
                  className={smallInputCls}
                  aria-label={t("pricing.start")}
                />
                <span>–</span>
                <input
                  type="time"
                  value={w.end}
                  onChange={(e) =>
                    patch({
                      windows: draft.windows.map((x, i) =>
                        i === wi ? { ...x, end: e.target.value } : x,
                      ),
                    })
                  }
                  className={smallInputCls}
                  aria-label={t("pricing.end")}
                />
                <button
                  type="button"
                  onClick={() => patch({ windows: draft.windows.filter((_, i) => i !== wi) })}
                  className="ml-auto text-slate-400 hover:text-red-500"
                  title={t("pricing.removeWindow")}
                >
                  ✕
                </button>
              </div>
            ))}
            <button
              type="button"
              onClick={() =>
                patch({
                  windows: [
                    ...draft.windows,
                    { days: ["mon", "tue", "wed", "thu", "fri"], start: "09:00", end: "12:00" },
                  ],
                })
              }
              className="text-xs text-indigo-600 hover:underline dark:text-indigo-400"
            >
              {t("pricing.addWindow")}
            </button>
          </div>

          <div className="grid grid-cols-2 gap-2">
            <label className="block">
              <span className={labelCls}>{t("pricing.tz")}</span>
              <input
                type="number"
                value={draft.tz}
                onChange={(e) => patch({ tz: e.target.value })}
                placeholder="480"
                className={inputCls}
              />
            </label>
            <label className="block">
              <span className={labelCls}>{t("pricing.currency")}</span>
              <input
                value={draft.currency}
                onChange={(e) => patch({ currency: e.target.value })}
                placeholder="CNY"
                className={inputCls}
              />
            </label>
          </div>

          <label className="block">
            <span className={labelCls}>{t("pricing.modelTag")}</span>
            <input
              value={draft.model}
              onChange={(e) => patch({ model: e.target.value })}
              placeholder={props.preset?.default_model ?? ""}
              className={inputCls}
            />
          </label>

          <div className="space-y-1">
            <div className="grid grid-cols-[1fr_repeat(3,1fr)] gap-2 text-xs">
              <span />
              <span className={labelCls}>{t("pricing.hit")}</span>
              <span className={labelCls}>{t("pricing.miss")}</span>
              <span className={labelCls}>{t("pricing.out")}</span>

              <span className={labelCls}>{t("pricing.peak")}</span>
              {(
                [
                  ["peakHit", "peak"],
                  ["peakMiss", "peak"],
                  ["peakOut", "peak"],
                ] as const
              ).map(([key]) => (
                <input
                  key={key}
                  type="number"
                  step="any"
                  min="0"
                  value={draft[key]}
                  onChange={(e) => patch({ [key]: e.target.value })}
                  className={smallInputCls}
                />
              ))}

              <span className={labelCls}>{t("pricing.offPeak")}</span>
              {(["offHit", "offMiss", "offOut"] as const).map((key) => (
                <input
                  key={key}
                  type="number"
                  step="any"
                  min="0"
                  value={draft[key]}
                  onChange={(e) => patch({ [key]: e.target.value })}
                  className={smallInputCls}
                />
              ))}
            </div>
          </div>
        </div>
      )}
    </fieldset>
  );
}

/** mount-only 副作用（平台切换后组件重建会重新执行）。 */
function useEffectOnce(fn: () => void) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => ref.current(), []);
}

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

  // 峰谷定价：ref 收集（PricingSection 内聚草稿态，mount/变更时上报）
  const pricingRef = useRef<PricingConfig | undefined>(undefined);
  const initialTab = initial?.kind.type === "template" ? "template" : "native";
  const selectedPreset = useMemo(() => {
    if (tab !== "native") return null;
    return natives.data?.find((m) => m.id === nativeProvider)?.pricing ?? null;
  }, [tab, nativeProvider, natives.data]);

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
        pricing: pricingRef.current,
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

          <PricingSection
            key={`${tab}:${nativeProvider}`}
            preset={selectedPreset}
            initial={tab === initialTab ? initial?.pricing : undefined}
            onChange={(p) => {
              pricingRef.current = p;
            }}
          />

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
