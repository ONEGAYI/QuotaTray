import deepseekUrl from "../assets/providers/deepseek.svg";
import kimiUrl from "../assets/providers/kimi.svg";
import minimaxUrl from "../assets/providers/minimax.svg";
import novitaUrl from "../assets/providers/novita.svg";
import openrouterUrl from "../assets/providers/openrouter.svg";
import siliconflowUrl from "../assets/providers/siliconflow.svg";
import stepfunUrl from "../assets/providers/stepfun.svg";
import zaiUrl from "../assets/providers/zai.svg";
import zhipuUrl from "../assets/providers/zhipu.svg";

const PROVIDER_ICON_URLS: Readonly<Record<string, string>> = {
  deepseek: deepseekUrl,
  siliconflow: siliconflowUrl,
  siliconflow_global: siliconflowUrl,
  openrouter: openrouterUrl,
  kimi_cn: kimiUrl,
  kimi_global: kimiUrl,
  kimi_code_cn: kimiUrl,
  kimi_code_global: kimiUrl,
  zhipu: zhipuUrl,
  zhipu_api: zhipuUrl,
  zai: zaiUrl,
  zai_api: zaiUrl,
  stepfun: stepfunUrl,
  novita: novitaUrl,
  minimax: minimaxUrl,
  minimax_global: minimaxUrl,
};

/// 浅色品牌图（如 StepFun 官方纯白图形）：图标容器需切换深底变体
/// （index.css 的 is-light-logo）才能在固定浅底方案下可见。
const LIGHT_LOGO_IDS: ReadonlySet<string> = new Set(["stepfun"]);

/** 返回预置 Provider 的官方品牌图；未知 native 由调用方回退为首字母。 */
export function providerIconUrl(nativeId: string): string | null {
  return PROVIDER_ICON_URLS[nativeId] ?? null;
}

/** 该 native 的品牌图是否为浅色图形（容器需深底，见 is-light-logo 变体）。 */
export function isLightLogo(nativeId: string): boolean {
  return LIGHT_LOGO_IDS.has(nativeId);
}
