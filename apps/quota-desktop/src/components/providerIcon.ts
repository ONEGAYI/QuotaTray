import deepseekUrl from "../assets/providers/deepseek.svg";
import kimiUrl from "../assets/providers/kimi.svg";
import openrouterUrl from "../assets/providers/openrouter.svg";
import siliconflowUrl from "../assets/providers/siliconflow.svg";
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
  zai: zaiUrl,
};

/** 返回预置 Provider 的官方品牌图；未知 native 由调用方回退为首字母。 */
export function providerIconUrl(nativeId: string): string | null {
  return PROVIDER_ICON_URLS[nativeId] ?? null;
}
