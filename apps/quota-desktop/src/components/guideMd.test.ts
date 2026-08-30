import { describe, expect, it } from "vitest";
import { parseGuideInline, parseGuideMd } from "./guideMd";
import { GUIDE_DOCS } from "./guideDocs";

describe("parseGuideInline", () => {
  it("契约：纯文本原样通过", () => {
    expect(parseGuideInline("普通文本")).toEqual([{ kind: "text", text: "普通文本" }]);
  });

  it("契约：**粗体** 与 `代码` 解析", () => {
    expect(parseGuideInline("**重点**")).toEqual([{ kind: "strong", text: "重点" }]);
    expect(parseGuideInline("`sk-` 开头")).toEqual([
      { kind: "code", text: "sk-" },
      { kind: "text", text: " 开头" },
    ]);
  });

  it("契约：[链接](href) 与 ![图片](src) 解析", () => {
    expect(parseGuideInline("见[控制台](https://example.com)")).toEqual([
      { kind: "text", text: "见" },
      { kind: "link", text: "控制台", href: "https://example.com" },
    ]);
    expect(parseGuideInline("![截图](assets/bundle/a.png)")).toEqual([
      { kind: "image", alt: "截图", src: "assets/bundle/a.png" },
    ]);
  });

  it("契约：未闭合标记整段回退纯文本，不丢字", () => {
    expect(parseGuideInline("**未闭合")).toEqual([{ kind: "text", text: "**未闭合" }]);
    expect(parseGuideInline("`未闭合")).toEqual([{ kind: "text", text: "`未闭合" }]);
    expect(parseGuideInline("[无括号目标")).toEqual([{ kind: "text", text: "[无括号目标" }]);
  });

  it("契约：空目标与含括号目标不按链接处理（回退文本）", () => {
    expect(parseGuideInline("[文本]()")).toEqual([{ kind: "text", text: "[文本]()" }]);
    expect(parseGuideInline("[文本](a(b))")).toEqual([{ kind: "text", text: "[文本](a(b))" }]);
  });
});

describe("parseGuideMd", () => {
  it("契约：标题 h1–h3 / 段落 / 分隔线", () => {
    const blocks = parseGuideMd("# 主标题\n\n正文段落。\n\n---\n\n## 二级\n\n### 三级");
    expect(blocks.map((b) => b.kind)).toEqual([
      "heading",
      "paragraph",
      "hr",
      "heading",
      "heading",
    ]);
    expect(blocks[0]).toMatchObject({ level: 1 });
  });

  it("契约：有序/无序列表连续项合并为单块", () => {
    const md = "1. 第一项\n2. 第二项\n\n- 甲\n- 乙\n* 丙";
    const [ordered, unordered] = parseGuideMd(md);
    expect(ordered).toMatchObject({ kind: "list", ordered: true });
    if (ordered.kind === "list") expect(ordered.items).toHaveLength(2);
    expect(unordered).toMatchObject({ kind: "list", ordered: false });
    if (unordered.kind === "list") expect(unordered.items).toHaveLength(3);
  });

  it("契约：引用块连续行合并，> 前缀剥离", () => {
    const blocks = parseGuideMd("> 阅读提示\n> 第二行\n\n正文");
    expect(blocks[0]).toMatchObject({ kind: "quote" });
    if (blocks[0].kind === "quote") {
      expect(blocks[0].lines).toEqual([{ kind: "text", text: "阅读提示" }, { kind: "text", text: "第二行" }]);
    }
  });

  it("契约：围栏代码块整段保留，语言标注忽略，内部标记不解析", () => {
    const blocks = parseGuideMd("```json\n{\"a\": \"**not bold**\"}\n```\n\n正文");
    expect(blocks[0]).toEqual({ kind: "code", text: '{"a": "**not bold**"}' });
  });

  it("契约：段内多行以空格合并；CRLF 兼容", () => {
    const blocks = parseGuideMd("第一行\r\n第二行\r\n\r\n下一段");
    expect(blocks[0]).toMatchObject({ kind: "paragraph" });
    if (blocks[0].kind === "paragraph") {
      expect(blocks[0].inline).toEqual([{ kind: "text", text: "第一行 第二行" }]);
    }
  });

  it("契约：真实文档冒烟——阿里云指引可全量解析且首块为 h1", () => {
    const doc = GUIDE_DOCS["阿里云余额监控配置指引.md"];
    expect(doc).toBeTruthy();
    const blocks = parseGuideMd(doc);
    expect(blocks[0]).toMatchObject({ kind: "heading", level: 1 });
    // 子集覆盖自查：真实文档至少用到标题/列表/引用/代码行内标记
    const kinds = new Set<string>(blocks.map((b) => b.kind));
    for (const expected of ["heading", "list", "quote", "paragraph"]) {
      expect(kinds.has(expected), `真实文档缺少块级 ${expected}`).toBe(true);
    }
  });
});
