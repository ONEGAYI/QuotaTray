import { describe, expect, it } from "vitest";
import { scopeName } from "./UsageStatsPage";

describe("使用统计的窗口显示名", () => {
  it("窗口键的全角/半角括号标注决定显示名", () => {
    expect(scopeName("OpenCode Go（5h）", 0, "zh")).toBe("5h");
    expect(scopeName("GLM Coding Plan（MCP）", 2, "en")).toBe("MCP");
  });

  it("week 与 month 标注按语言本地化，避免中英混杂", () => {
    expect(scopeName("Kimi Code（week）", 1, "zh")).toBe("周限");
    expect(scopeName("Kimi Code（week）", 1, "en")).toBe("Weekly");
    expect(scopeName("OpenCode Go（month）", 2, "zh")).toBe("月限");
    expect(scopeName("OpenCode Go（month）", 2, "en")).toBe("Monthly");
  });

  it("无括号的合成键回退序号，其余原样透出", () => {
    expect(scopeName("w1", 0, "zh")).toBe("窗口 1");
    expect(scopeName("w2#2", 1, "en")).toBe("Window 2");
    expect(scopeName("自定义窗口", 0, "zh")).toBe("自定义窗口");
  });
});
