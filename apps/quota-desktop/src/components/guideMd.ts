// 轻量 Markdown 子集解析（配置指引渲染器专用，guideDocs.ts 收集的文档 → GuideBlock[]）。
// 子集约定（预研文档 §六，2026-08-30 所有者补充图片能力）：
//   块级 = h1–h3 / 有序无序列表 / 围栏代码块 / 引用块 / 段落 / 分隔线；
//   行内 = **粗体** / `代码` / [链接](href) / ![图片](src)。
// 输入域为 docs/guide 下人工维护文档；未闭合标记整段回退纯文本（inlineMd 同款不丢字），
// 不追求 CommonMark 完整语义。

export type GuideInline =
  | { kind: "text"; text: string }
  | { kind: "strong"; text: string }
  | { kind: "code"; text: string }
  | { kind: "link"; text: string; href: string }
  | { kind: "image"; alt: string; src: string };

export type GuideBlock =
  | { kind: "heading"; level: 1 | 2 | 3; inline: GuideInline[] }
  | { kind: "paragraph"; inline: GuideInline[] }
  | { kind: "list"; ordered: boolean; items: GuideInline[][] }
  | { kind: "code"; text: string }
  | { kind: "quote"; lines: GuideInline[] }
  | { kind: "hr" };

/** 行内解析：`**`/`` ` ``/`[]()`/`![]()`；未闭合标记按普通文本回退。 */
export function parseGuideInline(text: string): GuideInline[] {
  const tokens: GuideInline[] = [];
  let rest = text;
  let plain = "";
  const flushPlain = () => {
    if (plain.length > 0) {
      tokens.push({ kind: "text", text: plain });
      plain = "";
    }
  };

  while (rest.length > 0) {
    // 图片 ![alt](src)（先于链接判定，`!` 前缀）
    if (rest.startsWith("![")) {
      const img = matchBracketLink(rest, 2);
      if (img) {
        flushPlain();
        tokens.push({ kind: "image", alt: img.text, src: img.href });
        rest = rest.slice(img.consumed);
        continue;
      }
    }
    // 链接 [text](href)
    if (rest.startsWith("[")) {
      const link = matchBracketLink(rest, 1);
      if (link) {
        flushPlain();
        tokens.push({ kind: "link", text: link.text, href: link.href });
        rest = rest.slice(link.consumed);
        continue;
      }
    }
    // 粗体 **...**
    if (rest.startsWith("**")) {
      const close = rest.indexOf("**", 2);
      if (close !== -1 && close > 2) {
        flushPlain();
        tokens.push({ kind: "strong", text: rest.slice(2, close) });
        rest = rest.slice(close + 2);
        continue;
      }
    }
    // 行内代码 `...`
    if (rest.startsWith("`")) {
      const close = rest.indexOf("`", 1);
      if (close !== -1 && close > 1) {
        flushPlain();
        tokens.push({ kind: "code", text: rest.slice(1, close) });
        rest = rest.slice(close + 1);
        continue;
      }
    }
    plain += rest[0];
    rest = rest.slice(1);
  }
  flushPlain();
  return tokens;
}

/** 匹配 `[text](href)`（offset 为 `[` 之后内容起始）；返回内容、目标与消费长度。 */
function matchBracketLink(
  s: string,
  offset: number,
): { text: string; href: string; consumed: number } | null {
  const closeBracket = s.indexOf("](", offset);
  if (closeBracket === -1) return null;
  const closeParen = s.indexOf(")", closeBracket + 2);
  if (closeParen === -1) return null;
  const text = s.slice(offset, closeBracket);
  const href = s.slice(closeBracket + 2, closeParen);
  if (text.length === 0 || href.length === 0) return null;
  // href 内不允许再出现括号（截断匹配直接放弃，避免半截链接吞字）
  if (/[[\]()]/.test(href)) return null;
  return { text, href, consumed: closeParen + 1 };
}

/** 块级解析：逐行状态机；段内多行以空格合并（Markdown 惯例）。 */
export function parseGuideMd(md: string): GuideBlock[] {
  const blocks: GuideBlock[] = [];
  const lines = md.split(/\r?\n/);
  let i = 0;
  let paragraph: string[] = [];

  const flushParagraph = () => {
    if (paragraph.length === 0) return;
    blocks.push({ kind: "paragraph", inline: parseGuideInline(paragraph.join(" ")) });
    paragraph = [];
  };

  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();

    // 围栏代码块：``` 开，``` 闭（不支持嵌套/语言语义，语言标注忽略）
    if (trimmed.startsWith("```")) {
      flushParagraph();
      const codeLines: string[] = [];
      i += 1;
      while (i < lines.length && !lines[i].trim().startsWith("```")) {
        codeLines.push(lines[i]);
        i += 1;
      }
      i += 1; // 跳过闭合围栏（文件末尾未闭合则自然收束）
      blocks.push({ kind: "code", text: codeLines.join("\n") });
      continue;
    }

    if (trimmed === "") {
      flushParagraph();
      i += 1;
      continue;
    }

    // 分隔线：三连字符（首行 h1 下惯例分隔，渲染为 hr）
    if (/^-{3,}$/.test(trimmed)) {
      flushParagraph();
      blocks.push({ kind: "hr" });
      i += 1;
      continue;
    }

    // 标题 h1–h3
    const heading = /^(#{1,3})\s+(.*)$/.exec(trimmed);
    if (heading) {
      flushParagraph();
      const level = heading[1].length as 1 | 2 | 3;
      blocks.push({ kind: "heading", level, inline: parseGuideInline(heading[2]) });
      i += 1;
      continue;
    }

    // 引用块：连续 `> ` 行合并为一块（每行独立行内解析）
    if (trimmed.startsWith(">")) {
      flushParagraph();
      const quoteLines: GuideInline[] = [];
      while (i < lines.length && lines[i].trim().startsWith(">")) {
        const content = lines[i].trim().replace(/^>\s?/, "");
        if (content.length > 0) quoteLines.push(...parseGuideInline(content));
        i += 1;
      }
      blocks.push({ kind: "quote", lines: quoteLines });
      continue;
    }

    // 有序列表：`1. `（连续项合并为一块）
    const orderedItem = /^(\d+)\.\s+(.*)$/.exec(trimmed);
    if (orderedItem) {
      flushParagraph();
      const items: GuideInline[][] = [];
      while (i < lines.length) {
        const m = /^(\d+)\.\s+(.*)$/.exec(lines[i].trim());
        if (!m) break;
        items.push(parseGuideInline(m[2]));
        i += 1;
      }
      blocks.push({ kind: "list", ordered: true, items });
      continue;
    }

    // 无序列表：`- ` / `* `（后者要求后随空格，避免与行首粗体混淆）
    const bulletItem = /^[-*]\s+(.*)$/.exec(trimmed);
    if (bulletItem) {
      flushParagraph();
      const items: GuideInline[][] = [];
      while (i < lines.length) {
        const m = /^[-*]\s+(.*)$/.exec(lines[i].trim());
        if (!m) break;
        items.push(parseGuideInline(m[1]));
        i += 1;
      }
      blocks.push({ kind: "list", ordered: false, items });
      continue;
    }

    paragraph.push(trimmed);
    i += 1;
  }
  flushParagraph();
  return blocks;
}
