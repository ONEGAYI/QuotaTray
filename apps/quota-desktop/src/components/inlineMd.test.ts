import { describe, expect, it } from "vitest";
import { parseInlineMd } from "./inlineMd";

// 契约：parseInlineMd 只认 **粗体** 与 `代码` 两种行内标记，
// 输入是与 AGENTS.md/README 逐字一致的固定安全提示原文，
// 渲染层据此把 Markdown 语法转为 UI 富文本，字典值保持原文不变。

describe("parseInlineMd", () => {
  it("契约：纯文本返回单个 text token", () => {
    expect(parseInlineMd("你好 world")).toEqual([{ kind: "text", text: "你好 world" }]);
  });

  it("契约：空字符串返回空数组", () => {
    expect(parseInlineMd("")).toEqual([]);
  });

  it("契约：**粗体** 解析为 strong", () => {
    expect(parseInlineMd("**便携版安全提示**")).toEqual([{ kind: "strong", text: "便携版安全提示" }]);
  });

  it("契约：`代码` 解析为 code", () => {
    expect(parseInlineMd("`Data/portable.key`")).toEqual([{ kind: "code", text: "Data/portable.key" }]);
  });

  it("契约：混合文本按出现顺序交错切分", () => {
    expect(parseInlineMd("⚠️ **提示**：密钥在 `Data/portable.key` 中。")).toEqual([
      { kind: "text", text: "⚠️ " },
      { kind: "strong", text: "提示" },
      { kind: "text", text: "：密钥在 " },
      { kind: "code", text: "Data/portable.key" },
      { kind: "text", text: " 中。" },
    ]);
  });

  it("契约：相邻标记无间隔也能切分", () => {
    expect(parseInlineMd("**a**b`c`")).toEqual([
      { kind: "strong", text: "a" },
      { kind: "text", text: "b" },
      { kind: "code", text: "c" },
    ]);
  });

  it("契约：未闭合标记按普通文本原样保留", () => {
    expect(parseInlineMd("a **b c `d")).toEqual([{ kind: "text", text: "a **b c `d" }]);
  });

  it("契约：粗体内不嵌套解析代码标记", () => {
    expect(parseInlineMd("**x `y` z**")).toEqual([{ kind: "strong", text: "x `y` z" }]);
  });

  it("契约：固定安全提示全文可完整切分且无损还原", () => {
    const raw =
      "⚠️ **便携版安全提示**：便携版会将用于解密凭据的主密钥保存在 `Data/portable.key`。虽然配置中的凭据仍以 AES-GCM 密文存储，但密钥与密文位于同一便携目录，因此整个 `Data/` 目录的保密级别等同明文凭据。请勿将其上传网盘、提交版本库或交给他人；若存储介质遗失或目录泄露，请立即轮换其中使用的全部 API Key。";
    const tokens = parseInlineMd(raw);
    // 无损：token 拼回原文（语法标记不丢字）
    expect(tokens.map((t) => (t.kind === "text" ? t.text : t.kind === "strong" ? `**${t.text}**` : `\`${t.text}\``)).join("")).toBe(raw);
    expect(tokens).toContainEqual({ kind: "strong", text: "便携版安全提示" });
    expect(tokens).toContainEqual({ kind: "code", text: "Data/portable.key" });
  });
});
