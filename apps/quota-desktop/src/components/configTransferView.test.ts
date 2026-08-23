import { describe, expect, it } from "vitest";
import {
  defaultTransferFileName,
  ensureTransferExtension,
  transferErrorMessage,
} from "./configTransferView";

describe("config transfer view", () => {
  it("builds a stable local-time export filename", () => {
    const now = new Date(2026, 7, 24, 1, 2, 3);
    expect(defaultTransferFileName(now)).toBe(
      "QuotaTray-config-20260824-010203.qtray-export",
    );
  });

  it("adds the private-format extension only when missing", () => {
    expect(ensureTransferExtension("backup")).toBe("backup.qtray-export");
    expect(ensureTransferExtension("backup.QTRAY-EXPORT")).toBe("backup.QTRAY-EXPORT");
  });

  it("normalizes backend and JavaScript errors", () => {
    expect(transferErrorMessage(new Error("broken"))).toBe("broken");
    expect(transferErrorMessage("backend failure")).toBe("backend failure");
    expect(transferErrorMessage(null)).toBe("unknown error");
  });
});
