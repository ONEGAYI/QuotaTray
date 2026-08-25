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

  it("matchedPresetId 命中原样内容，手改后灭灯", () => {
    const first = PRESET_TEMPLATES[0];
    expect(matchedPresetId(first.json)).toBe(first.id);
    expect(matchedPresetId(`${first.json}\n`)).toBeNull();
    expect(matchedPresetId("{}")).toBeNull();
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
