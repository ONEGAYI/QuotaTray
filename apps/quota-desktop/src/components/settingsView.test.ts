import { describe, expect, it } from "vitest";
import { en } from "../i18n/en";
import { zh } from "../i18n/zh";
import {
  catalogDescription,
  downloadPercent,
  formatBytes,
  formatDownloadProgress,
  proxyHostFromInput,
  proxyPortFromInput,
  resolveNotificationPermissionAction,
  backgroundIntervalOptions,
  SETTINGS_TAB_ORDER,
  resolveCatalogScheduleHint,
  resolveTabOnOpen,
  resolveUpdateAction,
  resolveUpdateError,
  resolveUpdateErrorDetail,
  resolveErrorDetailExpanded,
  resolveUpdateStatus,
  runtimeLabel,
  savedApkIsCurrent,
  type SettingsTab,
  thresholdCombinationValid,
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

describe("设置页签集合（#133 网络环境页）", () => {
  it("页签顺序：常规 → 更新 → 网络环境 → 数据管理", () => {
    expect([...SETTINGS_TAB_ORDER]).toEqual(["general", "update", "network", "data"]);
  });

  it("页签顺序与联合类型一致（导航渲染不出现类型外的页签）", () => {
    const allTabs: SettingsTab[] = ["general", "update", "network", "data"];
    for (const tab of SETTINGS_TAB_ORDER) expect(allTabs).toContain(tab);
  });

  it("打开设置可直达网络环境页（更新页指路入口的目标页签）", () => {
    expect(resolveTabOnOpen<SettingsTab>(true, "network", "general")).toBe("network");
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

describe("目录状态行描述（catalogDescription，#134）", () => {
  const NOW = Date.UTC(2026, 8, 26, 12, 0, 0);
  const base = {
    revision: 42,
    origin: "cached" as const,
    fallback_reason: null,
    last_success_ms: null,
  };

  it("未加载（undefined）返回空串", () => {
    expect(catalogDescription(undefined, { lang: "zh", autoUpdate: true, nowMs: NOW })).toBe("");
  });

  it("基础态：revision 与来源标签双语（bundled / cached）", () => {
    expect(
      catalogDescription(
        { ...base, origin: "bundled", last_attempt_ms: null, last_error: null },
        { lang: "zh", autoUpdate: true, nowMs: NOW },
      ),
    ).toBe("revision 42 · 内置");
    expect(
      catalogDescription(
        { ...base, origin: "bundled", last_attempt_ms: null, last_error: null },
        { lang: "en", autoUpdate: true, nowMs: NOW },
      ),
    ).toBe("revision 42 · bundled");
    expect(
      catalogDescription(
        { ...base, last_attempt_ms: null, last_error: null },
        { lang: "zh", autoUpdate: false, nowMs: NOW },
      ),
    ).toBe("revision 42 · 已缓存");
    expect(
      catalogDescription(
        { ...base, last_attempt_ms: null, last_error: null },
        { lang: "en", autoUpdate: false, nowMs: NOW },
      ),
    ).toBe("revision 42 · cached");
  });

  it("最近检查跟随注入时钟：成功态呈现相对时间", () => {
    expect(
      catalogDescription(
        { ...base, last_attempt_ms: NOW - 2 * 3_600_000, last_error: null },
        { lang: "zh", autoUpdate: true, nowMs: NOW },
      ),
    ).toBe("revision 42 · 已缓存 · 上次检查 2 小时前");
    expect(
      catalogDescription(
        { ...base, last_attempt_ms: NOW - 2 * 3_600_000, last_error: null },
        { lang: "en", autoUpdate: true, nowMs: NOW },
      ),
    ).toBe("revision 42 · cached · last checked 2h ago");
  });

  it("从未检查（last_attempt 缺失）不出现「上次检查」段", () => {
    const text = catalogDescription(
      { ...base, last_attempt_ms: null, last_error: null },
      { lang: "zh", autoUpdate: true, nowMs: NOW },
    );
    expect(text).not.toContain("上次检查");
  });

  it("检查失败且自动更新开启：失败标记附 30 分钟重试口径", () => {
    expect(
      catalogDescription(
        { ...base, last_attempt_ms: NOW - 3 * 60_000, last_error: "HTTP 500" },
        { lang: "zh", autoUpdate: true, nowMs: NOW },
      ),
    ).toBe("revision 42 · 已缓存 · 上次检查 3 分钟前（失败，至少 30 分钟后自动重试）");
    expect(
      catalogDescription(
        { ...base, last_attempt_ms: NOW - 3 * 60_000, last_error: "HTTP 500" },
        { lang: "en", autoUpdate: true, nowMs: NOW },
      ),
    ).toBe("revision 42 · cached · last checked 3m ago (failed; retries no sooner than 30 minutes later)");
  });

  it("检查失败且自动更新关闭：失败可见但不承诺自动重试", () => {
    const zhText = catalogDescription(
      { ...base, last_attempt_ms: NOW - 3 * 60_000, last_error: "HTTP 500" },
      { lang: "zh", autoUpdate: false, nowMs: NOW },
    );
    expect(zhText).toContain("失败");
    expect(zhText).not.toContain("重试");
    const enText = catalogDescription(
      { ...base, last_attempt_ms: NOW - 3 * 60_000, last_error: "HTTP 500" },
      { lang: "en", autoUpdate: false, nowMs: NOW },
    );
    expect(enText).toContain("failed");
    expect(enText).not.toContain("retr");
  });
});

describe("目录自动更新周期小字（#134）", () => {
  it("开关状态映射：开启分平台（桌面/移动措辞分叉），关闭统一", () => {
    expect(resolveCatalogScheduleHint({ enabled: true, mobile: false })).toBe("on-desktop");
    expect(resolveCatalogScheduleHint({ enabled: true, mobile: true })).toBe("on-mobile");
    expect(resolveCatalogScheduleHint({ enabled: false, mobile: false })).toBe("off");
    expect(resolveCatalogScheduleHint({ enabled: false, mobile: true })).toBe("off");
  });

  it("三键中英成对非空互异（漏键由 en 的 Record 类型在编译期拦截）", () => {
    const keys = [
      "settings.catalogScheduleOnDesktop",
      "settings.catalogScheduleOnMobile",
      "settings.catalogScheduleOff",
    ] as const;
    for (const key of keys) {
      expect(zh[key].trim()).not.toBe("");
      expect(en[key].trim()).not.toBe("");
      expect(zh[key]).not.toBe(en[key]);
    }
  });

  it("开启文案说明约每 6 小时检查（中英一致口径）", () => {
    expect(zh["settings.catalogScheduleOnDesktop"]).toContain("6 小时");
    expect(en["settings.catalogScheduleOnDesktop"]).toContain("6 hours");
    expect(zh["settings.catalogScheduleOnMobile"]).toContain("6 小时");
    expect(en["settings.catalogScheduleOnMobile"]).toContain("6 hours");
  });

  it("Android 口径：开启文案以前台为准，不暗示退后台仍定时联网", () => {
    expect(zh["settings.catalogScheduleOnMobile"]).toContain("前台");
    expect(en["settings.catalogScheduleOnMobile"]).toContain("foreground");
    expect(zh["settings.catalogScheduleOnMobile"]).not.toContain("后台");
    expect(en["settings.catalogScheduleOnMobile"].toLowerCase()).not.toContain("background");
  });

  it("关闭文案：说明仍可手动「立即更新」", () => {
    expect(zh["settings.catalogScheduleOff"]).toContain("立即更新");
    expect(en["settings.catalogScheduleOff"]).toContain("Update now");
  });

  it("目录相关文案不声称每日更新（开关说明与小字全量排查）", () => {
    const keys = [
      "settings.catalogAutoUpdateHint",
      "settings.catalogScheduleOnDesktop",
      "settings.catalogScheduleOnMobile",
      "settings.catalogScheduleOff",
    ] as const;
    for (const key of keys) {
      for (const text of [zh[key], en[key]]) {
        const lowered = text.toLowerCase();
        expect(lowered).not.toMatch(/每天|每日|daily|every day/);
      }
    }
  });
});

describe("阈值组合校验（双剩余口径：恢复剩余阈值 vs 低余额剩余阈值）", () => {
  it("合法组合：恢复剩余阈值严格高于低余额剩余阈值", () => {
    // 默认组合（低余额 20、恢复 95，均剩余语义）
    expect(thresholdCombinationValid(20, 95)).toBe(true);
    // 恢复线高出一点即合法
    expect(thresholdCombinationValid(20, 21)).toBe(true);
    expect(thresholdCombinationValid(5, 95)).toBe(true);
    // 边界极端值
    expect(thresholdCombinationValid(0, 1)).toBe(true);
    expect(thresholdCombinationValid(99, 100)).toBe(true);
  });

  it("非法组合：恢复线 ≤ 低余额线（恢复线不高于低余额线）", () => {
    // 两线相等：恢复线贴住低余额线
    expect(thresholdCombinationValid(20, 20)).toBe(false);
    expect(thresholdCombinationValid(6, 6)).toBe(false);
    // 恢复线低于低余额线
    expect(thresholdCombinationValid(50, 30)).toBe(false);
    // 极端：两线同拉满（剩余 100 同时落在两个判定区间，非法）
    expect(thresholdCombinationValid(100, 100)).toBe(false);
  });
});

describe("代理字段 draft 往返（#133 网络环境页）", () => {
  it("编辑变换：主机非空原样进 draft，空串归 null（清空语义）", () => {
    expect(proxyHostFromInput("proxy.lan")).toBe("proxy.lan");
    expect(proxyHostFromInput("")).toBeNull();
  });

  it("编辑变换：端口空/非法归 null（直连），数值收进 1..65535", () => {
    expect(proxyPortFromInput("7890")).toBe(7890);
    expect(proxyPortFromInput("")).toBeNull();
    expect(proxyPortFromInput("abc")).toBeNull();
    expect(proxyPortFromInput("0")).toBe(1);
    expect(proxyPortFromInput("70000")).toBe(65535);
    expect(proxyPortFromInput("7890.6")).toBe(7891);
  });

  it("打开→编辑→保存→重开往返一致：显示侧格式化与编辑变换互逆", () => {
    // 已保存值经 input 显示格式化（?? "" / String）再走编辑变换，
    // 不改值时回到原值——重开后表单显示与 draft 一致
    const host = "proxy.lan";
    expect(proxyHostFromInput(host ?? "")).toBe(host);
    const port = 7890;
    expect(proxyPortFromInput(String(port ?? ""))).toBe(port);
    // null（未配置/直连）经显示格式化（?? "" / String）归空串，再保存仍 null
    expect(proxyHostFromInput("")).toBeNull();
    expect(proxyPortFromInput("")).toBeNull();
  });
});

describe("低余额阈值与消息文案（剩余语义，T-22）", () => {
  it("settings 阈值说明为剩余方向（双语成对，不含已用措辞）", () => {
    expect(zh["settings.thresholdHint"]).toContain("剩余");
    expect(zh["settings.thresholdHint"]).not.toContain("已用");
    expect(en["settings.thresholdHint"]).toContain("remaining");
    expect(en["settings.thresholdHint"]).not.toContain("usage reaches");
    // 与恢复阈值提示同为剩余比例口径（T-21 已改，方向一致不回退）
    expect(zh["settings.recoveryThresholdHint"]).toContain("剩余");
    expect(en["settings.recoveryThresholdHint"]).toContain("remaining");
  });

  it("msgCenter 低余额正文与后端 low_balance_notify_body 成对：剩余措辞、占位 remaining", () => {
    expect(zh["msgCenter.lowBalanceBody"]).toBe("{name} 剩余 {remaining}%");
    expect(en["msgCenter.lowBalanceBody"]).toBe("{name} has {remaining}% left");
  });

  it("hover 主数值 label 翻转为剩余额度（键随语义更名，双语成对）", () => {
    expect(zh["hover.remainingQuota"]).toBe("剩余额度");
    expect(en["hover.remainingQuota"]).toBe("Remaining");
    // 旧键不得残留（防止引用悬空或口径回退）
    expect("hover.usedQuota" in zh).toBe(false);
    expect("hover.usedQuota" in en).toBe(false);
  });
});
