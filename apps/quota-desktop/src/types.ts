// core serde 形状的 TypeScript 镜像（字段名与 camelCase/序列化规则一一对应）。
// 修改 core 类型时需同步此处——形状漂移由试查/保存链路立刻暴露。

/** 单个套餐/时间窗口的用量数据（全部字段可缺省，对应 skip_serializing_if）。 */
export interface UsageData {
  plan_name?: string;
  total?: number;
  used?: number;
  remaining?: number;
  unit?: string;
  /** 额度窗口重置时刻（epoch 毫秒；订阅/限额窗口有值，余额类无）。 */
  reset_at?: number;
  is_valid?: boolean;
  invalid_message?: string;
  extra?: unknown;
}

/** 历史库单点的 IPC 镜像；字段名沿用 core HistoryPoint 的 snake_case。 */
export interface HistoryPoint {
  window_key: string;
  sampled_at: number;
  used?: number;
  remaining?: number;
  total?: number;
  unit?: string;
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

/** 查询方式（serde internally tagged：native / template / script，后两者平铺）。 */
export type ProviderKind =
  | { type: "native"; provider: string }
  | ({ type: "template" } & TemplateConfig)
  | { type: "script"; code: string; allowInsecure?: boolean };

/** 供应商条目（api_key_enc/api_key2_enc 为密文，仅结构回显；key 写入走单独通道）。 */
export interface ProviderEntry {
  id: string;
  name: string;
  kind: ProviderKind;
  enabled: boolean;
  api_key_enc?: string;
  /** 第二凭据槽密文（{{apiKey2}}，如 new-api 系站点的用户 ID） */
  api_key2_enc?: string;
  base_url?: string;
  /** 峰谷定价自定义（缺省 = 回退预置，见 core pricing::resolve） */
  pricing?: PricingConfig;
  /** 订阅套餐变体（缺省 = auto 自动推断；智谱系 v1 无周限 / v2+ 有周限） */
  plan_variant?: PlanVariant;
  /** 查询走代理（条目级开关，缺省 false；端口取设置的网络代理端口） */
  use_proxy?: boolean;
  /** 控制台直达 URL 覆盖（明文，非敏感；缺省 = native 条目回退预置默认） */
  console_url?: string;
}

/** 订阅套餐变体（core PlanVariant 镜像，serde snake_case）。 */
export type PlanVariant = "auto" | "no_weekly" | "weekly";

// ---- 峰谷定价（core pricing 模块镜像，snake_case 与宿主一致） ----

export type Weekday = "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun";

/** 一档价格（单位：每 MTokens；字段可部分缺失）。 */
export interface PriceTier {
  cache_hit_input?: number;
  cache_miss_input?: number;
  output?: number;
}

/** 高峰窗口：days 上每天的 [start, end)（左闭右开，同日不跨日）。 */
export interface PeakWindow {
  days: Weekday[];
  start: string;
  end: string;
}

/** 峰谷定价自定义（全部字段可缺省：缺省即回退预置）。 */
export interface PricingConfig {
  model?: string;
  /** UTC 偏移（分钟，东八区 = 480）；缺省 = 本地时区 */
  timezone_offset_minutes?: number;
  /** null = 回退预置；[] = 恒空闲 */
  windows?: PeakWindow[] | null;
  peak?: PriceTier;
  off_peak?: PriceTier;
  currency?: string;
}

/** 计费模式：按量 = 三档价格有效；订阅 = 积分/额度倍率（价格档为空，窗口表达折扣时段）。 */
export type PlanKind = "pay_as_you_go" | "subscription";

/** 预置单模型价格档（IPC 形状，来自 list_native_metas）。 */
export interface PresetModel {
  id: string;
  display: string;
  plan: PlanKind;
  /** 模型级窗口覆盖：null = 继承平台级（订阅项在此携带折扣时段）。 */
  windows: PeakWindow[] | null;
  peak: PriceTier;
  off_peak: PriceTier;
}

/** 峰谷定价预置（IPC 形状）。 */
export interface PresetPricing {
  currency: string;
  timezone_offset_minutes: number;
  windows: PeakWindow[];
  default_model: string;
  models: PresetModel[];
}

/** 用户自定义模型库条目（AppConfig.custom_models 的只读 GUI 镜像）。 */
export interface CustomModelDef {
  id: string;
  display: string;
  timezone_offset_minutes?: number;
  windows?: PeakWindow[] | null;
  peak?: PriceTier;
  off_peak?: PriceTier;
  currency?: string;
}

/** GUI 设置（settings.json，桌面/Android 共用字段，与 Rust Settings 一一对应）。 */
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
  /** 自动检查更新（应用运行期间轮询；CLI 启动钩子共用） */
  update_check_enabled: boolean;
  /** 上次自动检测时间（epoch 毫秒，null = 从未） */
  update_last_check: number | null;
  /** 更新通道代理端口（本机 HTTP 代理；null = 直连，CLI 共用） */
  update_proxy_port: number | null;
  /** 检测到新版本后自动下载安装包（仅安装版；完成后经消息中心询问安装） */
  update_auto_download: boolean;
  /** 系统通知总开关（两端共用；Android 侧叠加系统运行时权限层） */
  notifications_enabled: boolean;
  /** Android 后台刷新开关（WorkManager 周期查询；默认关，桌面不消费） */
  background_refresh_enabled: boolean;
  /** Android 后台刷新周期（分钟；系统硬限最小 15，收口 15–360） */
  background_refresh_interval_minutes: number;
}

