import { describe, expect, it } from "vitest";
import {
  downloadPercent,
  formatBytes,
  formatDownloadProgress,
  resolveUpdateError,
  resolveUpdateStatus,
} from "./settingsView";

describe("更新设置视图", () => {
  it("最新手动操作错误优先于后端历史错误", () => {
    expect(
      resolveUpdateError({
        checkError: new Error("本次检测失败"),
        downloadError: null,
        backendError: "旧错误",
        hasAvailable: false,
      }),
    ).toContain("本次检测失败");
  });

  it("已有可用版本时不展示历史检测错误", () => {
    expect(
      resolveUpdateError({
        checkError: null,
        downloadError: null,
        backendError: "旧错误",
        hasAvailable: true,
      }),
    ).toBeNull();
  });

  it("检测失败不会误判为已是最新", () => {
    expect(resolveUpdateStatus({ checking: false, hasAvailable: false, error: "失败" }))
      .toBe("error");
  });

  it("有可用版本时下载失败仍显示错误态（不静默成发现新版本）", () => {
    expect(resolveUpdateStatus({ checking: false, hasAvailable: true, error: "下载失败" }))
      .toBe("error");
  });

  it("格式化已知总量的下载进度与速率", () => {
    const progress = {
      downloaded_bytes: 5 * 1024 * 1024,
      total_bytes: 20 * 1024 * 1024,
      bytes_per_second: 2.5 * 1024 * 1024,
    };
    expect(downloadPercent(progress)).toBe(25);
    expect(formatDownloadProgress(progress)).toBe("5.0 MB / 20.0 MB · 2.5 MB/s · 25%");
  });

  it("总量未知时只展示已下载量和速率", () => {
    const progress = {
      downloaded_bytes: 1536,
      total_bytes: null,
      bytes_per_second: 0,
    };
    expect(downloadPercent(progress)).toBeNull();
    expect(formatDownloadProgress(progress)).toBe("1.5 KB · 0 B/s");
    expect(formatBytes(1024 * 1024 * 1024)).toBe("1.0 GB");
  });
});
