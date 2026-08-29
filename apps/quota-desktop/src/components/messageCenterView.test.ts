import { describe, expect, it } from "vitest";
import {
  hasUnread,
  mergeMessage,
  messageId,
  type CenterMessage,
} from "./messageCenterView";

describe("消息中心纯逻辑", () => {
  const msg = (version: string): CenterMessage => ({ kind: "update-ready", version });
  const avail = (version: string): CenterMessage => ({ kind: "update-available", version });
  const low = (providerId: string, percent = 92): CenterMessage => ({
    kind: "low-balance",
    providerId,
    name: providerId,
    percent,
  });

  it("messageId 由 kind + 版本构成", () => {
    expect(messageId(msg("0.8.0"))).toBe("update-ready:0.8.0");
  });

  it("update-available 以版本为业务键，同 kind 取代语义与 update-ready 一致", () => {
    expect(messageId(avail("0.9.0"))).toBe("update-available:0.9.0");
    const base = [avail("0.9.0")];
    // 重复广播（移动端每次进更新页检测）不叠加、不重排
    expect(mergeMessage(base, avail("0.9.0"))).toEqual(base);
    // 新版本取代旧版本（查看按钮始终对准最新版本）
    expect(mergeMessage(base, avail("0.10.0"))).toEqual([avail("0.10.0")]);
  });

  it("mergeMessage 入列去重：同版本不重复、新版本取代旧版本", () => {
    const base = [msg("0.8.0")];
    // 重复广播（重启后探测恢复）不叠加、不重排
    expect(mergeMessage(base, msg("0.8.0"))).toEqual(base);
    // 新版本取代同 kind 旧版本（安装按钮始终对应最新包，旧卡片不并排）
    expect(mergeMessage(base, msg("0.9.0"))).toEqual([msg("0.9.0")]);
  });

  it("low-balance 以 providerId 为业务键：同条目去重、不同条目并存", () => {
    expect(messageId(low("p1"))).toBe("low-balance:p1");
    const base = [low("p1", 90)];
    // 同一 provider 重复入列：去重不重排（也不刷新已读状态）
    expect(mergeMessage(base, low("p1", 95))).toEqual(base);
    // 不同 provider 并存，入列顺序保持
    expect(mergeMessage(base, low("p2", 85))).toEqual([low("p1", 90), low("p2", 85)]);
  });

  it("low-balance 上限 5 条：超限丢最旧的同 kind 条目", () => {
    let messages: CenterMessage[] = [];
    for (const id of ["p1", "p2", "p3", "p4", "p5", "p6"]) {
      messages = mergeMessage(messages, low(id));
    }
    expect(messages).toEqual([low("p2"), low("p3"), low("p4"), low("p5"), low("p6")]);
  });

  it("混合消息列表：单例 kind 取代不误删其他 kind 条目", () => {
    const base = [msg("0.8.0"), low("p1", 90), avail("0.9.0")];
    const next = mergeMessage(base, avail("0.10.0"));
    expect(next).toEqual([msg("0.8.0"), low("p1", 90), avail("0.10.0")]);
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

  it("hasUnread 对 low-balance 同样生效", () => {
    const messages = [low("p1")];
    expect(hasUnread(messages, new Set())).toBe(true);
    expect(hasUnread(messages, new Set([messageId(low("p1"))]))).toBe(false);
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
