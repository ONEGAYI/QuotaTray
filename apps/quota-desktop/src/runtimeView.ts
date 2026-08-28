export type RuntimePlatform = "desktop" | "android";

export interface RuntimeUiPolicy {
  mobile: boolean;
  hover: boolean;
  titleBar: boolean;
  tray: boolean;
  autostart: boolean;
  selfUpdate: boolean;
  cliAssist: boolean;
  fullScreenDialogs: boolean;
  bottomNavigation: boolean;
  /** 控制台直达入口（余额卡片图标按钮）：Android 的 opener 拉起系统
   *  浏览器行为未真机验证，桌面先行、移动默认隐藏（数据层跨端就绪）。 */
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
    selfUpdate: !mobile,
    cliAssist: !mobile,
    fullScreenDialogs: mobile,
    bottomNavigation: mobile,
    consoleLink: !mobile,
  };
}

export type DisclosureAction = "toggle" | "select" | "dismiss";

export function reduceDisclosure(open: boolean, action: DisclosureAction): boolean {
  return action === "toggle" ? !open : false;
}

export function shouldCloseDialogOnPop(dialogId: string, nextDialogId: string | null): boolean {
  return dialogId !== nextDialogId;
}
