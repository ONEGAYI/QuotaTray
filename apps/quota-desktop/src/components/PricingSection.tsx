import { useEffect, useRef, useState, type ReactNode } from "react";
import { HelpCircle } from "lucide-react";
import { useLang } from "../i18n";
import type {
  PeakWindow,
  PresetModel,
  PresetPricing,
  PriceTier,
  PricingConfig,
  Weekday,
} from "../types";
import {
  buildPricing,
  draftFrom,
  formatPrice,
  formatUtcOffset,
  selectedPresetModel,
  type PricingDraft,
} from "./pricingDraft";
import { Badge, Tooltip } from "./ui";

const WEEKDAYS: Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const fieldCls = "qt-input";
const compactFieldCls = "qt-input qt-input-compact";
const subduedTextCls = "qt-text-subdued";

interface Props {
  preset: PresetPricing | null;
  initial: PricingConfig | undefined;
  onChange: (pricing: PricingConfig | undefined) => void;
}

export function PricingSection(props: Props) {
  const { t } = useLang();
  const [draft, setDraft] = useState(() => draftFrom(props.initial, props.preset));
  const onChangeRef = useRef(props.onChange);
  onChangeRef.current = props.onChange;

  useEffect(() => {
    onChangeRef.current(buildPricing(draft, props.preset));
  }, [draft, props.preset]);

  const patch = (partial: Partial<PricingDraft>) => {
    setDraft((current) => ({ ...current, ...partial }));
  };

  const presetModel = selectedPresetModel(props.preset, draft.model);
  const modelIsKnown =
    props.preset?.models.some(
      (model) => model.id.toLowerCase() === draft.model.trim().toLowerCase(),
    ) ?? false;
  const selectedModelId = modelIsKnown
    ? draft.model.trim()
    : draft.model.trim() || props.preset?.default_model || "";
  const modelLabel =
    (!modelIsKnown && draft.model.trim()) || presetModel?.display || t("pricing.customModel");

  const setMode = (custom: boolean) => {
    patch({
      custom,
      scheduleCustom: custom && !props.preset ? true : draft.scheduleCustom,
    });
  };

  const activateCustomSchedule = () => {
    const presetWindows = props.preset?.windows.map((window) => ({
      days: [...window.days],
      start: window.start,
      end: window.end,
    }));
    patch({
      scheduleCustom: true,
      windows: draft.windows.length > 0 ? draft.windows : (presetWindows ?? []),
    });
  };

  const clearOverrides = () => {
    patch({
      custom: false,
      scheduleCustom: false,
      model: props.preset ? draft.model : "",
      tz: "",
      currency: "",
      windows: [],
      peakHit: "",
      peakMiss: "",
      peakOut: "",
      offHit: "",
      offMiss: "",
      offOut: "",
    });
  };

  const updateWindows = (windows: PeakWindow[]) => patch({ windows });

  return (
    <fieldset className="qt-pricing-section">
      <legend className="sr-only">{t("pricing.section")}</legend>

      <div className="qt-pricing-section-header">
        <div className="min-w-0">
          <h3>
            {t("pricing.section")}
          </h3>
          <p className={subduedTextCls}>
            {draft.custom
              ? t("pricing.summaryCustom", { model: modelLabel })
              : props.preset
                ? t("pricing.summaryPreset", {
                    model: modelLabel,
                    timezone: formatUtcOffset(props.preset.timezone_offset_minutes),
                    currency: props.preset.currency,
                  })
                : t("pricing.summaryOff")}
          </p>
        </div>
        <Badge tone={draft.custom ? "accent" : "neutral"}>
          {draft.custom
            ? t("pricing.statusCustom")
            : props.preset
              ? t("pricing.statusPreset")
              : t("pricing.statusOff")}
        </Badge>
      </div>

      <div className="qt-pricing-mode">
        <ModeButton active={!draft.custom} onClick={() => setMode(false)}>
          {props.preset ? t("pricing.modePreset") : t("pricing.modeOff")}
        </ModeButton>
        <ModeButton active={draft.custom} onClick={() => setMode(true)}>
          {props.preset ? t("pricing.modeCustom") : t("pricing.modeConfigure")}
        </ModeButton>
      </div>

      {props.preset && (
        <div className="qt-pricing-model-row">
          <label htmlFor="pricing-model">
            {t("pricing.presetModel")}
          </label>
          <select
            id="pricing-model"
            value={selectedModelId}
            onChange={(event) => patch({ model: event.target.value })}
            className={compactFieldCls}
          >
            {!modelIsKnown && draft.model.trim() && (
              <option value={draft.model.trim()}>{draft.model.trim()}</option>
            )}
            {props.preset.models.map((model) => (
              <option key={model.id} value={model.id}>
                {model.display}
                {model.id === props.preset?.default_model ? t("pricing.presetDefault") : ""}
              </option>
            ))}
          </select>
          <span className={`${subduedTextCls} sm:text-right`}>{t("pricing.presetNote")}</span>
        </div>
      )}

      {!draft.custom && props.preset && presetModel && (
        <PresetPreview preset={props.preset} model={presetModel} />
      )}

      {draft.custom && (
        <div>
          {props.preset && (
            <div className="qt-inherit-banner">
              <span>{t("pricing.inheritNote")}</span>
              <button type="button" onClick={clearOverrides} className="font-medium hover:underline">
                {t("pricing.clearOverrides")}
              </button>
            </div>
          )}

          <section className="px-4 py-4" aria-labelledby="pricing-windows-heading">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <div>
                <h3 id="pricing-windows-heading" className="text-sm font-medium text-slate-900 dark:text-slate-100">
                  {t("pricing.windowsTitle")}
                </h3>
                <p className={`mt-1 ${subduedTextCls}`}>{t("pricing.windowsHint")}</p>
              </div>
              {props.preset && (
                <div className="grid shrink-0 grid-cols-2 gap-1 rounded-lg bg-slate-100 p-1 dark:bg-slate-900/70">
                  <ModeButton
                    active={!draft.scheduleCustom}
                    compact
                    onClick={() => patch({ scheduleCustom: false })}
                  >
                    {t("pricing.schedulePreset")}
                  </ModeButton>
                  <ModeButton active={draft.scheduleCustom} compact onClick={activateCustomSchedule}>
                    {t("pricing.scheduleCustom")}
                  </ModeButton>
                </div>
              )}
            </div>

            <div className="mt-4 grid gap-2 sm:grid-cols-[7rem_minmax(12rem,18rem)_1fr] sm:items-center">
              <label htmlFor="pricing-timezone" className="text-sm font-medium text-slate-700 dark:text-slate-200">
                {t("pricing.timezone")}
              </label>
              <input
                id="pricing-timezone"
                type="number"
                min={-840}
                max={840}
                value={draft.tz}
                onChange={(event) => patch({ tz: event.target.value })}
                placeholder={props.preset ? String(props.preset.timezone_offset_minutes) : ""}
                className={compactFieldCls}
              />
              <span className={subduedTextCls}>
                {draft.tz.trim()
                  ? t("pricing.timezoneValue", { value: formatUtcOffset(Number(draft.tz)) })
                  : props.preset
                    ? t("pricing.timezoneInherited", {
                        value: formatUtcOffset(props.preset.timezone_offset_minutes),
                      })
                    : t("pricing.timezoneLocal")}
              </span>
            </div>

            {draft.scheduleCustom && (
              <div className="mt-4 space-y-2.5">
                {draft.windows.length === 0 && (
                  <p className="rounded-lg border border-dashed border-slate-300 px-3 py-4 text-center text-xs text-slate-500 dark:border-slate-600 dark:text-slate-400">
                    {t("pricing.emptyWindows")}
                  </p>
                )}
                {draft.windows.map((window, index) => (
                  <WindowEditor
                    key={index}
                    index={index}
                    window={window}
                    onChange={(next) =>
                      updateWindows(
                        draft.windows.map((item, itemIndex) =>
                          itemIndex === index ? next : item,
                        ),
                      )
                    }
                    onRemove={() =>
                      updateWindows(draft.windows.filter((_, itemIndex) => itemIndex !== index))
                    }
                  />
                ))}
                <button
                  type="button"
                  onClick={() =>
                    updateWindows([
                      ...draft.windows,
                      {
                        days: ["mon", "tue", "wed", "thu", "fri"],
                        start: "09:00",
                        end: "12:00",
                      },
                    ])
                  }
                  className="py-1 text-xs font-medium text-indigo-600 hover:underline dark:text-indigo-400"
                >
                  {t("pricing.addWindow")}
                </button>
              </div>
            )}
          </section>

          <section className="border-t border-slate-200 px-4 py-4 dark:border-slate-700" aria-labelledby="pricing-price-heading">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
              <div>
                <h3 id="pricing-price-heading" className="text-sm font-medium text-slate-900 dark:text-slate-100">
                  {t("pricing.priceTitle")}
                </h3>
                <p className={`mt-1 ${subduedTextCls}`}>
                  {props.preset ? t("pricing.priceHint") : t("pricing.priceHintNoPreset")}
                </p>
              </div>
              <label className="grid gap-1 sm:grid-cols-[auto_7rem] sm:items-center sm:gap-2">
                <span className="text-sm font-medium text-slate-700 dark:text-slate-200">
                  {t("pricing.currency")}
                </span>
                <input
                  value={draft.currency}
                  onChange={(event) => patch({ currency: event.target.value })}
                  placeholder={props.preset ? t("pricing.inheritValue", { value: props.preset.currency }) : "CNY"}
                  className={compactFieldCls}
                />
              </label>
            </div>

            {!props.preset && (
              <label className="mt-4 block">
                <span className="text-sm font-medium text-slate-700 dark:text-slate-200">
                  {t("pricing.modelTag")}
                </span>
                <input
                  value={draft.model}
                  onChange={(event) => patch({ model: event.target.value })}
                  placeholder={t("pricing.customModelPlaceholder")}
                  className={`mt-1 ${fieldCls}`}
                />
              </label>
            )}

            <div className="mt-4 grid gap-3 md:grid-cols-2">
              <PriceTierEditor kind="peak" draft={draft} presetTier={presetModel?.peak} patch={patch} />
              <PriceTierEditor kind="off" draft={draft} presetTier={presetModel?.off_peak} patch={patch} />
            </div>
            <p className={`mt-3 text-right ${subduedTextCls}`}>{t("pricing.unit")}</p>
          </section>
        </div>
      )}
    </fieldset>
  );
}

