import { describe, expect, it } from "vitest";
import {
  buildAiAssistPackage,
  buildAiAssistPrompt,
  buildCliDebugGuide,
  sanitizeSharedText,
} from "./aiAssistPack";

const input = {
  mode: "template" as const,
  providerName: "某中转站",
  baseUrl: "https://relay.example.com",
  docsUrl: "https://relay.example.com/docs",
  goal: "查询剩余额度",
  draft: JSON.stringify({
    request: {
      url: "{{baseUrl}}/api/user/self",
      headers: { Authorization: "Bearer sk-live-secret-123456" },
    },
    extract: { remaining: "$.data.quota" },
  }),
  validationMessage: "extract.remaining：路径不存在",
  testError: "HTTP 401：token=[REDACTED]",
  responseSample: '{"data":{"quota":123,"token":"sk-response-secret-123"}}',
};

describe("AI 调试求助包", () => {
  it("不会把常见明文凭据带入包或提示词", () => {
    const pkg = buildAiAssistPackage(input);
    const prompt = buildAiAssistPrompt(input, "zh");
    const serialized = JSON.stringify(pkg);

    expect(serialized).not.toContain("sk-live-secret-123456");
    expect(serialized).not.toContain("sk-response-secret-123");
    expect(serialized).toContain("Bearer {{apiKey}}");
    expect(prompt).not.toContain("sk-live-secret-123456");
    expect(prompt).toContain("{{apiKey}}");
  });

  it("保留未保存草稿、现场错误与联网检索要求", () => {
    const prompt = buildAiAssistPrompt(input, "zh");

    expect(prompt).toContain("某中转站");
    expect(prompt).toContain("路径不存在");
    expect(prompt).toContain("联网搜索");
    expect(prompt).toContain("quota assist validate");
    expect(prompt).toContain("quotatray-ai-result");
  });

  it("CLI 指引明确 QuotaTray 只提供调试能力", () => {
    const guide = buildCliDebugGuide("D:\\Temp\\relay.qtray-assist.json", "template", "zh");

    expect(guide).toContain("不提供 AI Agent");
    expect(guide).toContain("quota assist schema");
    expect(guide).toContain("--input \"D:\\Temp\\relay.qtray-assist.json\"");
  });

  it("会清理 Bearer、JWT 与敏感 JSON 字段", () => {
    const cleaned = sanitizeSharedText(
      'Authorization: Bearer abcdefghijklmnop {"cookie":"session-secret","safe":12} eyJhbGciOiJIUzI1NiJ9.abc.def',
    );

    expect(cleaned).not.toContain("abcdefghijklmnop");
    expect(cleaned).not.toContain("session-secret");
    expect(cleaned).not.toContain("eyJhbGciOiJIUzI1NiJ9");
    expect(cleaned).toContain("12");
  });
});
