import { useState } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import { Button } from "./ui";

/** 官方模型资料；不将用户覆盖价格冒充已核验价格。 */
export function PricingProvenance({ sourceUrls, verifiedAt }: {
  sourceUrls: readonly string[];
  verifiedAt: string | null;
}) {
  const { t } = useLang();
  const [error, setError] = useState(false);
  const verifiedMs = verifiedAt ? Date.parse(verifiedAt) : NaN;
  const stale = Number.isFinite(verifiedMs) && Date.now() - verifiedMs > 30 * 86_400_000;
  return (
    <details className="qt-pricing-provenance">
      <summary className="qt-btn qt-btn-ghost">{t("pricing.officialInfo")}</summary>
      <div className="grid gap-1 text-xs text-[var(--qt-text-soft)]">
        <span>{t("pricing.verifiedAt")}：{verifiedAt ?? t("pricing.verificationUnknown")}</span>
        {stale && <span>{t("pricing.verificationStale")}</span>}
        <span>{t("pricing.referenceOnly")}</span>
        {sourceUrls.length === 0 && <span>{t("pricing.sourceMissing")}</span>}
        {sourceUrls.map((url) => (
          <Button key={url} type="button" variant="ghost" className="whitespace-normal break-all text-left"
            onClick={() => { setError(false); void api.openConsoleUrl(url).catch(() => setError(true)); }}>
            {url}
          </Button>
        ))}
        {error && <span role="status">{t("pricing.sourceOpenFailed")}</span>}
      </div>
    </details>
  );
}
