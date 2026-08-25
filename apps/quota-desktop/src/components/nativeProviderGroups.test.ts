import { describe, expect, it } from "vitest";
import type { NativeMeta } from "../types";
import { groupNativeProviders } from "./nativeProviderGroups";

function meta(id: string, name = id): NativeMeta {
  return {
    id,
    name,
    pricing: null,
    pricing_by_currency: {},
    custom_models: [],
    supports_plan_variant: false,
  };
}

describe("添加供应商的平台聚合", () => {
  it("按品牌顺序聚合双站和 Kimi 两类产品，不受后端返回顺序影响", () => {
    const groups = groupNativeProviders([
      meta("zai"),
      meta("zai_api"),
      meta("kimi_code_global"),
      meta("siliconflow_global"),
      meta("deepseek"),
      meta("kimi_cn"),
      meta("zhipu"),
      meta("minimax_global"),
      meta("zhipu_api"),
      meta("openrouter"),
      meta("kimi_global"),
      meta("siliconflow"),
      meta("stepfun"),
      meta("kimi_code_cn"),
      meta("novita"),
      meta("minimax"),
    ]);

    expect(groups.map((group) => group.key)).toEqual([
      "deepseek",
      "siliconflow",
      "openrouter",
      "kimi",
      "zhipu",
      "zai",
      "stepfun",
      "novita",
      "minimax",
    ]);
    expect(groups.find((group) => group.key === "siliconflow")?.providers.map((p) => p.id))
      .toEqual(["siliconflow", "siliconflow_global"]);
    expect(groups.find((group) => group.key === "kimi")?.providers.map((p) => p.id))
      .toEqual(["kimi_cn", "kimi_global", "kimi_code_cn", "kimi_code_global"]);
    expect(groups.find((group) => group.key === "zhipu")?.providers.map((p) => p.id))
      .toEqual(["zhipu_api", "zhipu"]);
    expect(groups.find((group) => group.key === "zai")?.providers.map((p) => p.id))
      .toEqual(["zai_api", "zai"]);
    expect(groups.find((group) => group.key === "minimax")?.providers.map((p) => p.id))
      .toEqual(["minimax", "minimax_global"]);
  });

  it("缺失平台不生成空组，未来 native 作为独立兜底组保留", () => {
    const groups = groupNativeProviders([meta("openrouter"), meta("future", "Future AI")]);

    expect(groups.map((group) => group.key)).toEqual(["openrouter", "future"]);
    expect(groups[1].label).toBe("Future AI");
    expect(groups[1].providers.map((provider) => provider.id)).toEqual(["future"]);
  });
});
