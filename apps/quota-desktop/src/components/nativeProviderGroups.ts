import type { NativeMeta } from "../types";

export type NativeProviderGroupKey =
  | "deepseek"
  | "siliconflow"
  | "openrouter"
  | "kimi"
  | "zhipu"
  | "zai"
  | string;

export interface NativeProviderGroup {
  key: NativeProviderGroupKey;
  /** 未知 native 使用后端名称；已知平台由 UI 的双语文案覆盖。 */
  label: string;
  iconProviderId: string;
  providers: NativeMeta[];
}

const KNOWN_GROUPS = [
  { key: "deepseek", ids: ["deepseek"] },
  { key: "siliconflow", ids: ["siliconflow", "siliconflow_global"] },
  { key: "openrouter", ids: ["openrouter"] },
  { key: "kimi", ids: ["kimi_cn", "kimi_global", "kimi_code_cn", "kimi_code_global"] },
  { key: "zhipu", ids: ["zhipu"] },
  { key: "zai", ids: ["zai"] },
] as const;

/** 将 native 注册表聚合为稳定的平台一级菜单；未知项独立追加，避免被静默隐藏。 */
export function groupNativeProviders(metas: NativeMeta[]): NativeProviderGroup[] {
  const byId = new Map(metas.map((meta) => [meta.id, meta]));
  const knownIds = new Set<string>();
  const groups: NativeProviderGroup[] = [];

  for (const definition of KNOWN_GROUPS) {
    const providers = definition.ids.flatMap((id) => {
      knownIds.add(id);
      const provider = byId.get(id);
      return provider ? [provider] : [];
    });
    if (providers.length === 0) continue;
    groups.push({
      key: definition.key,
      label: providers[0].name,
      iconProviderId: providers[0].id,
      providers,
    });
  }

  for (const provider of metas) {
    if (knownIds.has(provider.id)) continue;
    groups.push({
      key: provider.id,
      label: provider.name,
      iconProviderId: provider.id,
      providers: [provider],
    });
  }

  return groups;
}
