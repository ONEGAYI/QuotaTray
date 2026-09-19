import { describe, expect, it } from "vitest";
import {
  IMPORT_OVERWRITE_COUNTDOWN_SECONDS,
  buildImportOptions,
  resolveImportSubmitState,
  validateImportInput,
} from "./transferImportView";

describe("import container password validation", () => {
  it("convenient containers pass regardless of password input", () => {
    expect(validateImportInput("Convenient", "")).toBeNull();
    expect(validateImportInput("Convenient", "leftover")).toBeNull();
  });

  it("password containers require a non-empty password", () => {
    expect(validateImportInput("Password", "")).toEqual({
      key: "settings.importPasswordRequired",
    });
    expect(validateImportInput("Password", "12345678")).toBeNull();
  });
});

describe("import submit state (armed state machine)", () => {
  const armed = {
    hasFile: true,
    passwordError: false,
    strategy: "overwrite" as const,
    acknowledged: true,
    countdownRemaining: 0,
  };

  it("merge submits once a file is picked and password input is valid", () => {
    expect(
      resolveImportSubmitState({ ...armed, strategy: "merge", acknowledged: false }),
    ).toEqual({ canSubmit: true, danger: false, ackEnabled: false, countdownSeconds: null });
  });

  it("blocks submit before a file is picked or while password is missing", () => {
    expect(resolveImportSubmitState({ ...armed, strategy: "merge", hasFile: false })).toEqual({
      canSubmit: false,
      danger: false,
      ackEnabled: false,
      countdownSeconds: null,
    });
    expect(
      resolveImportSubmitState({ ...armed, strategy: "merge", passwordError: true }),
    ).toEqual({ canSubmit: false, danger: false, ackEnabled: false, countdownSeconds: null });
  });

  it("overwrite stays danger and locked while the reading countdown runs", () => {
    const state = resolveImportSubmitState({
      ...armed,
      countdownRemaining: IMPORT_OVERWRITE_COUNTDOWN_SECONDS,
    });
    // 三重防线之倒计时：确认钮与风险勾选同时禁用
    expect(state.canSubmit).toBe(false);
    expect(state.ackEnabled).toBe(false);
    expect(state.danger).toBe(true);
    expect(state.countdownSeconds).toBe(IMPORT_OVERWRITE_COUNTDOWN_SECONDS);
  });

  it("overwrite unlocks the checkbox once countdown reaches zero but submit still needs the acknowledgement", () => {
    const unacked = resolveImportSubmitState({ ...armed, acknowledged: false });
    expect(unacked).toEqual({
      canSubmit: false,
      danger: true,
      ackEnabled: true,
      countdownSeconds: null,
    });
    const acked = resolveImportSubmitState({ ...armed, acknowledged: true });
    expect(acked.canSubmit).toBe(true);
  });

  it("overwrite without a file cannot submit even when fully armed", () => {
    expect(
      resolveImportSubmitState({ ...armed, hasFile: false }),
    ).toMatchObject({ canSubmit: false, danger: true });
  });
});

describe("buildImportOptions payload", () => {
  it("maps merge to the Merge variant without leaking the password field", () => {
    expect(buildImportOptions("Password", "12345678", "merge")).toEqual({
      password: "12345678",
      strategy: "Merge",
    });
  });

  it("convenient containers never carry the password across IPC", () => {
    expect(buildImportOptions("Convenient", "leftover", "overwrite")).toEqual({
      password: null,
      strategy: "Overwrite",
    });
  });

  it("maps overwrite for password containers with the typed password", () => {
    expect(buildImportOptions("Password", "secret-pass", "overwrite")).toEqual({
      password: "secret-pass",
      strategy: "Overwrite",
    });
  });
});
