// 中文文案字典——前端 i18n 的类型基准（en.ts 必须完整覆盖这里的全部键）。
// 与 Rust 侧 `src-tauri/src/i18n.rs` 是平行双实现（成对约定）：
// 两端语义保持一致，修改任一侧须同步另一侧。

/** 实际渲染语言（"system" 在解析时折叠为此类型）。 */
export type UiLang = "zh" | "en";

export const zh = {
  // ---- 通用 ----
  "common.cancel": "取消",
  "common.save": "保存",
  "common.saving": "保存中…",

  // ---- 主窗口 ----
  "app.subtitle": "AI 账户余额监视",
  "app.add": "+ 添加",
  "app.settings": "设置",
  "app.loading": "加载中…",
  "app.configError": "配置读取失败：{msg}",
  "app.emptyTitle": "还没有供应商条目",
  "app.emptyHint": "点击右上角「添加」接入预置平台，或用模板 JSON 接入任意平台",

  // ---- 供应商卡片 ----
  "card.disabled": "已停用",
  "card.deterministic": "确定性失败",
  "card.invalid": "已失效",
  "card.staleKeep": "暂不可达 · 保留旧值",
  "card.network": "网络波动",
  "card.refresh": "手动刷新",
  "card.disable": "停用",
  "card.enable": "启用",
  "card.edit": "编辑",
  "card.remove": "删除",
  "card.confirmRemove": "确定删除「{name}」？",
  "card.disabledNote": "条目已停用，不参与查询",
  "card.invalidPrefix": "已失效：", // 与 tray.rs invalid_prefix 成对
  "card.noReason": "未说明原因", // 与 tray.rs no_invalid_reason 成对
  "card.noData": "尚无数据", // 与 tray.rs no_data 成对
  "card.querying": "查询中…",
  "card.windowN": "窗口{n}", // 与 tray.rs window_name 成对
  "card.totalQuota": "总额度 {total}",
  "card.keyConfigured": "已配置 key",
  "card.keyMissing": "未配置 key",
  "card.snapshotAt": "上次于 {time}（启动快照）",
  "card.refreshing": "刷新中…",
  "card.templateKind": "模板",

  // ---- 添加/编辑对话框 ----
  "edit.titleEdit": "编辑供应商",
  "edit.titleAdd": "添加供应商",
  "edit.tabNative": "预置平台",
  "edit.tabTemplate": "模板",
  "edit.tabScript": "脚本（M4）",
  "edit.name": "名称",
  "edit.namePlaceholder": "如 DeepSeek 主号",
  "edit.nameRequired": "名称不能为空",
  "edit.platform": "平台",
  "edit.platformPlaceholder": "请选择…",
  "edit.nativeRequired": "请选择预置平台",
  "edit.templateJson": "模板 JSON",
  "edit.baseUrl": "baseUrl（{{baseUrl}} 变量来源）",
  "edit.jsonError": "JSON 解析失败：{msg}",
  "edit.fieldError": "{field}：{reason}",
  "edit.platformOption": "{name}（{id}）",
  "edit.templateJsonError": "模板 JSON 解析失败",
  "edit.insecureWarn":
    "⚠ 模板启用了 allowInsecure：请求可经明文 http 传输，API key 存在被网络窃听的风险",
  "edit.validate": "校验",
  "edit.test": "试查",
  "edit.testing": "试查中…",
  "edit.validated": "校验通过",
  "edit.testOk": "试查成功",
  "edit.apiKey": "API key",
  "edit.keyConfigured": "已配置（留空保持不变）",
  "edit.keyMissing": "未配置",

  // ---- 自定义标题栏 ----
  "titlebar.language": "切换语言",
  "titlebar.theme": "切换主题",
  "titlebar.minimize": "最小化",
  "titlebar.maximize": "最大化",
  "titlebar.restore": "还原",
  "titlebar.close": "关闭",

  // ---- 设置对话框 ----
  "settings.title": "设置",
  "settings.interval": "自动刷新间隔（分钟）",
  "settings.threshold": "低额度提醒阈值（已用 %）",
  "settings.autostart": "开机自启",
  "settings.langZh": "中文",
  "settings.langEn": "English",
  "settings.langSystem": "跟随系统",
  "settings.themeLight": "明亮",
  "settings.themeDark": "暗色",
  "settings.themeSystem": "跟随系统",
  "settings.ringUnits": "托盘圆环每圈单位",
} as const;

export type TextKey = keyof typeof zh;
