import { describe, expect, it } from "vitest";
import { providerIconUrl } from "./providerIcon";

describe("Provider 官方图标映射", () => {
  it("国内/国际及余额/订阅条目复用对应品牌图标", () => {
    expect(providerIconUrl("siliconflow_global")).toBe(providerIconUrl("siliconflow"));
    expect(providerIconUrl("kimi_global")).toBe(providerIconUrl("kimi_cn"));
    expect(providerIconUrl("kimi_code_cn")).toBe(providerIconUrl("kimi_cn"));
    expect(providerIconUrl("kimi_code_global")).toBe(providerIconUrl("kimi_cn"));
    expect(providerIconUrl("zhipu_api")).toBe(providerIconUrl("zhipu"));
    expect(providerIconUrl("zai_api")).toBe(providerIconUrl("zai"));
  });

  it("十二个预置 Provider 都有图标，未知 native 保留回退空间", () => {
    for (const id of [
      "deepseek",
      "siliconflow",
      "siliconflow_global",
      "openrouter",
      "kimi_cn",
      "kimi_global",
      "kimi_code_cn",
      "kimi_code_global",
      "zhipu_api",
      "zai_api",
      "zhipu",
      "zai",
    ]) {
      expect(providerIconUrl(id), id).toBeTruthy();
    }
    expect(providerIconUrl("future-provider")).toBeNull();
  });
});
