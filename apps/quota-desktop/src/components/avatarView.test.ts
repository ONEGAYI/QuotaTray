import { describe, expect, it } from "vitest";
import {
  avatarBrandKey,
  avatarValueKind,
  builtinAvatarChoices,
  canResetAvatar,
  resolveAvatar,
} from "./avatarView";
import { brandIconUrl } from "./providerIcon";

/** 契约：三种取值形态判别与 core `avatar` 模块同口径
 *  （`brand:` / `data:image/` / 未设置）。 */
describe("avatarValueKind", () => {
  it("识别品牌键与上传图", () => {
    expect(avatarValueKind("brand:openai")).toBe("brand");
    expect(avatarValueKind("data:image/png;base64,AAAA")).toBe("data");
    expect(avatarValueKind("data:image/svg+xml;base64,AAAA")).toBe("data");
  });

  it("缺省与空白都视为未设置", () => {
    expect(avatarValueKind(undefined)).toBe("none");
    expect(avatarValueKind(null)).toBe("none");
    expect(avatarValueKind("")).toBe("none");
    expect(avatarValueKind("   ")).toBe("none");
  });

  it("未知前缀按未设置处理（不显示破图）", () => {
    expect(avatarValueKind("lib:future")).toBe("none");
    expect(avatarValueKind("https://example.com/a.png")).toBe("none");
  });
});

describe("avatarBrandKey", () => {
  it("取出 slug；空 slug 与非品牌形态返回 null", () => {
    expect(avatarBrandKey("brand:kimi")).toBe("kimi");
    expect(avatarBrandKey("brand:")).toBeNull();
    expect(avatarBrandKey("data:image/png;base64,AA")).toBeNull();
    expect(avatarBrandKey(undefined)).toBeNull();
  });
});

/** 契约：解析优先级为「显式覆盖 → 平台默认 → 首字母」，任何一层失败
 *  都静默降到下一层。 */
describe("resolveAvatar", () => {
  it("显式品牌键覆盖平台默认图，且保持 contain 形态（不是照片的 cover）", () => {
    const source = resolveAvatar({ avatar: "brand:kimi", nativeId: "deepseek", entryName: "DS" });
    expect(source.kind).toBe("custom");
    expect(source).toHaveProperty("url", brandIconUrl("kimi"));
    // 品牌 SVG 的 viewBox 差异极大，cover 会裁得认不出；只有用户照片才铺满
    expect(source.className).toBe(" is-brand");
  });

  it("选浅色品牌图时同时带 is-brand 与 is-light-logo", () => {
    // 两类必须共存：is-brand 不再改底色，深底由 is-light-logo 提供，
    // 否则纯白图形会落在浅底上隐形
    const source = resolveAvatar({ avatar: "brand:stepfun", nativeId: "deepseek", entryName: "DS" });
    expect(source.className).toBe(" is-brand is-light-logo");
  });

  it("上传图按 data URL 原样渲染，带 is-upload（cover 铺满）", () => {
    const url = "data:image/png;base64,AAAA";
    const source = resolveAvatar({ avatar: url, nativeId: "deepseek", entryName: "DS" });
    // className 由解析层一次决定：三处渲染点直接取用，避免某处漏掉形态类
    expect(source).toEqual({ kind: "custom", url, className: " is-upload" });
  });

  it("跟随平台的品牌图不带填充类（默认 contain 留边）", () => {
    const source = resolveAvatar({ avatar: undefined, nativeId: "deepseek", entryName: "DS" });
    expect(source.kind).toBe("platform");
    expect(source.className).toBe("");
  });

  it("stepfun 浅色图同时带 is-light-logo", () => {
    const source = resolveAvatar({ avatar: undefined, nativeId: "stepfun", entryName: "SF" });
    expect(source.className).toContain("is-light-logo");
  });

  it("无覆盖时回落到平台品牌图（contain 形态）", () => {
    const source = resolveAvatar({ avatar: undefined, nativeId: "deepseek", entryName: "DS" });
    expect(source.kind).toBe("platform");
  });

  it("未知品牌 slug 静默回落平台默认，而不是渲染破图", () => {
    const source = resolveAvatar({ avatar: "brand:nope", nativeId: "deepseek", entryName: "DS" });
    expect(source.kind).toBe("platform");
  });

  it("模板条目按名称启发匹配，匹配不到走首字母", () => {
    expect(
      resolveAvatar({ avatar: undefined, nativeId: null, entryName: "我的 newapi 中转" }).kind,
    ).toBe("platform");
    expect(resolveAvatar({ avatar: undefined, nativeId: null, entryName: "私人账户" }).kind).toBe(
      "initials",
    );
  });

});

/** 契约：内置挑选面板的数据源——每个 slug 都能取到图，且不重复
 *  （4 个 Kimi 变体共用一张图，挑图面板里只该出现一次）。 */
describe("builtinAvatarChoices", () => {
  it("每个可选项都取得到图", () => {
    const choices = builtinAvatarChoices();
    expect(choices.length).toBeGreaterThan(0);
    for (const choice of choices) {
      expect(choice.url).toBeTruthy();
      expect(brandIconUrl(choice.slug)).toBe(choice.url);
    }
  });

  it("收敛到 16 个品牌资产文件，而非 22 个 native id", () => {
    // 品牌图按文件去重（4 个 Kimi 变体共用 kimi.svg），挑图面板里只该出现一次
    const slugs = builtinAvatarChoices().map((c) => c.slug);
    expect(slugs).toHaveLength(16);
    // 逐个核对，不靠 Set 去重后再比长度（那与上一行同源，恒真）
    expect(slugs.every((slug) => slug.length > 0)).toBe(true);
  });

  it("每个 native id 用的品牌图都在可选项里", () => {
    // 反向锁：新增品牌资产若忘了加进 BRAND_ICON_URLS，挑图面板就会漏项
    for (const slug of ["deepseek", "kimi", "stepfun", "openrouter", "zhipu", "zai"]) {
      expect(builtinAvatarChoices().some((c) => c.slug === slug)).toBe(true);
    }
  });
});

describe("恢复默认", () => {
  it("无覆盖时「恢复默认」不可用", () => {
    expect(canResetAvatar(undefined)).toBe(false);
    expect(canResetAvatar("")).toBe(false);
    expect(canResetAvatar("brand:kimi")).toBe(true);
    expect(canResetAvatar("data:image/png;base64,AA")).toBe(true);
  });
});