function ModeButton(props: {
  active: boolean;
  compact?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={props.active}
      onClick={props.onClick}
      className={`rounded-md text-sm transition-colors ${props.compact ? "px-3 py-1.5" : "px-3 py-2"} ${
        props.active
          ? "bg-white font-medium text-indigo-700 shadow-sm dark:bg-slate-700 dark:text-indigo-300"
          : "text-slate-500 hover:text-slate-700 dark:text-slate-400 dark:hover:text-slate-200"
      }`}
    >
      {props.children}
    </button>
  );
}

function PresetPreview(props: { preset: PresetPricing; model: PresetModel }) {
  const { t } = useLang();
  return (
    <div className="qt-preset-preview">
      <PreviewItem
        label={t("pricing.windowsTitle")}
        value={t("pricing.windowCount", { count: props.preset.windows.length })}
      />
      <PreviewItem
        label={t("pricing.timezoneCurrency")}
        value={`${formatUtcOffset(props.preset.timezone_offset_minutes)} · ${props.preset.currency}`}
      />
      <PresetTierPreview label={t("pricing.peak")} tier={props.model.peak} />
      <PresetTierPreview label={t("pricing.offPeak")} tier={props.model.off_peak} />
    </div>
  );
}

function PreviewItem(props: { label: string; value: string }) {
  return (
    <div className="qt-preview-item">
      <span className={subduedTextCls}>{props.label}</span>
      <p>{props.value}</p>
    </div>
  );
}

