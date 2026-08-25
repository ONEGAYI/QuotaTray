import { describe, expect, it } from "vitest";
import {
  isLightLogo,
  providerIconUrl,
  templateProviderIconUrl,
} from "./providerIcon";

describe("Provider 官方图标映射", () => {
  it("国内/国际及余额/订阅条目复用对应品牌图标", () => {
    expect(providerIconUrl("siliconflow_global")).toBe(providerIconUrl("siliconflow"));
    expect(providerIconUrl("kimi_global")).toBe(providerIconUrl("kimi_cn"));
    expect(providerIconUrl("kimi_code_cn")).toBe(providerIconUrl("kimi_cn"));
    expect(providerIconUrl("kimi_code_global")).toBe(providerIconUrl("kimi_cn"));
    expect(providerIconUrl("zhipu_api")).toBe(providerIconUrl("zhipu"));
    expect(providerIconUrl("zai_api")).toBe(providerIconUrl("zai"));
    expect(providerIconUrl("minimax_global")).toBe(providerIconUrl("minimax"));
  });

  it("十六个预置 Provider 都有图标，未知 native 保留回退空间", () => {
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
      "stepfun",
      "novita",
      "minimax",
      "minimax_global",
    ]) {
      expect(providerIconUrl(id), id).toBeTruthy();
    }
    expect(providerIconUrl("future-provider")).toBeNull();
  });

  it("浅色品牌图标记：仅白色 logo 需要深底变体", () => {
    expect(isLightLogo("stepfun")).toBe(true);
    for (const id of ["deepseek", "novita", "minimax", "minimax_global", "future"]) {
      expect(isLightLogo(id), id).toBe(false);
    }
  });

  it("模板条目按名称启发匹配：含 newapi（不区分大小写）得品牌图，其余回退", () => {
    expect(templateProviderIconUrl("NewAPI 中转")).toBeTruthy();
    expect(templateProviderIconUrl("  my-newapi-site ")).toBeTruthy();
    expect(templateProviderIconUrl("自建聚合站")).toBeNull();
    expect(templateProviderIconUrl("")).toBeNull();
  });
});
