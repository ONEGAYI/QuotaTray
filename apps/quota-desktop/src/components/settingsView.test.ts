import { describe, expect, it } from "vitest";
import {
  downloadPercent,
  formatBytes,
  formatDownloadProgress,
  resolveNotificationPermissionAction,
  backgroundIntervalOptions,
  resolveTabOnOpen,
  resolveUpdateAction,
  resolveUpdateError,
  resolveUpdateErrorDetail,
  resolveErrorDetailExpanded,
  resolveUpdateStatus,
  runtimeLabel,
  savedApkIsCurrent,
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

  it("移动端错误详情 disclosure：detail 在场时展开态跟随点击，消失时强制收起", () => {
    // 悬停气泡在移动端被全局禁用，详情唯一通路是点击展开（T-010）
    expect(resolveErrorDetailExpanded(false, "API rate limit exceeded")).toBe(false);
    expect(resolveErrorDetailExpanded(true, "API rate limit exceeded")).toBe(true);
    // 错误清空/换源无详情：旧展开态不得残留到下一次渲染
    expect(resolveErrorDetailExpanded(true, null)).toBe(false);
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

  it("zip 更新形态：已下载动作是打开下载目录，不提供运行安装包", () => {
    expect(
      resolveUpdateAction({ downloading: false, canDownload: true, hasDownloaded: true, manualUpdate: true }),
    ).toBe("open-dir");
    expect(
      resolveUpdateAction({ downloading: false, canDownload: true, hasDownloaded: false, manualUpdate: true }),
    ).toBe("download");
    // x64 安装态默认仍运行 setup
    expect(
      resolveUpdateAction({ downloading: false, canDownload: true, hasDownloaded: true }),
    ).toBe("install");
  });

  it("Android：APK 已保存到 SAF 位置时动作是移动安装，优先于桌面分流", () => {
    // 移动端 downloaded_path 恒空（content URI 不入后端状态表），「已下载」
    // 由 mobileSaved 表达；manualUpdate=true（APK 形态推导）不再走 open-dir
    expect(
      resolveUpdateAction({
        downloading: false,
        canDownload: true,
        hasDownloaded: false,
        manualUpdate: true,
        mobileSaved: true,
      }),
    ).toBe("install-mobile");
    // 未保存时仍是下载；下载中拦截一切
    expect(
      resolveUpdateAction({
        downloading: false,
        canDownload: true,
        hasDownloaded: false,
        manualUpdate: true,
        mobileSaved: false,
      }),
    ).toBe("download");
    expect(
      resolveUpdateAction({
        downloading: true,
        canDownload: true,
        hasDownloaded: false,
        mobileSaved: true,
      }),
    ).toBe("downloading");
    // 换版本/检测失败后 savedApkUri 已由重检测失效：无可下载版本回到检查
    expect(
      resolveUpdateAction({
        downloading: false,
        canDownload: false,
        hasDownloaded: false,
        mobileSaved: true,
      }),
    ).toBe("check");
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

describe("savedApkIsCurrent：Android 已保存 APK 的版本快照有效性", () => {
  it("未保存（null）恒不可用", () => {
    expect(savedApkIsCurrent(null, "0.8.1")).toBe(false);
    expect(savedApkIsCurrent(null, null)).toBe(false);
  });

  it("快照与当前可用版本一致时可用（同版本重检不清空 18MB 产物）", () => {
    const saved = { uri: "content://downloads/42", version: "0.8.1" };
    expect(savedApkIsCurrent(saved, "0.8.1")).toBe(true);
  });

  it("重检测出新版本时自动失效（旧包不该再装）", () => {
    const saved = { uri: "content://downloads/42", version: "0.8.1" };
    expect(savedApkIsCurrent(saved, "0.9.0")).toBe(false);
  });

  it("available 为 null 时仅 null 快照匹配（版本未知不装旧包）", () => {
    expect(savedApkIsCurrent({ uri: "content://1", version: null }, null)).toBe(true);
    expect(savedApkIsCurrent({ uri: "content://1", version: "0.8.1" }, null)).toBe(false);
  });
});

describe("通知权限行动作", () => {
  const base = { mobile: true, notificationsEnabled: true, permission: "prompt" };

  it("桌面与开关关闭时无权限行（桌面无运行时权限概念）", () => {
    expect(resolveNotificationPermissionAction({ ...base, mobile: false })).toBe("none");
    expect(
      resolveNotificationPermissionAction({ ...base, notificationsEnabled: false }),
    ).toBe("none");
  });

  it("未请求过（prompt 系）显示请求按钮——点按弹系统对话框", () => {
    expect(resolveNotificationPermissionAction({ ...base, permission: "prompt" })).toBe(
      "request",
    );
    expect(
      resolveNotificationPermissionAction({ ...base, permission: "prompt-with-rationale" }),
    ).toBe("request");
  });

  it("拒绝过（denied）改为引导跳系统设置——Android 13+ 不再弹对话框", () => {
    expect(resolveNotificationPermissionAction({ ...base, permission: "denied" })).toBe(
      "open-settings",
    );
  });

  it("已授权与未加载不显示动作（加载完成后 granted 即终态）", () => {
    expect(resolveNotificationPermissionAction({ ...base, permission: "granted" })).toBe(
      "none",
    );
    expect(resolveNotificationPermissionAction({ ...base, permission: null })).toBe("none");
  });
});

describe("设置页签消费时序", () => {
  it("打开时消费 initialTab（覆盖当前页签，支持消息卡片直达）", () => {
    expect(resolveTabOnOpen(true, "update", "general")).toBe("update");
  });

  it("开着期间直达入口变化同样消费（设置页已开时再次触发直达）", () => {
    expect(resolveTabOnOpen(true, "data", "update")).toBe("data");
  });

  it("关闭/未打开不消费——页签状态保持（重置由 onClose 负责）", () => {
    expect(resolveTabOnOpen(false, "update", "general")).toBe("general");
  });
});

describe("后台刷新周期档位", () => {
  it("档位与后端 sanitize 区间一致且文案按分钟/小时分流", () => {
    const options = backgroundIntervalOptions();
    expect(options.map((o) => o.minutes)).toEqual([15, 30, 60, 120, 360]);
    expect(options[0]).toEqual({ minutes: 15, kind: "minutes", unit: 15 });
    expect(options[1]).toEqual({ minutes: 30, kind: "minutes", unit: 30 });
    expect(options[2]).toEqual({ minutes: 60, kind: "hours", unit: 1 });
    expect(options[4]).toEqual({ minutes: 360, kind: "hours", unit: 6 });
    // 全部档位落在后端收口区间（15..=360），不出现被 sanitize 改写的中间态
    for (const option of options) {
      expect(option.minutes).toBeGreaterThanOrEqual(15);
      expect(option.minutes).toBeLessThanOrEqual(360);
    }
  });
});
