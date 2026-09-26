// 消息中心纯逻辑（桌面标题栏 / 移动顶部应用栏铃铛下拉共用）：消息入列
// 去重与未读判定。消息为会话级内存态（重启即空），但 #132 起恢复消息
// 有跨重启补读：Android 后台 Worker 触发的恢复事件落盘，前端启动经
// take_recovery_messages 读取入列（读取即清）；低余额由下次成功查询
// 重新入列。不设清除动作（打开面板即全量已读）。

/** 消息中心条目联合类型；渲染与去重按 kind + 业务键。
 * - update-ready：桌面安装包已下载完成（后端桌面 cfg 广播）；
 * - update-available：移动端检测到新版本（未自动下载，后端移动 cfg 广播）；
 * - low-balance：条目任一窗口已用百分比达到阈值（两端共用，按 provider 去重）；
 * - balance-recovered：先前低额度的条目所有百分比窗口剩余达恢复阈值
 *   （#132，两端共用；替换同条目的 low-balance 卡片，作为新消息未读）。 */
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
    }
  | {
      kind: "balance-recovered";
      /** 恢复的条目 id（去重业务键，与 low-balance 同组互斥）。 */
      providerId: string;
      /** 条目显示名（消息卡片正文）。 */
      name: string;
      /** 最低剩余百分比（0-100，参与判定的 % 窗口中最保守值）。 */
      remainingPercent: number;
    };

/** 单例消息 kind：每个 kind 全局只保留最新一条——新到取代旧的，
 * 保证卡片按钮承诺的动作（安装/查看）始终对准后端最新状态。 */
const SINGLETON_KINDS = new Set(["update-ready", "update-available"]);

/** 条目级余额消息上限（low-balance 与 balance-recovered 合并计数）：
 * 超限丢最旧的同组条目，防止消息面板被余额条目刷屏。 */
const MULTI_KIND_CAP = 5;

/** 条目级余额消息组键（#132）：low-balance / balance-recovered 反映同一
 * 条目的互斥状态，同组只保留最新一张卡片——恢复卡片替换过时的低额度
 * 卡片，再入低额度时低额度卡片同样替换过时的恢复卡片。 */
function balanceGroupKey(message: CenterMessage): string | null {
  if (message.kind === "low-balance" || message.kind === "balance-recovered") {
    return `balance:${message.providerId}`;
  }
  return null;
}

/** 稳定消息标识：update-* 用版本号（后端重启后可能对同一版本重复
 * 广播——探测恢复场景，据此去重不叠加）；余额消息用 kind + 条目 id
 * （同组替换语义见 balanceGroupKey，消息 id 仍含 kind 使 React key
 * 在卡片替换时正确更新）。 */
export function messageId(message: CenterMessage): string {
  switch (message.kind) {
    case "low-balance":
      return `low-balance:${message.providerId}`;
    case "balance-recovered":
      return `balance-recovered:${message.providerId}`;
    default:
      return `${message.kind}:${message.version}`;
  }
}

/** 入列合并：同标识消息原样返回（不重排、不重复；恢复消息的启动补读
 * 与本会话广播重叠时据此去重）；不同标识时——单例 kind 新消息取代
 * 同 kind 旧消息（理由见 SINGLETON_KINDS）；条目级余额消息（组键见
 * balanceGroupKey）替换同组旧卡片后追加，全部余额卡片（两种 kind）
 * 合并计数超上限丢最旧的。 */
export function mergeMessage(
  existing: CenterMessage[],
  incoming: CenterMessage,
): CenterMessage[] {
  const id = messageId(incoming);
  if (existing.some((m) => messageId(m) === id)) return existing;
  const group = balanceGroupKey(incoming);
  let next = existing;
  if (group != null) {
    // 替换同条目的另一状态卡片（低额度 ↔ 恢复互斥，只留最新）
    next = next.filter((m) => balanceGroupKey(m) !== group);
    // 余额卡片合并计数（两种 kind 同池），超上限丢最旧的同池条目
    const balanceCards = next.filter((m) => balanceGroupKey(m) != null);
    if (balanceCards.length >= MULTI_KIND_CAP) {
      const dropId = messageId(balanceCards[0]);
      next = next.filter((m) => messageId(m) !== dropId);
    }
  } else if (SINGLETON_KINDS.has(incoming.kind)) {
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