function PresetTierPreview(props: { label: string; tier: PriceTier }) {
  const { t } = useLang();
  return (
    <div className="qt-preview-item qt-preview-tier">
      <span className={subduedTextCls}>{props.label}</span>
      <dl>
        <div><dt>{t("pricing.hit")}</dt><dd>{formatPrice(props.tier.cache_hit_input)}</dd></div>
        <div><dt>{t("pricing.miss")}</dt><dd>{formatPrice(props.tier.cache_miss_input)}</dd></div>
        <div><dt>{t("pricing.out")}</dt><dd>{formatPrice(props.tier.output)}</dd></div>
      </dl>
    </div>
  );
}

function WindowEditor(props: {
  index: number;
  window: PeakWindow;
  onChange: (window: PeakWindow) => void;
  onRemove: () => void;
}) {
  const { t } = useLang();
  const toggleDay = (day: Weekday) => {
    const days = props.window.days.includes(day)
      ? props.window.days.filter((item) => item !== day)
      : [...props.window.days, day];
    props.onChange({ ...props.window, days });
  };

  return (
    <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-3 dark:border-slate-700 dark:bg-slate-900/50">
      <div className="mb-2.5 flex items-center justify-between gap-3">
        <span className="text-xs font-medium text-slate-700 dark:text-slate-200">
          {t("pricing.windowN", { n: props.index + 1 })}
        </span>
        <button
          type="button"
          onClick={props.onRemove}
          className="text-xs text-slate-500 hover:text-red-600 dark:text-slate-400 dark:hover:text-red-400"
        >
          {t("pricing.removeWindow")}
        </button>
      </div>
      <div className="grid gap-3 lg:grid-cols-[minmax(18rem,1fr)_minmax(13rem,auto)] lg:items-end">
        <div className="grid grid-cols-4 gap-1.5 sm:grid-cols-7" aria-label={t("pricing.day")}>
          {WEEKDAYS.map((day) => {
            const active = props.window.days.includes(day);
            return (
              <button
                key={day}
                type="button"
                aria-pressed={active}
                onClick={() => toggleDay(day)}
                className={`rounded-md border px-2 py-1.5 text-xs ${
                  active
                    ? "border-indigo-400 bg-indigo-50 font-medium text-indigo-700 dark:border-indigo-500 dark:bg-indigo-950/70 dark:text-indigo-300"
                    : "border-slate-200 bg-white text-slate-500 hover:border-slate-300 dark:border-slate-600 dark:bg-slate-800 dark:text-slate-400"
                }`}
              >
                {t(`pricing.day.${day}`)}
              </button>
            );
          })}
        </div>
        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
          <input
            type="text"
            inputMode="numeric"
            value={props.window.start}
            onChange={(event) => props.onChange({ ...props.window, start: event.target.value })}
            placeholder="09:00"
            aria-label={t("pricing.start")}
            className={`${compactFieldCls} text-center`}
          />
          <span className="text-xs text-slate-500 dark:text-slate-400">{t("pricing.to")}</span>
          <input
            type="text"
            inputMode="numeric"
            value={props.window.end}
            onChange={(event) => props.onChange({ ...props.window, end: event.target.value })}
            placeholder="12:00"
            aria-label={t("pricing.end")}
            className={`${compactFieldCls} text-center`}
          />
        </div>
      </div>
    </div>
  );
}

