import { useEffect, useRef, useState, type ReactNode } from "react";
import { HelpCircle } from "lucide-react";
import { useLang } from "../i18n";
import type {
  CustomModelDef,
  PeakWindow,
  PlanKind,
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
import { pricingModelChoices } from "./providerPricing";

const WEEKDAYS: Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const fieldCls = "qt-input";
const compactFieldCls = "qt-input qt-input-compact";
const subduedTextCls = "qt-text-subdued";

interface Props {
  preset: PresetPricing | null;
  customModels: CustomModelDef[];
  initial: PricingConfig | undefined;
  onChange: (pricing: PricingConfig | undefined) => void;
}

export function PricingSection(props: Props) {
  const { t } = useLang();
  const [draft, setDraft] = useState(() => draftFrom(props.initial, props.preset));
  const onChangeRef = useRef(props.onChange);
  onChangeRef.current = props.onChange;

  useEffect(() => {
    onChangeRef.current(buildPricing(draft, props.preset, props.customModels));
  }, [draft, props.preset, props.customModels]);

  const patch = (partial: Partial<PricingDraft>) => {
    setDraft((current) => ({ ...current, ...partial }));
  };

  const modelChoices = pricingModelChoices(props.preset, props.customModels);
  const selectedChoice = draft.model.trim()
    ? modelChoices.find(
        (choice) => choice.modelId?.toLowerCase() === draft.model.trim().toLowerCase(),
      )
    : modelChoices.find((choice) => choice.value === "default");
  const modelIsKnown = Boolean(selectedChoice);
  const selectedModelValue = draft.model.trim()
    ? selectedChoice?.value ?? `model:${draft.model.trim()}`
    : "default";
  const hasImplicitDefaultChoice = modelChoices.some((choice) => choice.value === "default");
  const libraryModel = selectedChoice?.source === "custom"
    ? props.customModels.find(
        (model) => model.id.toLowerCase() === selectedChoice.modelId?.toLowerCase(),
      )
    : undefined;
  const presetModel = libraryModel
    ? undefined
    : selectedPresetModel(props.preset, draft.model);
  const effectiveWindows = libraryModel?.windows ?? presetModel?.windows ?? props.preset?.windows ?? [];
  const effectiveTimezone = libraryModel?.timezone_offset_minutes
    ?? props.preset?.timezone_offset_minutes;
  const effectiveCurrency = libraryModel?.currency ?? props.preset?.currency;
  const effectivePeak = libraryModel ? libraryModel.peak : presetModel?.peak;
  const effectiveOffPeak = libraryModel ? libraryModel.off_peak : presetModel?.off_peak;
  const effectivePlan = libraryModel ? "pay_as_you_go" : presetModel?.plan ?? "pay_as_you_go";
  const modelLabel =
    (!modelIsKnown && draft.model.trim())
    || selectedChoice?.label
    || presetModel?.display
    || t("pricing.customModel");

  const setMode = (custom: boolean) => {
    patch({
      custom,
      scheduleCustom: custom && !props.preset ? true : draft.scheduleCustom,
    });
  };

  const activateCustomSchedule = () => {
    const presetWindows = effectiveWindows.map((window) => ({
      days: [...window.days],
      start: window.start,
      end: window.end,
    }));
    patch({
      scheduleCustom: true,
      windows: draft.windows.length > 0 ? draft.windows : presetWindows,
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
                    timezone: effectiveTimezone == null ? "—" : formatUtcOffset(effectiveTimezone),
                    currency: effectiveCurrency ?? "—",
                  })
                : t("pricing.summaryOff")}
          </p>
        </div>
        <Badge tone={draft.custom || selectedChoice?.source === "custom" ? "accent" : "neutral"}>
          {draft.custom
            ? t("pricing.statusCustom")
            : selectedChoice?.source === "custom"
              ? t("pricing.libraryModel")
            : props.preset
              ? t("pricing.statusPreset")
              : t("pricing.statusOff")}
        </Badge>
      </div>

      <div className="qt-segmented qt-pricing-mode">
        <ModeButton active={!draft.custom} onClick={() => setMode(false)}>
          {selectedChoice?.source === "custom"
            ? t("pricing.modeModel")
            : props.preset ? t("pricing.modePreset") : t("pricing.modeOff")}
        </ModeButton>
        <ModeButton active={draft.custom} onClick={() => setMode(true)}>
          {props.preset ? t("pricing.modeCustom") : t("pricing.modeConfigure")}
        </ModeButton>
      </div>

      {modelChoices.length > 0 && (
        <div className="qt-pricing-model-row">
          <label htmlFor="pricing-model">
            {t("pricing.presetModel")}
          </label>
          <select
            id="pricing-model"
            value={selectedModelValue}
            onChange={(event) => {
              const choice = modelChoices.find((item) => item.value === event.target.value);
              const next: Partial<PricingDraft> = {
                model: choice ? (choice.modelId ?? "") : event.target.value.replace(/^model:/, ""),
                custom: !props.preset && choice?.modelId != null ? true : draft.custom,
              };
              if (choice?.plan === "subscription") {
                Object.assign(next, {
                  peakHit: "",
                  peakMiss: "",
                  peakOut: "",
                  offHit: "",
                  offMiss: "",
                  offOut: "",
                });
              }
              patch(next);
            }}
            className={compactFieldCls}
          >
            {!modelIsKnown && draft.model.trim() && (
              <option value={`model:${draft.model.trim()}`}>{draft.model.trim()}</option>
            )}
            {!hasImplicitDefaultChoice && (
              <option value="default">{t("pricing.noModel")}</option>
            )}
            {modelChoices.map((choice) => (
              <option key={choice.value} value={choice.value}>
                {choice.label}
                {choice.value === "default" ? t("pricing.presetDefault") : ""}
                {choice.source === "custom" ? ` · ${t("pricing.libraryModel")}` : ""}
                {choice.plan === "subscription" ? ` · ${t("pricing.subscriptionShort")}` : ""}
              </option>
            ))}
          </select>
          <span className={`${subduedTextCls} sm:text-right`}>
            {selectedChoice?.source === "custom"
              ? t("pricing.libraryNote")
              : t("pricing.presetNote")}
          </span>
        </div>
      )}

      {!draft.custom && (presetModel || libraryModel) && (
        <PresetPreview
          windows={effectiveWindows}
          timezone={effectiveTimezone}
          currency={effectiveCurrency}
          plan={effectivePlan}
          peak={effectivePeak}
          offPeak={effectiveOffPeak}
        />
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
                <h3 id="pricing-windows-heading" className="text-sm font-medium text-[var(--qt-text)]">
                  {t("pricing.windowsTitle")}
                </h3>
                <p className={`mt-1 ${subduedTextCls}`}>{t("pricing.windowsHint")}</p>
              </div>
              {props.preset && (
                <div className="qt-segmented qt-segmented-compact shrink-0">
                  <ModeButton
                    active={!draft.scheduleCustom}
                    onClick={() => patch({ scheduleCustom: false })}
                  >
                    {t("pricing.schedulePreset")}
                  </ModeButton>
                  <ModeButton active={draft.scheduleCustom} onClick={activateCustomSchedule}>
                    {t("pricing.scheduleCustom")}
                  </ModeButton>
                </div>
              )}
            </div>

            <div className="mt-4 grid gap-2 sm:grid-cols-[7rem_minmax(12rem,18rem)_1fr] sm:items-center">
              <label htmlFor="pricing-timezone" className="text-sm font-medium text-[var(--qt-text)]">
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
                  <p className="rounded-[var(--qt-radius-md)] border border-dashed border-[var(--qt-border-strong)] px-3 py-4 text-center text-xs text-[var(--qt-text-soft)]">
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
                  className="py-1 text-xs font-medium text-[var(--qt-accent-strong)] hover:underline"
                >
                  {t("pricing.addWindow")}
                </button>
              </div>
            )}
          </section>

          <section className="border-t border-[var(--qt-border)] px-4 py-4" aria-labelledby="pricing-price-heading">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
              <div>
                <h3 id="pricing-price-heading" className="text-sm font-medium text-[var(--qt-text)]">
                  {t("pricing.priceTitle")}
                </h3>
                <p className={`mt-1 ${subduedTextCls}`}>
                  {effectivePlan === "subscription"
                    ? t("pricing.subscriptionHint")
                    : props.preset ? t("pricing.priceHint") : t("pricing.priceHintNoPreset")}
                </p>
              </div>
              {effectivePlan === "pay_as_you_go" && (
                <label className="grid gap-1 sm:grid-cols-[auto_7rem] sm:items-center sm:gap-2">
                <span className="text-sm font-medium text-[var(--qt-text)]">
                  {t("pricing.currency")}
                </span>
                <input
                  value={draft.currency}
                  onChange={(event) => patch({ currency: event.target.value })}
                  placeholder={props.preset ? t("pricing.inheritValue", { value: props.preset.currency }) : "CNY"}
                  className={compactFieldCls}
                />
                </label>
              )}
            </div>

            {!props.preset && (
              <label className="mt-4 block">
                <span className="text-sm font-medium text-[var(--qt-text)]">
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

            {effectivePlan === "pay_as_you_go" ? (
              <>
                <div className="mt-4 grid gap-3 md:grid-cols-2">
                  <PriceTierEditor kind="peak" draft={draft} presetTier={effectivePeak} patch={patch} />
                  <PriceTierEditor kind="off" draft={draft} presetTier={effectiveOffPeak} patch={patch} />
                </div>
                <p className={`mt-3 text-right ${subduedTextCls}`}>{t("pricing.unit")}</p>
              </>
            ) : (
              <div className="qt-subscription-editor-note">
                <Badge tone="accent">{t("pricing.subscriptionShort")}</Badge>
                <span>{t("pricing.subscriptionPlan")}</span>
              </div>
            )}
          </section>
        </div>
      )}
    </fieldset>
  );
}

