import { describe, expect, it } from "vitest";
import {
  EXPORT_PASSWORD_MIN_LENGTH,
  buildExportOptions,
  validateExportInput,
} from "./transferExportView";

describe("export mode input validation", () => {
  it("convenient mode always passes regardless of password fields", () => {
    expect(validateExportInput("convenient", "", "")).toBeNull();
    expect(validateExportInput("convenient", "leftover", "other")).toBeNull();
  });

  it("password mode rejects passwords shorter than the minimum", () => {
    expect(validateExportInput("password", "", "pass")).toEqual({
      key: "settings.exportPasswordTooShort",
      min: EXPORT_PASSWORD_MIN_LENGTH,
    });
    expect(validateExportInput("password", "1234567", "1234567")).toEqual({
      key: "settings.exportPasswordTooShort",
      min: EXPORT_PASSWORD_MIN_LENGTH,
    });
  });

  it("password mode accepts exactly the minimum length when inputs match", () => {
    const exact = "a".repeat(EXPORT_PASSWORD_MIN_LENGTH);
    expect(validateExportInput("password", exact, exact)).toBeNull();
  });

  it("counts password length by code point like core's char counting", () => {
    // 7 个 CJK 字符 = 7 chars（UTF-16 length 同为 7）：不足
    expect(validateExportInput("password", "密码密码密码密", "密码密码密码密")).toEqual({
      key: "settings.exportPasswordTooShort",
      min: EXPORT_PASSWORD_MIN_LENGTH,
    });
    // 8 个 CJK 字符恰好达标
    const cjk = "密码".repeat(4);
    expect(validateExportInput("password", cjk, cjk)).toBeNull();
  });

  it("password mode rejects mismatched confirmation once length is sufficient", () => {
    const exact = "a".repeat(EXPORT_PASSWORD_MIN_LENGTH);
    expect(validateExportInput("password", exact, `${exact}x`)).toEqual({
      key: "settings.exportPasswordMismatch",
    });
  });

  it("reports length before mismatch when both problems are present", () => {
    expect(validateExportInput("password", "short", "different")).toEqual({
      key: "settings.exportPasswordTooShort",
      min: EXPORT_PASSWORD_MIN_LENGTH,
    });
  });
});

describe("buildExportOptions payload", () => {
  it("maps convenient mode to the unit variant literal", () => {
    expect(buildExportOptions("convenient", "ignored")).toBe("Convenient");
  });

  it("maps password mode to the externally tagged single-key object", () => {
    expect(buildExportOptions("password", "12345678")).toEqual({
      Password: { password: "12345678" },
    });
  });
});
