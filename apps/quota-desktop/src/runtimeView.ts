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
  };
}

export type DisclosureAction = "toggle" | "select" | "dismiss";

export function reduceDisclosure(open: boolean, action: DisclosureAction): boolean {
  return action === "toggle" ? !open : false;
}

export function shouldCloseDialogOnPop(dialogId: string, nextDialogId: string | null): boolean {
  return dialogId !== nextDialogId;
}