/* 分段按钮：视觉全部由 qt-segmented 承担（样式规范 T-002），此处仅行为 */
function ModeButton(props: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={props.active}
      onClick={props.onClick}
    >
      {props.children}
    </button>
  );
}

function PresetPreview(props: {
  windows: PeakWindow[];
  timezone: number | undefined;
  currency: string | undefined;
  plan: PlanKind;
  peak: PriceTier | undefined;
  offPeak: PriceTier | undefined;
}) {
  const { t } = useLang();
  return (
    <div className="qt-preset-preview">
      <PreviewItem
        label={t("pricing.windowsTitle")}
        value={t("pricing.windowCount", { count: props.windows.length })}
      />
      <PreviewItem
        label={t("pricing.timezoneCurrency")}
        value={`${props.timezone == null ? "—" : formatUtcOffset(props.timezone)} · ${props.currency ?? "—"}`}
      />
      {props.plan === "subscription" ? (
        <div className="qt-preview-item qt-preview-subscription">
          <Badge tone="accent">{t("pricing.subscriptionShort")}</Badge>
          <div>
            <p>{t("pricing.subscriptionPlan")}</p>
            <span>{t("pricing.subscriptionHint")}</span>
          </div>
        </div>
      ) : (
        <>
          <PresetTierPreview label={t("pricing.peak")} tier={props.peak} />
          <PresetTierPreview label={t("pricing.offPeak")} tier={props.offPeak} />
        </>
      )}
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

function PresetTierPreview(props: { label: string; tier: PriceTier | undefined }) {
  const { t } = useLang();
  if (
    !props.tier
    || (props.tier.cache_hit_input == null
      && props.tier.cache_miss_input == null
      && props.tier.output == null)
  ) {
    return <PreviewItem label={props.label} value={t("pricing.noValue")} />;
  }
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
    <div className="rounded-[var(--qt-radius-md)] border border-[var(--qt-border)] bg-[var(--qt-surface-soft)] px-3 py-3">
      <div className="mb-2.5 flex items-center justify-between gap-3">
        <span className="text-xs font-medium text-[var(--qt-text)]">
          {t("pricing.windowN", { n: props.index + 1 })}
        </span>
        <button
          type="button"
          onClick={props.onRemove}
          className="text-xs text-[var(--qt-text-soft)] hover:text-[var(--qt-danger)]"
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
                className={`rounded-[var(--qt-radius-xs)] border px-2 py-1.5 text-xs ${
                  active
                    ? "border-[var(--qt-accent)] bg-[var(--qt-accent-soft)] font-medium text-[var(--qt-accent-strong)]"
                    : "border-[var(--qt-border)] bg-[var(--qt-surface)] text-[var(--qt-text-soft)] hover:border-[var(--qt-border-strong)]"
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
          <span className="text-xs text-[var(--qt-text-soft)]">{t("pricing.to")}</span>
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
    <div className="rounded-[var(--qt-radius-md)] border border-[var(--qt-border)] bg-[var(--qt-surface-soft)] px-3 py-3">
      <div className="mb-2 flex items-center justify-between gap-3">
        <h4 className="flex items-center gap-2 text-sm font-medium text-[var(--qt-text)]">
          <span className={`h-2 w-2 rounded-full ${peak ? "bg-[var(--qt-peak)]" : "bg-[var(--qt-offpeak)]"}`} />
          {peak ? t("pricing.peak") : t("pricing.offPeak")}
        </h4>
        <button type="button" onClick={reset} className="text-xs text-[var(--qt-text-soft)] hover:underline">
          {t("pricing.resetTier")}
        </button>
      </div>
      <div className="space-y-1">
        {rows.map(([draftKey, presetKey, label]) => {
          const inherited = props.presetTier?.[presetKey];
          return (
            <label key={draftKey} className="grid grid-cols-[minmax(8rem,1fr)_minmax(5rem,7rem)] items-center gap-x-2 py-1">
              <span className="flex items-center gap-1 text-xs text-[var(--qt-text-soft)]">
                {label}
                <Tooltip
                  multiline
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
