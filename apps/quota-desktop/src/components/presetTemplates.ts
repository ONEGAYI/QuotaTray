/** 模板编辑器的预设模板库（GUI 内嵌副本，与 examples/templates 四种
 * 形态对齐；示例目录更新时同步维护）。选中预设即整体填入编辑器，
 * 用户可在此基础上继续手改。 */

export interface PresetTemplate {
  id: "custom" | "balance" | "site" | "credits" | "windows";
  /** 完整模板 JSON 文本。 */
  json: string;
}

export const PRESET_TEMPLATES: readonly PresetTemplate[] = [
  {
    id: "custom",
    json: `{
  "request": {
    "url": "{{baseUrl}}/v1/user/info",
    "headers": { "Authorization": "Bearer {{apiKey}}" }
  },
  "extract": {
    "remaining": "$.data.totalBalance",
    "unit": { "const": "CNY" }
  }
}`,
  },
  {
    id: "balance",
    json: `{
  "request": {
    "url": "https://api.deepseek.com/user/balance",
    "headers": {
      "Authorization": "Bearer {{apiKey}}"
    }
  },
  "extract": {
    "planName": { "const": "DeepSeek" },
    "remaining": "$.balance_infos[0].total_balance",
    "unit": "$.balance_infos[0].currency",
    "isValid": "$.is_available"
  }
}`,
  },
  {
    id: "site",
    json: `{
  "request": {
    "url": "{{baseUrl}}/v1/user/info",
    "headers": {
      "Authorization": "Bearer {{apiKey}}"
    }
  },
  "extract": {
    "planName": { "const": "SiliconFlow" },
    "remaining": "$.data.totalBalance",
    "unit": { "const": "CNY" }
  },
  "transforms": [
    { "op": "round", "field": "remaining", "digits": 2 }
  ]
}`,
  },
  {
    id: "credits",
    json: `{
  "request": {
    "url": "https://openrouter.ai/api/v1/credits",
    "headers": {
      "Authorization": "Bearer {{apiKey}}"
    }
  },
  "extract": {
    "planName": { "const": "OpenRouter" },
    "total": "$.data.total_credits",
    "used": "$.data.total_usage",
    "unit": { "const": "USD" }
  },
  "transforms": [
    { "op": "round", "field": "used", "digits": 2 }
  ]
}`,
  },
  {
    id: "windows",
    json: `{
  "request": {
    "url": "{{baseUrl}}/usage",
    "headers": {
      "Authorization": "Bearer {{apiKey}}"
    }
  },
  "windowsFrom": "$.windows",
  "windows": [
    {
      "name": "配额窗口",
      "extract": {
        "used": "$.used",
        "total": "$.limit",
        "unit": "$.unit"
      },
      "transforms": [
        { "op": "round", "field": "used", "digits": 2 }
      ]
    }
  ]
}`,
  },
];

/** 取指定预设的 JSON 文本（id 不存在属编程错误，直接抛）。 */
export function presetJsonOf(id: PresetTemplate["id"]): string {
  const preset = PRESET_TEMPLATES.find((p) => p.id === id);
  if (!preset) throw new Error(`未知模板预设：${id}`);
  return preset.json;
}

/** 编辑器内容与某预设完全一致时返回其 id（预设按钮组高亮依据；
 * 手改任意字符即视为脱离预设，返回 null）。 */
export function matchedPresetId(json: string): PresetTemplate["id"] | null {
  return PRESET_TEMPLATES.find((p) => p.json === json)?.id ?? null;
}