function PriceTierEditor(props: {
  kind: "peak" | "off";
  draft: PricingDraft;
  presetTier: PriceTier | undefined;
  patch: (partial: Partial<PricingDraft>) => void;
}) {
  const { t } = useLang();
  const peak = props.kind === "peak";
  const rows = peak
    ? ([
        ["peakHit", "cache_hit_input", t("pricing.hit")],
        ["peakMiss", "cache_miss_input", t("pricing.miss")],
        ["peakOut", "output", t("pricing.out")],
      ] as const)
    : ([
        ["offHit", "cache_hit_input", t("pricing.hit")],
        ["offMiss", "cache_miss_input", t("pricing.miss")],
        ["offOut", "output", t("pricing.out")],
      ] as const);

  const reset = () => {
    props.patch(
      peak
        ? { peakHit: "", peakMiss: "", peakOut: "" }
        : { offHit: "", offMiss: "", offOut: "" },
    );
  };

  return (
    <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-3 dark:border-slate-700 dark:bg-slate-900/50">
      <div className="mb-2 flex items-center justify-between gap-3">
        <h4 className="flex items-center gap-2 text-sm font-medium text-slate-800 dark:text-slate-100">
          <span className={`h-2 w-2 rounded-full ${peak ? "bg-orange-500" : "bg-blue-500"}`} />
          {peak ? t("pricing.peak") : t("pricing.offPeak")}
        </h4>
        <button type="button" onClick={reset} className="text-xs text-slate-500 hover:underline dark:text-slate-400">
          {t("pricing.resetTier")}
        </button>
      </div>
      <div className="space-y-1">
        {rows.map(([draftKey, presetKey, label]) => {
          const inherited = props.presetTier?.[presetKey];
          return (
            <label key={draftKey} className="grid grid-cols-[minmax(8rem,1fr)_minmax(5rem,7rem)] items-center gap-x-2 py-1">
              <span className="flex items-center gap-1 text-xs text-slate-700 dark:text-slate-300">
                {label}
                <Tooltip
                  text={
                    presetKey === "cache_hit_input"
                      ? t("pricing.hitExplain")
                      : presetKey === "cache_miss_input"
                        ? t("pricing.missExplain")
                        : t("pricing.outExplain")
                  }
                >
                  <HelpCircle size={13} aria-hidden="true" />
                </Tooltip>
              </span>
              <input
                type="number"
                step="any"
                min="0"
                value={props.draft[draftKey]}
                onChange={(event) => props.patch({ [draftKey]: event.target.value })}
                className={compactFieldCls}
              />
              <span className={`col-start-2 mt-0.5 ${subduedTextCls}`}>
                {inherited == null
                  ? t("pricing.noValue")
                  : t("pricing.inheritValue", { value: formatPrice(inherited) })}
              </span>
            </label>
          );
        })}
      </div>
    </div>
  );
}
