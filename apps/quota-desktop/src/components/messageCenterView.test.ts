import { describe, expect, it } from "vitest";
import {
  hasUnread,
  mergeMessage,
  messageId,
  type CenterMessage,
} from "./messageCenterView";

describe("消息中心纯逻辑", () => {
  const msg = (version: string): CenterMessage => ({ kind: "update-ready", version });

  it("messageId 由 kind + 版本构成", () => {
    expect(messageId(msg("0.8.0"))).toBe("update-ready:0.8.0");
  });

  it("mergeMessage 入列去重：同版本不重复、新版本取代旧版本", () => {
    const base = [msg("0.8.0")];
    // 重复广播（重启后探测恢复）不叠加、不重排
    expect(mergeMessage(base, msg("0.8.0"))).toEqual(base);
    // 新版本取代同 kind 旧版本（安装按钮始终对应最新包，旧卡片不并排）
    expect(mergeMessage(base, msg("0.9.0"))).toEqual([msg("0.9.0")]);
  });

  it("hasUnread：未读驱动红点，全量已读后清零", () => {
    const messages = [msg("0.8.0"), msg("0.9.0")];
    // 空列表无红点
    expect(hasUnread([], new Set())).toBe(false);
    // 全新消息有红点
    expect(hasUnread(messages, new Set())).toBe(true);
    // 部分已读仍有红点
    const seenHalf = new Set([messageId(msg("0.8.0"))]);
    expect(hasUnread(messages, seenHalf)).toBe(true);
    // 打开面板全量已读后红点消失
    const seenAll = new Set(messages.map(messageId));
    expect(hasUnread(messages, seenAll)).toBe(false);
  });

  it("新事件到达已读集合之后的新消息重新点亮红点", () => {
    const first = [msg("0.8.0")];
    const seen = new Set(first.map(messageId));
    expect(hasUnread(first, seen)).toBe(false);
    const second = mergeMessage(first, msg("0.9.0"));
    // 更晚的版本重新点亮红点
    expect(hasUnread(second, seen)).toBe(true);
  });
});
