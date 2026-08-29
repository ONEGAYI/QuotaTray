export type RuntimePlatform = "desktop" | "android";

export interface RuntimeUiPolicy {
  mobile: boolean;
  hover: boolean;
  titleBar: boolean;
  tray: boolean;
  autostart: boolean;
  /** 更新页能力：两端渲染——桌面为检测/下载/NSIS 安装链，Android 为
   *  手动检测 + SAF 下载 + 系统安装器引导（2026-08-29 接入，验证状态
   *  以 AGENTS「移动端能力缺口追踪」为活追踪）。 */
  selfUpdate: boolean;
  cliAssist: boolean;
  fullScreenDialogs: boolean;
  bottomNavigation: boolean;
  /** 控制台直达入口（余额卡片；桌面图标钮 / Android trailing 文字按钮）：
   *  两端渲染（Android 命中区 44px，见 index.css 移动段与 console-link-spec
   *  §7）；验证状态与平台差异以 AGENTS「移动端能力缺口追踪」为活追踪。 */
  consoleLink: boolean;
}

export function runtimeUiPolicy(platform: RuntimePlatform): RuntimeUiPolicy {
  const mobile = platform === "android";
  return {
    mobile,
    hover: !mobile,
    titleBar: !mobile,
    tray: !mobile,
    autostart: !mobile,
    selfUpdate: true,
    cliAssist: !mobile,
    fullScreenDialogs: mobile,
    bottomNavigation: mobile,
    consoleLink: true,
  };
}

export type DisclosureAction = "toggle" | "select" | "dismiss";

export function reduceDisclosure(open: boolean, action: DisclosureAction): boolean {
  return action === "toggle" ? !open : false;
}

export function shouldCloseDialogOnPop(dialogId: string, nextDialogId: string | null): boolean {
  return dialogId !== nextDialogId;
}
