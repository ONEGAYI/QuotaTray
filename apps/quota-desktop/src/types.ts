// core serde 形状的 TypeScript 镜像（字段名与 camelCase/序列化规则一一对应）。
// 修改 core 类型时需同步此处——形状漂移由试查/保存链路立刻暴露。

/** 单个套餐/时间窗口的用量数据（全部字段可缺省，对应 skip_serializing_if）。 */
export interface UsageData {
  plan_name?: string;
  total?: number;
  used?: number;
  remaining?: number;
  unit?: string;
  is_valid?: boolean;
  invalid_message?: string;
  extra?: unknown;
}

/** 模板请求定义。 */
export interface TemplateRequest {
  method?: "GET" | "POST";
  url: string;
  headers?: Record<string, string>;
  body?: string;
}

/** 字段来源：JSONPath 字符串或 { const } 常量（serde untagged）。 */
export type FieldSource = string | { const: unknown };

export interface ExtractSpec {
  planName?: FieldSource;
  total?: FieldSource;
  used?: FieldSource;
  remaining?: FieldSource;
  unit?: FieldSource;
  isValid?: FieldSource;
  invalidMessage?: FieldSource;
}

export interface WindowSpec {
  name: string;
  extract: ExtractSpec;
  transforms?: Transform[];
}

export type Transform =
  | { op: "multiply"; field: "total" | "used" | "remaining"; by: number }
  | { op: "divide"; field: "total" | "used" | "remaining"; by: number }
  | { op: "add"; field: "total" | "used" | "remaining"; by: number }
  | { op: "sub"; field: "total" | "used" | "remaining"; by: number }
  | { op: "round"; field: "total" | "used" | "remaining"; digits?: number };

export interface TemplateConfig {
  request: TemplateRequest;
  extract?: ExtractSpec;
  transforms?: Transform[];
  windowsFrom?: string;
  windows?: WindowSpec[];
  allowInsecure?: boolean;
}

/** 查询方式（serde internally tagged：{"type":"native"...} | {"type":"template", ...平铺}）。 */
export type ProviderKind =
  | { type: "native"; provider: string }
  | ({ type: "template" } & TemplateConfig);

/** 供应商条目（api_key_enc 为密文，仅结构回显；key 写入走单独通道）。 */
export interface ProviderEntry {
  id: string;
  name: string;
  kind: ProviderKind;
  enabled: boolean;
  api_key_enc?: string;
  base_url?: string;
}

/** 桌面端设置（settings.json，字段与 Rust 侧 Settings 一一对应）。 */
export interface Settings {
  refresh_interval_minutes: number;
  low_balance_threshold_percent: number;
  autostart: boolean;
  /** "zh" | "en" | "system" */
  language: string;
  /** "light" | "dark" | "system" */
  theme: string;
  /** 托盘圆环每圈代表的余额数值 */
  ring_units_per_circle: number;
  /** 托盘图标显示的条目 id（null = 第一个启用条目） */
  tray_icon_entry_id: string | null;
}

/** 错误双轨（kind 对齐 CLI --json 约定）。 */
export interface ErrorInfo {
  kind: "transient" | "deterministic";
  message: string;
}

/** 查询结果（含 keep-last-good 保留的旧值）。 */
export interface QueryOutcome {
  ok: boolean;
  data: UsageData[] | null;
  error: ErrorInfo | null;
  at: number | null;
}

export interface NativeMeta {
  id: string;
  name: string;
}

export interface TemplateErrorDto {
  field: string;
  reason: string;
}

export interface SnapshotEntry {
  data: UsageData[];
  at: number;
}

/** keep-last-good 展示窗口：瞬时失败后保留旧值的时限（GUI-spec §3）。
 *  Rust 侧同值定义在 `src-tauri/src/tray.rs` 的 `KEEP_LAST_GOOD_MS`——两端同步修改。 */
export const KEEP_LAST_GOOD_MS = 10 * 60 * 1000;
