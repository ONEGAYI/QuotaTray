import { describe, expect, it } from "vitest";
import {
  buildAiAssistPackage,
  buildAiAssistPrompt,
  buildLocalAgentPrompt,
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

  it("本机 Agent 提示词包含真实 CLI 路径、任务步骤与安全边界", () => {
    const prompt = buildLocalAgentPrompt(
      "D:\\Temp\\relay.qtray-assist.json",
      "template",
      "D:\\Apps\\QuotaTray\\quota.exe",
      "zh",
      true,
    );

    expect(prompt).toContain("你是 QuotaTray 配置调试 Agent");
    expect(prompt).toContain("$quotaCli = 'D:\\Apps\\QuotaTray\\quota.exe'");
    expect(prompt).toContain("& $quotaCli assist schema");
    expect(prompt).toContain("--input $diagnosticPackage");
    expect(prompt).toContain("联网搜索");
    expect(prompt).toContain("不得索要、泄露或输出 API key");
    // entryId 存在 → 给出端测指引；不存在 → 明确跳过（结尾句同步分支化，
    // 不指涉不存在的命令块）
    expect(prompt).toContain("assist test --mode");
    expect(prompt).toContain("entryId");
    const noEntry = buildLocalAgentPrompt(
      "D:\\Temp\\relay.qtray-assist.json",
      "template",
      "D:\\Apps\\QuotaTray\\quota.exe",
      "zh",
      false,
    );
    expect(noEntry).not.toContain("assist test --mode");
    expect(noEntry).not.toContain("唯一允许的真实试查");
    expect(noEntry).toContain("无法端测");
  });

  it("已保存条目携带 entryId，新增草稿不带", () => {
    const withEntry = buildAiAssistPackage({ ...input, entryId: "p7" });
    expect(withEntry.entryId).toBe("p7");

    const fresh = buildAiAssistPackage({ ...input, entryId: null });
    expect(fresh.entryId).toBeUndefined();
    expect(JSON.stringify(fresh)).not.toContain("entryId");
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

  it("自家生态形态参与清理：api_key2 键与 New-Api-User 头的真实值被替换，占位符保留原样", () => {
    const cleaned = sanitizeSharedText(
      '{"api_key2":"user-9","New-Api-User":"12345678","headers":{"New-Api-User":"{{apiKey2}}","Authorization":"Bearer {{apiKey}}"}}',
    );

    expect(cleaned).not.toContain("user-9");
    expect(cleaned).not.toContain("12345678");
    // 纯占位符值不被误伤（误换会破坏草稿语义）
    expect(cleaned).toContain('"New-Api-User":"{{apiKey2}}"');
    expect(cleaned).toContain('"Bearer {{apiKey}}"');
    // Bearer + 第二槽占位的组合形态同样保留（Bearer 规则的前瞻覆盖两槽）
    expect(sanitizeSharedText("Bearer {{apiKey2}}")).toBe("Bearer {{apiKey2}}");
    expect(sanitizeSharedText('{"Authorization":"Bearer {{apiKey2}}"}')).toContain(
      '"Bearer {{apiKey2}}"',
    );
  });
});
