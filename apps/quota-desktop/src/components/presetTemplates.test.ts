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

  it("matchedPresetId：serde 真实往返形态不灭灯（保存后重编辑）", () => {
    // 模拟后端全量序列化补全：TemplateConfig 顶层五个 default 键 +
    // request 层的 method:"GET" 与 headers:{}
    const roundTrip = (json: string): string => {
      const config = JSON.parse(json) as Record<string, unknown>;
      config.extract = config.extract ?? {};
      config.transforms = config.transforms ?? [];
      config.windowsFrom = config.windowsFrom ?? null;
      config.windows = config.windows ?? [];
      config.allowInsecure = config.allowInsecure ?? false;
      const request = config.request as Record<string, unknown>;
      request.method = request.method ?? "GET";
      request.headers = request.headers ?? {};
      return JSON.stringify(config, null, 2);
    };
    for (const preset of PRESET_TEMPLATES) {
      expect(
        matchedPresetId(roundTrip(preset.json)),
        `预设 ${preset.id} 经 serde 往返补全后应仍点亮`,
      ).toBe(preset.id);
    }

    // 多余的缺省键混入非预设内容仍灭灯（宽容不等于放行任意内容）
    const custom = JSON.parse(roundTrip(presetJsonOf("custom"))) as {
      request: { url: string };
    };
    custom.request.url = "{{baseUrl}}/other";
    expect(matchedPresetId(JSON.stringify(custom))).toBeNull();
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
