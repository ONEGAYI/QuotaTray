import { describe, expect, it } from "vitest";
import { PRESET_TEMPLATES, matchedPresetId, presetJsonOf } from "./presetTemplates";

describe("模板编辑器预设", () => {
  it("每个预设都是合法 JSON 且带 request.url", () => {
    for (const preset of PRESET_TEMPLATES) {
      const parsed = JSON.parse(preset.json) as { request?: { url?: unknown } };
      expect(typeof parsed.request?.url).toBe("string");
      expect((parsed.request?.url as string).length).toBeGreaterThan(0);
    }
  });

  it("预设 id 唯一", () => {
    const ids = PRESET_TEMPLATES.map((p) => p.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("matchedPresetId：原样与格式差异（空白、键序）均命中", () => {
    const first = PRESET_TEMPLATES[0];
    expect(matchedPresetId(first.json)).toBe(first.id);
    expect(matchedPresetId(`${first.json}\n`)).toBe(first.id);
    const reordered = JSON.stringify(
      Object.fromEntries(Object.entries(JSON.parse(first.json)).reverse()),
      null,
      2,
    );
    expect(matchedPresetId(reordered)).toBe(first.id);
  });

  it("matchedPresetId：serde 往返补全的缺省空值键不灭灯（保存后重编辑）", () => {
    const first = JSON.parse(PRESET_TEMPLATES[0].json) as Record<string, unknown>;
    first.transforms = [];
    first.windowsFrom = null;
    first.windows = [];
    first.allowInsecure = false;
    expect(matchedPresetId(JSON.stringify(first, null, 2))).toBe(PRESET_TEMPLATES[0].id);

    // windows 形态：顶层补空 extract（serde 对无顶层 extract 的配置序列化出 {}）
    const windows = JSON.parse(presetJsonOf("windows")) as Record<string, unknown>;
    windows.extract = {};
    windows.transforms = [];
    windows.allowInsecure = false;
    expect(matchedPresetId(JSON.stringify(windows, null, 2))).toBe("windows");
  });

  it("matchedPresetId：实质改动灭灯，解析失败为 null", () => {
    const first = PRESET_TEMPLATES[0];
    const changed = JSON.parse(first.json) as { request: { url: string } };
    changed.request.url = "{{baseUrl}}/other";
    expect(matchedPresetId(JSON.stringify(changed))).toBeNull();

    // 删掉预设中非空的 transforms 是实质改动
    const site = JSON.parse(presetJsonOf("site")) as Record<string, unknown>;
    delete site.transforms;
    expect(matchedPresetId(JSON.stringify(site))).toBeNull();

    expect(matchedPresetId("not json")).toBeNull();
  });

  it("presetJsonOf 按 id 取 JSON，未知 id 抛错", () => {
    expect(presetJsonOf("custom")).toBe(
      PRESET_TEMPLATES.find((p) => p.id === "custom")?.json,
    );
    // @ts-expect-error 故意传入非法 id 验证防御
    expect(() => presetJsonOf("nope")).toThrow();
  });

  it("多窗口预设经 windowsFrom 声明窗口数组", () => {
    const windows = PRESET_TEMPLATES.find((p) => p.id === "windows");
    const parsed = JSON.parse(windows!.json) as { windowsFrom?: unknown };
    expect(typeof parsed.windowsFrom).toBe("string");
  });
});
