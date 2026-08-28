// 消息中心纯逻辑（TitleBar 铃铛下拉）：消息入列去重与未读判定。
// 消息为会话级内存态——重启即空（更新场景由后端探测恢复重新广播），
// 因此不持久化、不设清除动作（打开面板即全量已读）。

/** 消息中心条目。目前仅「更新就绪」一种；后续扩展（更新失败、告警等）
 * 在此联合类型上追加，渲染与去重按 kind + 业务键。 */
export type CenterMessage = {
  kind: "update-ready";
  /** 就绪安装包的版本号（消息卡片展示与安装确认）。 */
  version: string;
};

/** 稳定消息标识：同一版本的同类消息只保留一条（后端重启后可能对同一
 * 版本重复广播——探测恢复场景，前端据此去重不叠加）。 */
export function messageId(message: CenterMessage): string {
  return `${message.kind}:${message.version}`;
}

/** 入列合并：同标识消息原样返回（不重排、不重复）；不同版本时新消息
 * 取代同 kind 旧消息——「现在安装」始终作用于后端 downloaded 指向的
 * 最新包，旧版本卡片若保留，其按钮承诺的版本会与实际安装版本脱节，
 * 故每个 kind 只保留最新一条。 */
export function mergeMessage(
  existing: CenterMessage[],
  incoming: CenterMessage,
): CenterMessage[] {
  const id = messageId(incoming);
  if (existing.some((m) => messageId(m) === id)) return existing;
  return [...existing.filter((m) => m.kind !== incoming.kind), incoming];
}

/** 未读判定：存在任何未进入已读集合的消息即有红点。 */
export function hasUnread(messages: CenterMessage[], seen: ReadonlySet<string>): boolean {
  return messages.some((m) => !seen.has(messageId(m)));
}
