// 极简行内 Markdown 解析：只认 **粗体** 与 `代码` 两种标记，供把与
// AGENTS.md/README 逐字一致的固定安全提示原文渲染成 UI 富文本——
// 字典值保持文档原文不变，语法转换收敛在本模块。其余语法不处理。
export type InlineMdToken =
  | { kind: "text"; text: string }
  | { kind: "strong"; text: string }
  | { kind: "code"; text: string };

export function parseInlineMd(text: string): InlineMdToken[] {
  const tokens: InlineMdToken[] = [];
  let rest = text;
  while (rest.length > 0) {
    const bold = rest.indexOf("**");
    const code = rest.indexOf("`");
    // 取更早出现的开标记；代码标记先于粗体开头时优先按代码处理
    const useCode = code !== -1 && (bold === -1 || code < bold);
    const open = useCode ? code : bold;
    if (open === -1) {
      tokens.push({ kind: "text", text: rest });
      break;
    }
    const mark = useCode ? "`" : "**";
    const close = rest.indexOf(mark, open + mark.length);
    if (close === -1) {
      // 未闭合：剩余全部按普通文本，避免半截标记丢字
      tokens.push({ kind: "text", text: rest });
      break;
    }
    if (open > 0) tokens.push({ kind: "text", text: rest.slice(0, open) });
    tokens.push(useCode
      ? { kind: "code", text: rest.slice(open + 1, close) }
      : { kind: "strong", text: rest.slice(open + 2, close) });
    rest = rest.slice(close + mark.length);
  }
  return tokens;
}
