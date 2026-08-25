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
      "name": "Quota",
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

/** serde 序列化 TemplateConfig（顶层与 windows 项）会补全的缺省键：
 * 保存过的条目重新编辑时，编辑器内容经后端 JSON 往返会多出这些
 * 空值键——语义等价比较时，一侧缺省、另一侧为对应空值视为相等。 */
const SERDE_DEFAULTS: ReadonlySet<string> = new Set([
  "extract",
  "transforms",
  "windowsFrom",
  "windows",
  "allowInsecure",
]);

function isEmptyValue(value: unknown): boolean {
  if (value == null || value === false) return true;
  if (Array.isArray(value)) return value.length === 0;
  if (typeof value === "object") return Object.keys(value).length === 0;
  return false;
}

/** 深比较两个已 parse 的配置对象（键序无关）；`withDefaults` 标记当前层
 * 是否为 TemplateConfig / windows 项（应用 SERDE_DEFAULTS 缺省宽容）。 */
function semanticEquals(a: unknown, b: unknown, withDefaults: boolean): boolean {
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((item, i) => semanticEquals(item, b[i], withDefaults));
  }
  if (typeof a === "object" && a !== null && typeof b === "object" && b !== null) {
    const recordA = a as Record<string, unknown>;
    const recordB = b as Record<string, unknown>;
    for (const key of new Set([...Object.keys(recordA), ...Object.keys(recordB)])) {
      const inA = key in recordA;
      const inB = key in recordB;
      if (inA && inB) {
        if (!semanticEquals(recordA[key], recordB[key], false)) return false;
      } else if (withDefaults && SERDE_DEFAULTS.has(key)) {
        // 一侧缺省、另一侧是 serde 补全的空值 → 等价；非空值则是真实差异
        const present = (inA ? recordA : recordB)[key];
        if (!isEmptyValue(present)) return false;
      } else {
        return false;
      }
    }
    return true;
  }
  return a === b;
}

/** 编辑器内容与某预设语义等价时返回其 id（预设按钮组高亮依据）。
 * 比较在 JSON 层进行：格式差异（空白/键序）与 serde 往返补全的缺省
 * 空值键（transforms: [] 等）不算改动；取值路径等实质差异才视为
 * 脱离预设。解析失败返回 null。 */
export function matchedPresetId(json: string): PresetTemplate["id"] | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return null;
  }
  return (
    PRESET_TEMPLATES.find((p) => {
      try {
        return semanticEquals(parsed, JSON.parse(p.json), true);
      } catch {
        return false;
      }
    })?.id ?? null
  );
}
