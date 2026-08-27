import { describe, expect, it } from "vitest";
import {
  downloadPercent,
  formatBytes,
  formatDownloadProgress,
  resolveUpdateAction,
  resolveUpdateError,
  resolveUpdateErrorDetail,
  resolveUpdateStatus,
  runtimeLabel,
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

  it("安装错误在检测/下载无错时透出", () => {
    expect(
      resolveUpdateError({
        checkError: null,
        downloadError: null,
        installError: "安装包文件已丢失",
        backendError: null,
        hasAvailable: true,
      }),
    ).toContain("安装包文件已丢失");
    expect(
      resolveUpdateError({
        checkError: null,
        downloadError: new Error("下载失败"),
        installError: "安装包文件已丢失",
        backendError: null,
        hasAvailable: true,
      }),
    ).toContain("下载失败");
  });

  it("悬停详情：主错误恰为后端 last_error 时透出 detail", () => {
    const backendError = "网络错误：HTTP 403";
    const detail = "API rate limit exceeded for 1.2.3.4.";
    expect(
      resolveUpdateErrorDetail({
        operationError: backendError,
        backendError,
        backendErrorDetail: detail,
      }),
    ).toBe(detail);
    // 操作错误（非后端文案）无对应详情
    expect(
      resolveUpdateErrorDetail({
        operationError: "本次检测失败",
        backendError,
        backendErrorDetail: detail,
      }),
    ).toBeNull();
    // 无错误 / 后端无详情均为 null
    expect(
      resolveUpdateErrorDetail({ operationError: null, backendError, backendErrorDetail: detail }),
    ).toBeNull();
    expect(
      resolveUpdateErrorDetail({ operationError: backendError, backendError, backendErrorDetail: null }),
    ).toBeNull();
  });

  it("主按钮分派：下载中 > 已下载可安装 > 可下载 > 检查", () => {
    expect(resolveUpdateAction({ downloading: true, canDownload: true, hasDownloaded: true }))
      .toBe("downloading");
    expect(resolveUpdateAction({ downloading: false, canDownload: true, hasDownloaded: true }))
      .toBe("install");
    expect(resolveUpdateAction({ downloading: false, canDownload: true, hasDownloaded: false }))
      .toBe("download");
    expect(resolveUpdateAction({ downloading: false, canDownload: false, hasDownloaded: false }))
      .toBe("check");
    // 后端清了下载记录（换版本）→ 不再提供安装入口
    expect(resolveUpdateAction({ downloading: false, canDownload: false, hasDownloaded: true }))
      .toBe("check");
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

  it("运行形态标签：安装版只显示架构，便携版追加便携标记", () => {
    expect(runtimeLabel("x64", false, "便携版")).toBe("x64");
    expect(runtimeLabel("ARM64", true, "便携版")).toBe("ARM64 · 便携版");
  });

  it("运行形态标签：平台缺失时退化为仅便携标记或空串", () => {
    expect(runtimeLabel(null, false, "便携版")).toBe("");
    expect(runtimeLabel("  ", false, "便携版")).toBe("");
    expect(runtimeLabel(null, true, "便携版")).toBe("便携版");
  });
});
