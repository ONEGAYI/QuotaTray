/** 模板编辑器的预设模板库（GUI 内嵌副本，与 examples/templates 五种
 * 形态对齐——custom 为通用骨架无对应示例文件；示例目录更新时同步
 * 维护）。选中预设即整体填入编辑器，用户可在此基础上继续手改。 */

export interface PresetTemplate {
  id: "custom" | "balance" | "site" | "credits" | "windows" | "newapi";
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
  {
    id: "newapi",
    json: `{
  "request": {
    "url": "{{baseUrl}}/api/user/self",
    "headers": {
      "Authorization": "Bearer {{apiKey}}",
      "New-Api-User": "1"
    }
  },
  "extract": {
    "planName": "$.data.group",
    "remaining": "$.data.quota",
    "used": "$.data.used_quota",
    "isValid": "$.success",
    "invalidMessage": "$.message",
    "unit": { "const": "USD" }
  },
  "transforms": [
    { "op": "divide", "field": "remaining", "by": 500000 },
    { "op": "round", "field": "remaining", "digits": 2 },
    { "op": "divide", "field": "used", "by": 500000 },
    { "op": "round", "field": "used", "digits": 2 }
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

/** serde 序列化会补全的缺省键（对照 core TemplateConfig/TemplateRequest/
 * WindowSpec 定义，无 skip_serializing_if 的 default 字段）：
 * 保存过的条目重新编辑时，编辑器内容经后端 JSON 往返会多出这些键——
 * 语义等价比较时，一侧缺省、另一侧为对应缺省形态视为相等。
 * method 的缺省是具体值 "GET"，其余均为空值形态。 */
const SERDE_EMPTY_DEFAULTS: ReadonlySet<string> = new Set([
  "extract", // {}（ExtractSpec 全字段 skip，空对象即缺省）
  "transforms", // []
  "windowsFrom", // null
  "windows", // []
  "allowInsecure", // false
  "headers", // {}
]);
const SERDE_VALUE_DEFAULTS: Readonly<Record<string, unknown>> = {
  method: "GET",
};

function isEmptyValue(value: unknown): boolean {
  if (value == null || value === false) return true;
  if (Array.isArray(value)) return value.length === 0;
  if (typeof value === "object") return Object.keys(value).length === 0;
  return false;
}

function isSerdeDefault(key: string, value: unknown): boolean {
  if (key in SERDE_VALUE_DEFAULTS) return value === SERDE_VALUE_DEFAULTS[key];
  return SERDE_EMPTY_DEFAULTS.has(key) && isEmptyValue(value);
}

/** 深比较两个已 parse 的配置对象（键序无关）。`withDefaults` 标记当前层
 * 是否应用缺省宽容——仅 TemplateConfig（顶层）、TemplateRequest
 * （request 键下）、WindowSpec（windows 数组项）三层。 */
function semanticEquals(a: unknown, b: unknown, withDefaults: boolean): boolean {
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    // 数组项继承宽容：顶层 windows 项即 WindowSpec（Transform 等元素
    // 无白名单同名键，继承无宽容面）
    return a.every((item, i) => semanticEquals(item, b[i], withDefaults));
  }
  if (typeof a === "object" && a !== null && typeof b === "object" && b !== null) {
    const recordA = a as Record<string, unknown>;
    const recordB = b as Record<string, unknown>;
    for (const key of new Set([...Object.keys(recordA), ...Object.keys(recordB)])) {
      const inA = key in recordA;
      const inB = key in recordB;
      if (inA && inB) {
        // request 子层是 TemplateRequest、windows 数组项是 WindowSpec，
        // 这两支继承宽容；其余子层（extract 内部、headers map 等）不应用
        const childDefaults = withDefaults && (key === "request" || key === "windows");
        if (!semanticEquals(recordA[key], recordB[key], childDefaults)) return false;
      } else if (withDefaults && isSerdeDefault(key, (inA ? recordA : recordB)[key])) {
        // 一侧缺省、另一侧是 serde 补全的缺省形态 → 等价
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
