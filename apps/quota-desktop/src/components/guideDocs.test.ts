import { describe, expect, it } from "vitest";
import { BUNDLE_ASSETS, GUIDE_DOCS, GUIDE_FOR_PROVIDER, bundleImageSrc, resolveGuideDoc } from "./guideDocs";

// 文档收集层的语言映射契约：docs/guide/{zh,en}/ 子目录 → UI 语言选择 →
// 缺失回退；以及手写 GUIDE_FOR_PROVIDER 与 vite glob 实际收录的一致性。

describe("resolveGuideDoc", () => {
  it("契约：请求语言收录了同名文档时返回该语言档", () => {
    expect(resolveGuideDoc("aliyun_bss", "zh")).toEqual({
      lang: "zh",
      key: "aliyun-balance-setup-guide.md",
    });
    expect(resolveGuideDoc("aliyun_bss", "en")).toEqual({
      lang: "en",
      key: "aliyun-balance-setup-guide.md",
    });
  });

  it("契约：请求语言的文件未收录时回退另一语言（单语平台不白屏）", () => {
    const onlyZh = { zh: GUIDE_DOCS.zh, en: {} };
    expect(resolveGuideDoc("aliyun_bss", "en", onlyZh)).toEqual({
      lang: "zh",
      key: "aliyun-balance-setup-guide.md",
    });
    expect(resolveGuideDoc("aliyun_bss", "zh", onlyZh)).toEqual({
      lang: "zh",
      key: "aliyun-balance-setup-guide.md",
    });
  });

  it("契约：平台无指引映射或两侧均未收录时返回 null（入口不渲染）", () => {
    expect(resolveGuideDoc("deepseek", "zh")).toBeNull();
    expect(resolveGuideDoc("nonexistent", "en")).toBeNull();
    expect(resolveGuideDoc("aliyun_bss", "zh", { zh: {}, en: {} })).toBeNull();
  });
});

describe("guideDocs（vite glob 收集）", () => {
  it("契约：两语言子目录内同名文档均被收集，各自语言标题可寻", () => {
    expect(Object.keys(GUIDE_DOCS.zh).length).toBeGreaterThan(0);
    expect(Object.keys(GUIDE_DOCS.en).length).toBeGreaterThan(0);
    expect(GUIDE_DOCS.zh["aliyun-balance-setup-guide.md"]).toContain("# 阿里云余额监控配置指引");
    expect(GUIDE_DOCS.en["aliyun-balance-setup-guide.md"]).toContain("# Aliyun Balance Monitoring Setup Guide");
  });

  it("契约：GUIDE_FOR_PROVIDER 登记的文档名至少在一种语言内被 glob 收集", () => {
    for (const [providerId, key] of Object.entries(GUIDE_FOR_PROVIDER)) {
      const found = GUIDE_DOCS.zh[key] ?? GUIDE_DOCS.en[key];
      expect(found, `${providerId} 登记的 ${key} 未收录于 docs/guide/{zh,en}/`).toBeTruthy();
    }
  });

  it("契约：bundleImageSrc 命中裸文件名与完整相对路径两种形态", () => {
    const sample = Object.keys(BUNDLE_ASSETS)[0];
    if (sample) {
      expect(bundleImageSrc(`assets/bundle/${sample}`)).toBe(BUNDLE_ASSETS[sample]);
      expect(bundleImageSrc(sample)).toBe(BUNDLE_ASSETS[sample]);
    } else {
      // 预留期空目录：未命中一律占位
      expect(bundleImageSrc("assets/bundle/不存在.png")).toBeNull();
    }
  });

  it("契约：未命中的引用返回 null（渲染占位，不抛错）", () => {
    expect(bundleImageSrc("assets/bundle/不存在.png")).toBeNull();
  });
});
