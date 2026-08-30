import { describe, expect, it } from "vitest";
import { BUNDLE_ASSETS, GUIDE_DOCS, GUIDE_FOR_PROVIDER, bundleImageSrc } from "./guideDocs";

describe("guideDocs（vite glob 收集）", () => {
  it("契约：阿里云指引文档已被收集（文件名键）", () => {
    const doc = GUIDE_DOCS[GUIDE_FOR_PROVIDER.aliyun_bss];
    expect(doc).toBeTruthy();
    expect(doc).toContain("# 阿里云余额监控配置指引");
  });

  it("契约：GUIDE_FOR_PROVIDER 登记的文档必须存在于收集表", () => {
    for (const [providerId, docKey] of Object.entries(GUIDE_FOR_PROVIDER)) {
      expect(GUIDE_DOCS[docKey], `${providerId} 登记的 ${docKey} 未被 glob 收集`).toBeTruthy();
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
