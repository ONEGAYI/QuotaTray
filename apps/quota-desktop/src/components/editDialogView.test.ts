import { describe, expect, it } from "vitest";
import { resolveSaveKeys } from "./editDialogView";

describe("resolveSaveKeys", () => {
  it("契约：普通 native 平台不写第二槽（残留输入不落 vault）", () => {
    expect(
      resolveSaveKeys({
        tab: "native",
        usesCliCredentials: false,
        usesApiKey2: false,
        apiKey: "sk-1",
        apiKey2: "残留",
      }),
    ).toEqual({ saveKey: "sk-1", saveKey2: null });
  });

  it("契约：CLI 凭据型平台两个 key 都不写", () => {
    expect(
      resolveSaveKeys({
        tab: "native",
        usesCliCredentials: true,
        usesApiKey2: false,
        apiKey: "残留",
        apiKey2: "残留",
      }),
    ).toEqual({ saveKey: null, saveKey2: null });
  });

  it("契约：双凭据 native 平台（阿里云余额）第二槽按同语义写入/保持", () => {
    expect(
      resolveSaveKeys({
        tab: "native",
        usesCliCredentials: false,
        usesApiKey2: true,
        apiKey: "LTAI-id",
        apiKey2: "secret",
      }),
    ).toEqual({ saveKey: "LTAI-id", saveKey2: "secret" });
    // 空输入 = 保持不变（null 语义）
    expect(
      resolveSaveKeys({
        tab: "native",
        usesCliCredentials: false,
        usesApiKey2: true,
        apiKey: "",
        apiKey2: "  ",
      }),
    ).toEqual({ saveKey: null, saveKey2: null });
  });

  it("契约：template/script 双槽均为「空=保持不变」", () => {
    expect(
      resolveSaveKeys({
        tab: "template",
        usesCliCredentials: false,
        usesApiKey2: false,
        apiKey: "sk-t",
        apiKey2: "uid",
      }),
    ).toEqual({ saveKey: "sk-t", saveKey2: "uid" });
    expect(
      resolveSaveKeys({
        tab: "script",
        usesCliCredentials: false,
        usesApiKey2: false,
        apiKey: "  ",
        apiKey2: "",
      }),
    ).toEqual({ saveKey: null, saveKey2: null });
  });
});