/** 设置局部更新形状（与 Rust 侧 SettingsPatch 对应）：仅提交的字段
 * 会被后端覆盖，其余保持现值——单字段快切入口不得全量提交缓存设置
 * （历史 bug：陈旧缓存把代理端口等设置整体抹回默认值）。
 * 嵌套可空字段（如 tray_icon_entry_id）显式传 null 表示清空。 */
export interface SettingsPatch {
  refresh_interval_minutes?: number;
  low_balance_threshold_percent?: number;
  autostart?: boolean;
  language?: string;
  theme?: string;
  ring_units_per_circle?: number;
  tray_icon_entry_id?: string | null;
  update_check_enabled?: boolean;
  update_proxy_port?: number | null;
  update_auto_download?: boolean;
  notifications_enabled?: boolean;
  background_refresh_enabled?: boolean;
  background_refresh_interval_minutes?: number;
}

/** 错误双轨（kind 对齐 CLI --json 约定）。 */
export interface ErrorInfo {
  kind: "transient" | "deterministic";
  message: string;
  /** 排查详情（后端已脱敏的响应体片段等），仅供用户显式复制。 */
  detail?: string | null;
}

/** 检测到的可用新版本（IPC 形状，asset_url 仅后端下载用）。 */
export interface UpdateAvailable {
  version: string;
  html_url: string;
  notes: string | null;
  asset_name: string | null;
  asset_size: number | null;
  downloadable: boolean;
  asset_url: string | null;
}

/** get_boot_state 的 IPC 返回形状（前端首屏分支：便携首启确认页）。 */
export interface BootStateDto {
  ready: boolean;
  /** 便携首启待确认（ready=false 时的唯一成因）。 */
  pendingPortableInit: boolean;
  /** UI 壳层平台：Android 使用触摸优先布局，其余目标沿用桌面布局。 */
  platform: "desktop" | "android";
}

/** get_update_state 的 IPC 返回形状。 */
export interface UpdateStateDto {
  current_version: string;
  /** 运行架构标签（x64 / ARM64，编译期确定，与 CLI --version 一致）。 */
  platform: string;
  /** 便携形态（数据根存在 portable.key；安装版恒 false）。 */
  portable: boolean;
  /** 当前构建使用 zip 手动覆盖更新。 */
  manual_update: boolean;
  last_check: number | null;
  available: UpdateAvailable | null;
  last_error: string | null;
  /** last_error 的详情（如限流 403 的 GitHub message），悬停展示；null = 无详情。 */
  last_error_detail: string | null;
  /** 已下载安装包的完整路径（与当前 available 资产匹配；null = 未下载）。 */
  downloaded_path: string | null;
}

/** 后端下载器推送的实时安装包下载状态。 */
export interface DownloadProgress {
  downloaded_bytes: number;
  /** null 表示服务器未返回 Content-Length。 */
  total_bytes: number | null;
  /** 从下载开始计算的平均字节/秒。 */
  bytes_per_second: number;
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
  /** 峰谷定价预置（平台无预置则 null） */
  pricing: PresetPricing | null;
  /** 按余额查询币种选择的预置套；当前 DeepSeek 同时提供 CNY/USD。 */
  pricing_by_currency: Record<string, PresetPricing>;
  /** 该 native id 下由 CLI/config 定义的用户模型，只读展示与选择。 */
  custom_models: CustomModelDef[];
  /** 是否支持套餐变体声明（智谱系订阅套餐），编辑表单据此展示选择。 */
  supports_plan_variant: boolean;
  /** CLI 凭据型平台（订阅四家）：凭据查询时读本机官方 CLI 登录文件，
   *  编辑表单隐藏 key 输入框并展示提示卡。 */
  uses_cli_credentials: boolean;
  /** 双凭据 native 平台（当前仅阿里云余额）：api_key=AccessKey ID、
   *  api_key2=AccessKey Secret，编辑表单渲染必填语义的第二凭据槽。 */
  uses_api_key2: boolean;
  /** 控制台直达预置 URL（条目自定义覆盖优先；null = 该平台无预置） */
  console_url: string | null;
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
