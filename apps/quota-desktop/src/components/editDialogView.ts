// EditDialog 保存键策略（从组件内联抽出，供契约测试）。
// 红线 3：空输入 = 保持不变；CLI 凭据型平台与（非双凭据的）native 平台
// 永不写 key——跨平台切换残留的输入不得落 vault。

export type EditTab = "native" | "template" | "script";

export function resolveSaveKeys(input: {
  tab: EditTab;
  usesCliCredentials: boolean;
  usesApiKey2: boolean;
  apiKey: string;
  apiKey2: string;
}): { saveKey: string | null; saveKey2: string | null } {
  // CLI 凭据型平台凭据来自本机 CLI 登录文件：主 key 永不写入
  const saveKey =
    input.tab === "native" && input.usesCliCredentials
      ? null
      : input.apiKey.trim()
        ? input.apiKey
        : null;
  // 第二槽：native 仅双凭据平台（如阿里云余额的 AccessKey Secret）写入，
  // 其余 native 平台恒 null；template/script 空输入 = 保持不变
  const saveKey2 =
    input.tab === "native" && !input.usesApiKey2
      ? null
      : input.apiKey2.trim()
        ? input.apiKey2
        : null;
  return { saveKey, saveKey2 };
}
