// 消息中心纯逻辑（桌面标题栏 / 移动顶部应用栏铃铛下拉共用）：消息入列
// 去重与未读判定。消息为会话级内存态——重启即空（更新场景由后端探测
// 恢复重新广播，低余额由下次成功查询重新入列），因此不持久化、不设
// 清除动作（打开面板即全量已读）。

/** 消息中心条目联合类型；渲染与去重按 kind + 业务键。
 * - update-ready：桌面安装包已下载完成（后端桌面 cfg 广播）；
 * - update-available：移动端检测到新版本（未自动下载，后端移动 cfg 广播）；
 * - low-balance：条目任一窗口已用百分比达到阈值（两端共用，按 provider 去重）。 */
export type CenterMessage =
  | {
      kind: "update-ready";
      /** 就绪安装包的版本号（消息卡片展示与安装确认）。 */
      version: string;
    }
  | {
      kind: "update-available";
      /** 检测到的可用版本号（移动端无自动下载，动作是引导到更新页）。 */
      version: string;
    }
  | {
      kind: "low-balance";
      /** 触发提醒的条目 id（去重业务键）。 */
      providerId: string;
      /** 条目显示名（消息卡片正文）。 */
      name: string;
      /** 已用百分比（0-100，取数据中最高的窗口）。 */
      percent: number;
    };

/** 单例消息 kind：每个 kind 全局只保留最新一条——新到取代旧的，
 * 保证卡片按钮承诺的动作（安装/查看）始终对准后端最新状态。 */
const SINGLETON_KINDS = new Set(["update-ready", "update-available"]);

/** 多例消息 kind（low-balance，每个 provider 一条）的保留上限：
 * 超限丢最旧的同 kind 条目，防止消息面板被低余额条目刷屏。 */
const MULTI_KIND_CAP = 5;

/** 稳定消息标识：update-* 用版本号（后端重启后可能对同一版本重复
 * 广播——探测恢复场景，据此去重不叠加）；low-balance 用条目 id。 */
export function messageId(message: CenterMessage): string {
  switch (message.kind) {
    case "low-balance":
      return `low-balance:${message.providerId}`;
    default:
      return `${message.kind}:${message.version}`;
  }
}

/** 入列合并：同标识消息原样返回（不重排、不重复）；不同标识时——
 * 单例 kind 新消息取代同 kind 旧消息（理由见 SINGLETON_KINDS）；
 * 多例 kind 追加到尾部并与同 kind 条目并存，超上限丢最旧的。 */
export function mergeMessage(
  existing: CenterMessage[],
  incoming: CenterMessage,
): CenterMessage[] {
  const id = messageId(incoming);
  if (existing.some((m) => messageId(m) === id)) return existing;
  let next = existing;
  if (SINGLETON_KINDS.has(incoming.kind)) {
    next = next.filter((m) => m.kind !== incoming.kind);
  } else {
    const same = next.filter((m) => m.kind === incoming.kind);
    if (same.length >= MULTI_KIND_CAP) {
      const dropId = messageId(same[0]);
      next = next.filter((m) => messageId(m) !== dropId);
    }
  }
  return [...next, incoming];
}

/** 未读判定：存在任何未进入已读集合的消息即有红点。 */
export function hasUnread(messages: CenterMessage[], seen: ReadonlySet<string>): boolean {
  return messages.some((m) => !seen.has(messageId(m)));
}
