//! 集中式文案表：`t(lang, key)` 取无参文案、`xxx(lang, …)` 取带参文案、
//! [`apply_help_lang`] 做 clap help 的运行时翻译。
//!
//! 纯函数、无全局状态：语言由调用方显式传入（经 `Ctx.lang` 或参数）。
//! clap 的 doc comment 是编译期中文默认，运行时以本表为准（两语言均覆盖，
//! 防止表与注释漂移）。

use crate::lang::Lang;
use clap::Command;

/// 无参 / 前缀型文案键。exhaustive match 保证每键双语齐全（漏译即编译错误）。
#[derive(Clone, Copy, Debug)]
pub enum T {
    // ---- 通用 ----
    /// 「错误：」前缀。
    Err,
    /// 「stdin 读取失败：」前缀。
    StdinReadFail,
    /// 「输入读取失败：」前缀。
    InputReadFail,
    /// 「选择读取失败：」前缀。
    SelectReadFail,
    /// 「key 读取失败：」前缀。
    KeyReadFail,
    /// 「JSON 解析失败：」前缀。
    JsonParseFail,
    /// 「静态校验失败：」前缀。
    StaticCheckFail,

    // ---- render 表头与状态 ----
    ColName,
    ColPlan,
    ColUsed,
    ColRemaining,
    ColUnit,
    ColReset,
    ColStatus,
    ColType,
    ColEnabled,
    ColKeySet,
    /// 查询成功但无数据行。
    OkNoData,
    /// 「失效：」前缀（is_valid=false 时拼接 invalid_message）。
    InvalidPrefix,
    /// 错误分类前缀（瞬时）。
    Transient,
    /// 错误分类前缀（确定性）。
    Deterministic,
    Yes,
    No,

    // ---- io ----
    NotTerminal,
    InterruptCtrlC,
    InterruptCtrlD,
    /// 剪贴板打开失败（内部错误前缀）。
    ClipboardOpenFail,
    /// 剪贴板读取失败（内部错误前缀）。
    ClipboardReadFail,
    /// 多行 JSON 输入方式的第二行提示。
    MultilineJsonHint,
    /// 多行 JS 代码输入方式的第二行提示（单独一行 `.` 结束）。
    MultilineCodeHint,

    // ---- list / query / natives ----
    ListEmpty,
    QueryNoEntries,
    NativesEmpty,

    // ---- add 向导与校验 ----
    PasteHintA,
    PasteHintB,
    NamePromptAdd,
    TypePrompt,
    /// 向导类型列表的 template 选项。
    TemplateOption,
    /// 套餐变体问询（订阅型平台的限额窗口结构声明）。
    PlanVariantPrompt,
    PlanVariantAuto,
    PlanVariantNoWeekly,
    PlanVariantWeekly,
    BaseUrlPromptAdd,
    KeyPromptSkip,
    /// 订阅四家：凭据来自本机官方 CLI 登录文件的提示行。
    CliCredentialNote,
    /// 条目级查询代理开关的向导问题。
    UseProxyPrompt,
    IdEmptyHint,
    NameEmpty,
    IdGenFail,
    EncryptFail,
    /// JsonParseFail 的 --json 模式后缀提示。
    EntryJsonFields,
    ApiKeyEncRejected,
    /// 「模板校验失败：」前缀。
    TplValidateFail,
    /// 「静态校验未通过：」前缀（向导重试场景）。
    ValidateFail,
    /// 重试提示第二行（Ctrl+C 放弃）。
    RetrySuffix,
    /// 「粘贴模板 JSON」多行输入 prompt（add 向导与 template test 共用）。
    PasteTemplateJson,
    /// 向导类型列表的 script 选项。
    ScriptOption,
    /// 「粘贴脚本 JS 代码」多行输入 prompt（add 向导与 script test 共用）。
    PasteScriptCode,
    /// 「脚本校验失败：」前缀。
    ScriptValidateFail,
    /// 向导：脚本是否访问非 HTTPS 地址的确认项。
    AllowInsecurePrompt,

    // ---- edit ----
    PasteHintEdit,
    NamePromptEdit,
    BaseUrlPromptEdit,
    CurrentTemplateLabel,
    PasteNewTplPrompt,
    /// 编辑 script 条目：当前代码标签 / 重粘贴 prompt / 无效保持前缀。
    CurrentScriptLabel,
    PasteNewScriptPrompt,
    InvalidScriptKeep,
    EnabledConfirm,
    /// 「模板无效（保持原模板）：」前缀。
    InvalidTplKeep,
    /// 空输入 = 保持不变（内部错误消息，用于区分"保持"与"无效"）。
    EmptyInputKeep,

    // ---- setkey ----
    SetKeyPrompt,
    KeyEmptyRejected,

    // ---- template ----
    NeedEntryOrJson,
    StaticCheckOk,
    /// 试查临时条目名（不落盘）。
    TestEntryName,
    /// script 试查临时条目名（不落盘）。
    ScriptTestEntryName,
    TryQueryEncryptFail,
    TryQueryFail,
    NeedsKeyPrompt,
    /// 试查交互输入的 key 为空（终端场景）。
    KeyEmptyHint,
    /// 试查 key 为空且 stdin 被输入重定向占用（无法交互补输）。
    KeyEmptyRedirect,
    /// `--json` 输入以 { 开头却不是合法脚本配置 JSON 的回退提示。
    LooksLikeJsonHint,
    /// 试查输出的「有效」标签（是/否复用 Yes/No）。
    LblValid,
    /// 向导列表项 id 与名称的排版连接符（zh 全角双破折号 / en 单破折号，
    /// 与 TemplateOption 的排版先例一致）。
    Dash,

    // ---- vault ----
    VaultStoreOk,
    VaultStoreNotInit,
    VaultStoreReadFail,
    VaultStoreHint,
    VaultHealthy,
    VaultOpenFail,
    /// ctx 侧的「凭据保险库打开失败：」前缀（与 vault 命令的措辞不同源）。
    VaultOpenFailCtx,
    EngineInitFail,
    ConfigFilePrefix,
    ConfigTransferFail,

    // ---- devsmoke（仅 debug）----
    SmokeKeyFileFormat,
    SmokeSkipBody,
    SmokeUnknownWarn,
    SmokeEncryptFail,
    SmokeAllPass,

    // ---- clap help（运行时覆盖 doc comment）----
    HelpAbout,
    HelpConfig,
    HelpLang,
    HelpList,
    HelpListJson,
    HelpQuery,
    HelpQueryIds,
    HelpQueryJson,
    HelpQueryWatch,
    HelpQueryInterval,
    HelpAdd,
    HelpAddJson,
    HelpEdit,
    HelpEditId,
    HelpEditEnable,
    HelpEditDisable,
    HelpRemove,
    HelpRemoveId,
    HelpRemoveYes,
    HelpSetKey,
    HelpSetKeyId,
    HelpNatives,
    HelpTemplate,
    HelpTemplateTest,
    HelpTemplateEntry,
    HelpTemplateJson,
    HelpTemplateBaseUrl,
    HelpVault,
    HelpVaultStatus,
    HelpConfigTransfer,
    HelpConfigExport,
    HelpConfigExportOutput,
    HelpConfigExportYes,
    HelpConfigImport,
    HelpConfigImportInput,
    HelpConfigImportYes,
    HelpUpdate,
    HelpUpdateCheck,
    HelpUpdateYes,
    HelpUpdateOutput,
    UpdateCheckFail,
    UpdateNoRelease,
    UpdateManualUrl,
    UpdateDownloading,
    UpdateDownloadFail,
    UpdateSaveFail,
    UpdateRunHint,
    UpdateClientFail,

    // ---- pricing ----
    /// 「⚡高峰」标签。
    PeakLabel,
    /// 「空闲」标签。
    OffPeakLabel,
    /// pricing 表「项目」列头。
    ColPriceItem,
    /// pricing 表「高峰」列头。
    ColPeak,
    /// pricing 表「空闲」列头。
    ColOffPeak,
    /// natives 表「峰谷预置」列头。
    ColPricing,
    /// 「输入（缓存命中）」行名。
    PriceCacheHit,
    /// 「输入（缓存未命中）」行名。
    PriceCacheMiss,
    /// 「输出」行名。
    PriceOutput,
    /// 条目无峰谷定价（无预置且未自定义）。
    PricingNotConfigured,
    /// 「本地时区」。
    PricingLocalTz,
    /// 未设高峰窗口（恒按空闲价）。
    PricingNoWindows,
    /// 「峰谷定价校验失败：」前缀。
    PricingValidateFail,
    /// 订阅积分制说明行（价格档不适用）。
    PricingPlanNote,
    /// model list 表「模型」列头。
    ColModel,
    /// model list 表「来源」列头。
    ColSource,
    /// model list 表「模式」列头。
    ColPlanKind,
    /// 计费模式：按量。
    ColPlanPayg,
    /// 计费模式：订阅积分。
    ColPlanSubscription,
    /// model list 表「峰价（命中/输入/输出）」列头。
    ColPeakPrice,
    /// model list 表「闲价（命中/输入/输出）」列头。
    ColOffPeakPrice,
    /// model list 来源：预置。
    PricingModelSourcePreset,
    /// model list 来源：自定义。
    PricingModelSourceCustom,
    /// 平台无预置与自定义模型。
    PricingModelListEmpty,
    HelpPricing,
    HelpPricingShow,
    HelpPricingShowId,
    HelpPricingShowJson,
    HelpPricingSet,
    HelpPricingSetId,
    HelpPricingClear,
    HelpPricingClearId,
    HelpPricingModel,
    HelpPricingModelList,
    HelpPricingModelListProvider,
    HelpPricingModelListJson,
    HelpPricingModelAdd,
    HelpPricingModelAddProvider,
    HelpPricingModelRemove,
    HelpPricingModelRemoveProvider,
    HelpPricingModelRemoveId,
    HelpDevSmoke,
    HelpDevSmokeKeyFile,
}

/// 取文案。`System` 未消解时按中文兜底（调用约定：先 `resolve` 再取文案）。
pub fn t(lang: Lang, key: T) -> &'static str {
    match lang {
        Lang::En => en(key),
        _ => zh(key),
    }
}

fn zh(key: T) -> &'static str {
    match key {
        T::Err => "错误：",
        T::StdinReadFail => "stdin 读取失败：",
        T::InputReadFail => "输入读取失败：",
        T::SelectReadFail => "选择读取失败：",
        T::KeyReadFail => "key 读取失败：",
        T::JsonParseFail => "JSON 解析失败：",
        T::StaticCheckFail => "静态校验失败：",

        T::ColName => "名称",
        T::ColPlan => "套餐",
        T::ColUsed => "已用",
        T::ColRemaining => "剩余",
        T::ColUnit => "单位",
        T::ColReset => "重置",
        T::ColStatus => "状态",
        T::ColType => "类型",
        T::ColEnabled => "启用",
        T::ColKeySet => "凭据已配",
        T::OkNoData => "OK（无数据）",
        T::InvalidPrefix => "失效：",
        T::Transient => "瞬时",
        T::Deterministic => "确定性",
        T::Yes => "是",
        T::No => "否",

        T::NotTerminal => "stderr 不是终端，无法交互输入（管道场景请把 key 写入 stdin）",
        T::InterruptCtrlC => "输入被 Ctrl+C 中断",
        T::InterruptCtrlD => "输入被 Ctrl+D 中断",
        T::ClipboardOpenFail => "打开失败：",
        T::ClipboardReadFail => "读取失败：",
        T::MultilineJsonHint => "（粘贴多行 JSON，输入单独空行结束；或 Ctrl+Z / Ctrl+D 结束输入）",
        T::MultilineCodeHint => "（粘贴多行代码，空行照常保留；输入完后在单独一行输入 . 结束）",

        T::ListEmpty => "还没有供应商条目；用 quota add 添加。",
        T::QueryNoEntries => {
            "没有可查询的条目；用 quota add 添加，或 quota query <id> 指定禁用条目。"
        }
        T::NativesEmpty => "（无预置平台）",

        T::PasteHintA => {
            "粘贴提示：名称 / base_url 输入框请用 Shift+Ctrl+V 或鼠标右键（Ctrl+V 在此不生效）；"
        }
        T::PasteHintB => "          API key 输入支持 Ctrl+V 粘贴，回显为星号。",
        T::NamePromptAdd => "名称（仅做标识符，用于列表展示）",
        T::TypePrompt => "类型",
        T::PlanVariantPrompt => "套餐变体（限额窗口结构，影响显示哪些用量窗口）",
        T::PlanVariantAuto => "自动（按响应推断）",
        T::PlanVariantNoWeekly => "无周限（v1：仅 5 小时窗）",
        T::PlanVariantWeekly => "有周限（v2+：5 小时 + 周窗）",
        T::TemplateOption => "template —— 自定义 JSON 模板",
        T::BaseUrlPromptAdd => "base_url（模板 {{baseUrl}} 变量来源，可空）",
        T::KeyPromptSkip => "API key（直接回车跳过；输入显示为星号）",
        T::CliCredentialNote => {
            "该平台凭据来自本机已登录的官方 CLI，无需输入 API key（查询时自动读取）"
        }
        T::UseProxyPrompt => "查询走代理？（目标站点被墙时开启，端口在设置中统一配置）",
        T::IdEmptyHint => "id 不能为空（--json 模式需提供非空 id 字段）",
        T::NameEmpty => "名称不能为空",
        T::IdGenFail => "id 生成失败：",
        T::EncryptFail => "凭据加密失败：",
        T::EntryJsonFields => "（entry.json 需含 id、name、kind 字段）",
        T::ApiKeyEncRejected => {
            "entry.json 不应包含 api_key_enc（密文不经手）；请移除该字段，用 quota set-key 配置凭据"
        }
        T::TplValidateFail => "模板校验失败：",
        T::ValidateFail => "静态校验未通过：",
        T::RetrySuffix => "请修正后重新粘贴（Ctrl+C 放弃）",
        T::PasteTemplateJson => "粘贴模板 JSON",
        T::ScriptOption => "script —— 自定义 JS 脚本（QuickJS 沙箱）",
        T::PasteScriptCode => "粘贴脚本 JS 代码",
        T::ScriptValidateFail => "脚本校验失败：",
        T::AllowInsecurePrompt => "脚本是否需要访问非 HTTPS（http）地址？",

        T::PasteHintEdit => {
            "粘贴提示：名称 / base_url 输入框请用 Shift+Ctrl+V 或鼠标右键（Ctrl+V 在此不生效）。"
        }
        T::NamePromptEdit => "名称（仅做标识符，回车保持）",
        T::BaseUrlPromptEdit => "base_url（回车保持，输入 - 清空）",
        T::CurrentTemplateLabel => "当前模板：",
        T::PasteNewTplPrompt => "粘贴新模板 JSON（直接空行 = 保持不变）",
        T::CurrentScriptLabel => "当前脚本：",
        T::PasteNewScriptPrompt => {
            "粘贴新脚本 JS 代码（单独一行 . 结束；保持不变 = 不粘贴直接输入 . ）"
        }
        T::InvalidScriptKeep => "脚本无效（保持原脚本）：",
        T::EnabledConfirm => "启用该条目",
        T::InvalidTplKeep => "模板无效（保持原模板）：",
        T::EmptyInputKeep => "空输入（保持不变）",

        T::SetKeyPrompt => "输入新的 API key（输入显示为星号）",
        T::KeyEmptyRejected => "输入为空，key 未变更（如需删除条目请用 quota remove）",

        T::NeedEntryOrJson => "需要 --entry <id> 或 --json 之一（--help 查看用法）",
        T::StaticCheckOk => "静态校验通过",
        T::TestEntryName => "模板试查",
        T::ScriptTestEntryName => "脚本试查",
        T::TryQueryEncryptFail => "试查凭据加密失败：",
        T::TryQueryFail => "试查失败：",
        T::NeedsKeyPrompt => {
            "该查询引用 {{apiKey}}，输入测试用 key（仅本次不落盘；输入显示为星号）"
        }
        T::KeyEmptyHint => "key 为空；引用 {{apiKey}} 的查询需要 key，请重新运行并输入",
        T::KeyEmptyRedirect => {
            "key 为空；stdin 被输入重定向占用无法交互输入，请改用 --entry 复用已存条目"
        }
        T::LooksLikeJsonHint => {
            "提示：输入以 { 开头但不是合法的脚本配置 JSON，已按纯 JS 代码处理；若为配置请检查字段名（code / allowInsecure）"
        }
        T::LblValid => "有效",
        T::Dash => "——",

        T::VaultStoreOk => "系统凭据库：可读（主密钥已存在）",
        T::VaultStoreNotInit => "系统凭据库：可读（主密钥尚未初始化，本次检查将生成新密钥）",
        T::VaultStoreReadFail => "系统凭据库读取失败：",
        T::VaultStoreHint => {
            "（Windows 请检查凭据管理器可用性；Linux 需要 Secret Service / gnome-keyring）"
        }
        T::VaultHealthy => "保险库：健康（加解密就绪）",
        T::VaultOpenFail => "保险库打开失败：",
        T::VaultOpenFailCtx => "凭据保险库打开失败：",
        T::EngineInitFail => "查询引擎初始化失败：",
        T::ConfigFilePrefix => "配置文件：",
        T::ConfigTransferFail => "配置迁移失败：",

        T::SmokeKeyFileFormat => "key 文件应为 {\"平台id\": \"key\"} 的 JSON 对象：",
        T::SmokeSkipBody => "跳过（key 为空）",
        T::SmokeUnknownWarn => "告警：未知平台 id（计为失败，检查拼写或升级版本）",
        T::SmokeEncryptFail => "加密失败：",
        T::SmokeAllPass => "全部通过",

        T::HelpAbout => "多平台 AI 账户余额监视器的命令行前端",
        T::HelpConfig => "配置文件路径（默认 ~/.quotatray/config.json；不影响 vault 主密钥位置）",
        T::HelpLang => "界面语言（本次运行覆盖 settings.json；缺省跟随 settings.json / 系统）",
        T::HelpList => "列出全部供应商条目及状态",
        T::HelpListJson => "输出 providers 的 JSON（含凭据密文字段 api_key_enc，非明文）",
        T::HelpQuery => "查询全部或指定条目",
        T::HelpQueryIds => "条目 id（缺省 = 全部 enabled 条目）",
        T::HelpQueryJson => "输出 JSON（供脚本消费）",
        T::HelpQueryWatch => "轮询模式，每轮重绘表格，Ctrl+C 退出",
        T::HelpQueryInterval => "轮询间隔（分钟），默认 5（仅在 --watch 下有效）",
        T::HelpAdd => "添加供应商（交互向导，或 --json 从 stdin 读入）",
        T::HelpAddJson => "从 stdin 读 ProviderEntry 的 JSON（api_key_enc 必须缺省/为空）",
        T::HelpEdit => "编辑条目（向导，回车=保持不变；--enable/--disable 走非交互路径）",
        T::HelpEditId => "条目 id",
        T::HelpEditEnable => "启用条目",
        T::HelpEditDisable => "禁用条目",
        T::HelpRemove => "删除条目",
        T::HelpRemoveId => "条目 id",
        T::HelpRemoveYes => "跳过确认提示",
        T::HelpSetKey => "写入/更新 API key（星号掩码输入，不进 shell history）",
        T::HelpSetKeyId => "条目 id",
        T::HelpNatives => "列出预置平台",
        T::HelpTemplate => "模板工具",
        T::HelpTemplateTest => "模板静态校验 + 真实试查一次",
        T::HelpTemplateEntry => "复用已存条目的 key（vault 解密）与 base_url",
        T::HelpTemplateJson => "从 stdin 读模板 JSON（引用 {{apiKey}} 时经 tty 交互输入 key）",
        T::HelpTemplateBaseUrl => "覆盖 baseUrl 变量",
        T::HelpVault => "凭据保险库",
        T::HelpVaultStatus => "主密钥健康检查（系统凭据库可读性）",
        T::HelpConfigTransfer => "完整配置跨机器迁移",
        T::HelpConfigExport => "导出完整配置与凭据到私有迁移包",
        T::HelpConfigExportOutput => "迁移包输出路径",
        T::HelpConfigExportYes => "跳过敏感文件确认",
        T::HelpConfigImport => "从迁移包整体替换当前配置",
        T::HelpConfigImportInput => "迁移包输入路径",
        T::HelpConfigImportYes => "跳过整体替换确认",
        T::HelpUpdate => "检测 GitHub release 新版本，可选下载安装包",
        T::HelpUpdateCheck => "只检测不下载",
        T::HelpUpdateYes => "跳过下载确认",
        T::HelpUpdateOutput => "安装包保存目录（默认当前目录）",
        T::UpdateCheckFail => "检测失败：",
        T::UpdateNoRelease => "仓库暂无发布版本",
        T::UpdateManualUrl => "该版本没有可下载的安装包，请到发布页手动获取：",
        T::UpdateDownloading => "下载中…",
        T::UpdateDownloadFail => "下载失败：",
        T::UpdateSaveFail => "安装包写入失败：",
        T::UpdateRunHint => "下载完成，请手动运行安装包完成更新",
        T::UpdateClientFail => "无法构造 HTTP 客户端",
        T::PeakLabel => "⚡高峰",
        T::OffPeakLabel => "空闲",
        T::ColPriceItem => "项目",
        T::ColPeak => "高峰",
        T::ColOffPeak => "空闲",
        T::ColPricing => "峰谷",
        T::PriceCacheHit => "输入（缓存命中）",
        T::PriceCacheMiss => "输入（缓存未命中）",
        T::PriceOutput => "输出",
        T::PricingNotConfigured => {
            "该条目未配置峰谷定价，且其平台无预置（可用 quota pricing set 自定义）"
        }
        T::PricingLocalTz => "本地时区",
        T::PricingNoWindows => "未设高峰时段（恒按空闲价）",
        T::PricingValidateFail => "峰谷定价校验失败：",
        T::PricingPlanNote => "订阅积分制：价格档不适用，峰谷行表达折扣时段",
        T::ColModel => "模型",
        T::ColSource => "来源",
        T::ColPlanKind => "模式",
        T::ColPlanPayg => "按量",
        T::ColPlanSubscription => "订阅积分",
        T::ColPeakPrice => "峰价（命中/输入/输出）",
        T::ColOffPeakPrice => "闲价（命中/输入/输出）",
        T::PricingModelSourcePreset => "预置",
        T::PricingModelSourceCustom => "自定义",
        T::PricingModelListEmpty => "（该平台无预置与自定义模型）",
        T::HelpPricing => "峰谷定价：查看 / 自定义 / 清除",
        T::HelpPricingShow => "查看条目生效峰谷定价（当前判定 + 价格对照 + 时段）",
        T::HelpPricingShowId => "条目 id",
        T::HelpPricingShowJson => "输出 JSON（供脚本消费）",
        T::HelpPricingSet => "从 stdin 读 PricingConfig JSON 设为自定义（字段级覆盖预置）",
        T::HelpPricingSetId => "条目 id",
        T::HelpPricingClear => "清除自定义峰谷定价（回退预置）",
        T::HelpPricingClearId => "条目 id",
        T::HelpPricingModel => "自定义模型库管理（按平台聚类，条目 pricing.model 可选用）",
        T::HelpPricingModelList => "列出平台预置与自定义模型（价格对照）",
        T::HelpPricingModelListProvider => "native 平台 id（见 quota natives）",
        T::HelpPricingModelListJson => "输出 JSON（供脚本消费）",
        T::HelpPricingModelAdd => "从 stdin 读 CustomModelDef JSON 添加/覆盖（同 id 覆盖 = 更新）",
        T::HelpPricingModelAddProvider => "native 平台 id（见 quota natives）",
        T::HelpPricingModelRemove => "删除自定义模型",
        T::HelpPricingModelRemoveProvider => "native 平台 id（见 quota natives）",
        T::HelpPricingModelRemoveId => "模型 id",
        T::HelpDevSmoke => "真机冒烟（仅 debug 构建，读 .DevApiKey.json 走完整链路）",
        T::HelpDevSmokeKeyFile => "key 文件路径（默认当前目录 .DevApiKey.json）",
    }
}

fn en(key: T) -> &'static str {
    match key {
        T::Err => "error: ",
        T::StdinReadFail => "stdin read failed: ",
        T::InputReadFail => "input read failed: ",
        T::SelectReadFail => "selection read failed: ",
        T::KeyReadFail => "API key read failed: ",
        T::JsonParseFail => "JSON parse failed: ",
        T::StaticCheckFail => "static validation failed: ",

        T::ColName => "Name",
        T::ColPlan => "Plan",
        T::ColUsed => "Used",
        T::ColRemaining => "Remaining",
        T::ColUnit => "Unit",
        T::ColReset => "Resets",
        T::ColStatus => "Status",
        T::ColType => "Type",
        T::ColEnabled => "Enabled",
        T::ColKeySet => "Key set",
        T::OkNoData => "OK (no data)",
        T::InvalidPrefix => "invalid: ",
        T::Transient => "transient",
        T::Deterministic => "deterministic",
        T::Yes => "yes",
        T::No => "no",

        T::NotTerminal => "stderr is not a terminal; for piped scenarios write the key to stdin",
        T::InterruptCtrlC => "input interrupted by Ctrl+C",
        T::InterruptCtrlD => "input interrupted by Ctrl+D",
        T::ClipboardOpenFail => "open failed: ",
        T::ClipboardReadFail => "read failed: ",
        T::MultilineJsonHint => {
            "(paste multi-line JSON and finish with a blank line; or Ctrl+Z / Ctrl+D to end input)"
        }
        T::MultilineCodeHint => {
            "(paste multi-line code; blank lines are kept. Finish with a single line containing only .)"
        }

        T::ListEmpty => "No provider entries yet; add one with quota add.",
        T::QueryNoEntries => {
            "No entries to query; add one with quota add, or use quota query <id> to pick a disabled entry."
        }
        T::NativesEmpty => "(no built-in providers)",

        T::PasteHintA => {
            "Paste tip: use Shift+Ctrl+V or right-click in the name / base_url prompts (Ctrl+V does not work there);"
        }
        T::PasteHintB => {
            "           the API key prompt supports Ctrl+V paste and echoes asterisks."
        }
        T::NamePromptAdd => "Name (display label only, shown in listings)",
        T::TypePrompt => "Type",
        T::PlanVariantPrompt => "Plan variant (which usage windows to show)",
        T::PlanVariantAuto => "Auto (infer from response)",
        T::PlanVariantNoWeekly => "No weekly limit (v1: 5h window only)",
        T::PlanVariantWeekly => "Weekly limit (v2+: 5h + weekly)",
        T::TemplateOption => "template — custom JSON template",
        T::BaseUrlPromptAdd => "base_url (source of the {{baseUrl}} template variable, optional)",
        T::KeyPromptSkip => "API key (press Enter to skip; input is masked)",
        T::CliCredentialNote => {
            "This platform reads credentials from the locally signed-in official CLI; no API key needed"
        }
        T::UseProxyPrompt => {
            "Route queries through a proxy? (enable for blocked sites; port is configured in settings)"
        }
        T::IdEmptyHint => "id must not be empty (--json mode requires a non-empty id field)",
        T::NameEmpty => "name must not be empty",
        T::IdGenFail => "id generation failed: ",
        T::EncryptFail => "credential encryption failed: ",
        T::EntryJsonFields => " (entry.json requires the id, name and kind fields)",
        T::ApiKeyEncRejected => {
            "entry.json must not contain api_key_enc (ciphertext never passes through the CLI); remove the field and configure the credential via quota set-key"
        }
        T::TplValidateFail => "template validation failed: ",
        T::ValidateFail => "static validation failed: ",
        T::RetrySuffix => "fix it and paste again (Ctrl+C to abort)",
        T::PasteTemplateJson => "Paste the template JSON",
        T::ScriptOption => "script — custom JS script (QuickJS sandbox)",
        T::PasteScriptCode => "Paste the script JS code",
        T::ScriptValidateFail => "script validation failed: ",
        T::AllowInsecurePrompt => "Does the script need to access non-HTTPS (http) URLs?",

        T::PasteHintEdit => {
            "Paste tip: use Shift+Ctrl+V or right-click in the name / base_url prompts (Ctrl+V does not work there)."
        }
        T::NamePromptEdit => "Name (display label only; Enter = keep)",
        T::BaseUrlPromptEdit => "base_url (Enter = keep, '-' to clear)",
        T::CurrentTemplateLabel => "Current template:",
        T::PasteNewTplPrompt => "Paste the new template JSON (a blank line = keep unchanged)",
        T::CurrentScriptLabel => "Current script:",
        T::PasteNewScriptPrompt => {
            "Paste the new script JS code (finish with a single line . ; to keep unchanged, enter . right away)"
        }
        T::InvalidScriptKeep => "invalid script (keeping the current one): ",
        T::EnabledConfirm => "Enable this entry",
        T::InvalidTplKeep => "invalid template (keeping the current one): ",
        T::EmptyInputKeep => "empty input (kept unchanged)",

        T::SetKeyPrompt => "Enter the new API key (input is masked)",
        T::KeyEmptyRejected => "empty input, key unchanged (use quota remove to delete the entry)",

        T::NeedEntryOrJson => "either --entry <id> or --json is required (see --help)",
        T::StaticCheckOk => "static validation passed",
        T::TestEntryName => "template test",
        T::ScriptTestEntryName => "script test",
        T::TryQueryEncryptFail => "test credential encryption failed: ",
        T::TryQueryFail => "live query failed: ",
        T::NeedsKeyPrompt => {
            "this query references {{apiKey}}; enter a test key (this run only, not persisted; input is masked)"
        }
        T::KeyEmptyHint => {
            "key is empty; queries referencing {{apiKey}} need one — rerun and enter it"
        }
        T::KeyEmptyRedirect => {
            "key is empty; stdin is redirected so a key cannot be entered — use --entry to reuse a stored entry"
        }
        T::LooksLikeJsonHint => {
            "note: input starts with { but is not a valid script config JSON; treating it as plain JS code — check the field names (code / allowInsecure) if you meant a config"
        }
        T::LblValid => "valid",
        T::Dash => "—",

        T::VaultStoreOk => "system credential store: readable (master key present)",
        T::VaultStoreNotInit => {
            "system credential store: readable (master key not initialized yet; this check will generate one)"
        }
        T::VaultStoreReadFail => "system credential store read failed: ",
        T::VaultStoreHint => {
            "(on Windows check Credential Manager availability; on Linux a Secret Service / gnome-keyring is required)"
        }
        T::VaultHealthy => "vault: healthy (encrypt/decrypt ready)",
        T::VaultOpenFail => "vault open failed: ",
        T::VaultOpenFailCtx => "credential vault open failed: ",
        T::EngineInitFail => "query engine init failed: ",
        T::ConfigFilePrefix => "config file: ",
        T::ConfigTransferFail => "configuration transfer failed: ",

        T::SmokeKeyFileFormat => "key file must be a JSON object of {\"platform-id\": \"key\"}: ",
        T::SmokeSkipBody => "skipped (empty key)",
        T::SmokeUnknownWarn => {
            "warning: unknown platform id (counted as failure; check spelling or upgrade)"
        }
        T::SmokeEncryptFail => "encryption failed: ",
        T::SmokeAllPass => "all passed",

        T::HelpAbout => "Command-line frontend for the multi-platform AI account balance monitor",
        T::HelpConfig => {
            "Config file path (default ~/.quotatray/config.json; does not affect the vault master key location)"
        }
        T::HelpLang => {
            "Interface language for this run (overrides settings.json; default: settings.json / system)"
        }
        T::HelpList => "List all provider entries with status",
        T::HelpListJson => {
            "Output providers as JSON (includes the api_key_enc ciphertext field, not plaintext)"
        }
        T::HelpQuery => "Query all or the given entries",
        T::HelpQueryIds => "Entry ids (default: all enabled entries)",
        T::HelpQueryJson => "Output JSON (for scripts)",
        T::HelpQueryWatch => "Watch mode: redraw the table each round, Ctrl+C to quit",
        T::HelpQueryInterval => "Polling interval in minutes, default 5 (only with --watch)",
        T::HelpAdd => "Add a provider (interactive wizard, or read from stdin with --json)",
        T::HelpAddJson => "Read a ProviderEntry JSON from stdin (api_key_enc must be absent/empty)",
        T::HelpEdit => {
            "Edit an entry (wizard, Enter = keep; --enable/--disable take the non-interactive path)"
        }
        T::HelpEditId => "Entry id",
        T::HelpEditEnable => "Enable the entry",
        T::HelpEditDisable => "Disable the entry",
        T::HelpRemove => "Remove an entry",
        T::HelpRemoveId => "Entry id",
        T::HelpRemoveYes => "Skip the confirmation prompt",
        T::HelpSetKey => "Write/update the API key (masked input, never enters shell history)",
        T::HelpSetKeyId => "Entry id",
        T::HelpNatives => "List built-in providers",
        T::HelpTemplate => "Template tools",
        T::HelpTemplateTest => "Static validation + one live query",
        T::HelpTemplateEntry => "Reuse the stored entry's key (vault decrypt) and base_url",
        T::HelpTemplateJson => {
            "Read the template JSON from stdin (prompts for the key interactively when {{apiKey}} is referenced)"
        }
        T::HelpTemplateBaseUrl => "Override the baseUrl variable",
        T::HelpVault => "Credential vault",
        T::HelpVaultStatus => "Master key health check (system credential store readability)",
        T::HelpConfigTransfer => "Transfer the complete configuration between machines",
        T::HelpConfigExport => {
            "Export the complete configuration and credentials to a private transfer package"
        }
        T::HelpConfigExportOutput => "Transfer package output path",
        T::HelpConfigExportYes => "Skip the sensitive-file confirmation",
        T::HelpConfigImport => "Replace the current configuration from a transfer package",
        T::HelpConfigImportInput => "Transfer package input path",
        T::HelpConfigImportYes => "Skip the full-replacement confirmation",
        T::HelpUpdate => "Check for a new GitHub release, optionally download the installer",
        T::HelpUpdateCheck => "Check only, do not download",
        T::HelpUpdateYes => "Skip the download confirmation",
        T::HelpUpdateOutput => "Directory to save the installer (default: current directory)",
        T::UpdateCheckFail => "check failed: ",
        T::UpdateNoRelease => "No release published yet",
        T::UpdateManualUrl => {
            "No downloadable installer for this version; get it from the release page:"
        }
        T::UpdateDownloading => "Downloading…",
        T::UpdateDownloadFail => "download failed: ",
        T::UpdateSaveFail => "failed to write the installer: ",
        T::UpdateRunHint => "Download complete; run the installer manually to update",
        T::UpdateClientFail => "failed to build an HTTP client",
        T::PeakLabel => "Peak",
        T::OffPeakLabel => "Off-peak",
        T::ColPriceItem => "Item",
        T::ColPeak => "Peak",
        T::ColOffPeak => "Off-peak",
        T::ColPricing => "Pricing",
        T::PriceCacheHit => "Input (cache hit)",
        T::PriceCacheMiss => "Input (cache miss)",
        T::PriceOutput => "Output",
        T::PricingNotConfigured => {
            "no peak pricing configured for this entry (and no preset for its provider); define one with quota pricing set"
        }
        T::PricingLocalTz => "local timezone",
        T::PricingNoWindows => "no peak windows (always billed at off-peak rates)",
        T::PricingValidateFail => "peak pricing validation failed: ",
        T::PricingPlanNote => {
            "Subscription credits: price tiers N/A, peak windows mark discount hours"
        }
        T::ColModel => "Model",
        T::ColSource => "Source",
        T::ColPlanKind => "Plan",
        T::ColPlanPayg => "Pay-as-you-go",
        T::ColPlanSubscription => "Subscription",
        T::ColPeakPrice => "Peak (hit/in/out)",
        T::ColOffPeakPrice => "Off-peak (hit/in/out)",
        T::PricingModelSourcePreset => "preset",
        T::PricingModelSourceCustom => "custom",
        T::PricingModelListEmpty => "(no preset or custom models for this provider)",
        T::HelpPricing => "Peak/off-peak pricing: show / set / clear",
        T::HelpPricingShow => {
            "Show the effective peak pricing (current kind, price table, windows)"
        }
        T::HelpPricingShowId => "Entry id",
        T::HelpPricingShowJson => "Output JSON (for scripts)",
        T::HelpPricingSet => {
            "Read a PricingConfig JSON from stdin as the custom override (field-level fallback to presets)"
        }
        T::HelpPricingSetId => "Entry id",
        T::HelpPricingClear => "Clear the custom peak pricing (fall back to presets)",
        T::HelpPricingClearId => "Entry id",
        T::HelpPricingModel => {
            "Custom model library (per provider; entry pricing.model can select)"
        }
        T::HelpPricingModelList => "List preset and custom models of a provider (price comparison)",
        T::HelpPricingModelListProvider => "native provider id (see quota natives)",
        T::HelpPricingModelListJson => "Output JSON (for scripts)",
        T::HelpPricingModelAdd => {
            "Add/overwrite a custom model from CustomModelDef JSON on stdin (same id = update)"
        }
        T::HelpPricingModelAddProvider => "native provider id (see quota natives)",
        T::HelpPricingModelRemove => "Remove a custom model",
        T::HelpPricingModelRemoveProvider => "native provider id (see quota natives)",
        T::HelpPricingModelRemoveId => "model id",
        T::HelpDevSmoke => {
            "Live smoke test (debug builds only; runs the full pipeline via .DevApiKey.json)"
        }
        T::HelpDevSmokeKeyFile => {
            "Key file path (default: .DevApiKey.json in the current directory)"
        }
    }
}

// ---- 带参文案（标点与语序随语言变化，不能走 &str 键表）---------------------

/// `quota add` 成功提示。
pub fn added(lang: Lang, name: &str, id: &str) -> String {
    match lang {
        Lang::En => format!("added: {name} (id: {id})"),
        _ => format!("已添加：{name}（id: {id}）"),
    }
}

/// 添加后未配 key 的提示。
pub fn key_missing_hint(lang: Lang, id: &str) -> String {
    match lang {
        Lang::En => format!("hint: no API key yet; run quota set-key {id}"),
        _ => format!("提示：尚未配置 API key，运行 quota set-key {id}"),
    }
}

/// id 冲突。
pub fn id_exists(lang: Lang, id: &str) -> String {
    match lang {
        Lang::En => format!("id {id} already exists"),
        _ => format!("id {id} 已存在"),
    }
}

/// 找不到条目（edit/query/remove/setkey/template 共用）。
pub fn entry_not_found(lang: Lang, id: &str) -> String {
    match lang {
        Lang::En => format!("entry {id} not found"),
        _ => format!("找不到条目 {id}"),
    }
}

/// edit 快捷路径的启用/禁用结果。
pub fn state_changed(lang: Lang, enabled: bool, name: &str, id: &str) -> String {
    match lang {
        Lang::En => {
            let state = if enabled { "enabled" } else { "disabled" };
            format!("{state}: {name} ({id})")
        }
        _ => {
            let state = if enabled { "已启用" } else { "已禁用" };
            format!("{state}：{name}（{id}）")
        }
    }
}

/// edit 向导保存结果。
pub fn saved(lang: Lang, name: &str, id: &str) -> String {
    match lang {
        Lang::En => format!("saved: {name} ({id})"),
        _ => format!("已保存：{name}（{id}）"),
    }
}

/// remove 确认提示。
pub fn remove_confirm(lang: Lang, name: &str, id: &str) -> String {
    match lang {
        Lang::En => {
            format!("Remove {name} ({id})? Its credential ciphertext will be removed as well")
        }
        _ => format!("删除 {name}（{id}）？其凭据密文将一并移除"),
    }
}

/// remove 取消。
pub fn cancelled(lang: Lang) -> &'static str {
    match lang {
        Lang::En => "cancelled.",
        _ => "已取消。",
    }
}

/// 配置导出前的高敏感文件确认。
pub fn config_export_confirm(lang: Lang, path: &std::path::Path) -> String {
    match lang {
        Lang::En => format!(
            "Export to {}? The package contains a decryption key and must be protected like plaintext credentials",
            path.display()
        ),
        _ => format!(
            "导出到 {}？迁移包携带解密密钥，必须按明文凭据同等保护",
            path.display()
        ),
    }
}

/// 配置导入前的整体替换确认。
pub fn config_import_confirm(lang: Lang, path: &std::path::Path) -> String {
    match lang {
        Lang::En => format!(
            "Import {}? This replaces all current providers, credentials, pricing, and custom models",
            path.display()
        ),
        _ => format!(
            "导入 {}？这会整体替换当前所有供应商、凭据、定价与自定义模型",
            path.display()
        ),
    }
}

/// 配置导出完成提示。
pub fn config_exported(lang: Lang, path: &std::path::Path) -> String {
    match lang {
        Lang::En => format!("configuration exported to {}", path.display()),
        _ => format!("配置已导出至 {}", path.display()),
    }
}

/// 配置导入完成提示。
pub fn config_imported(lang: Lang, path: &std::path::Path, count: usize) -> String {
    match lang {
        Lang::En => format!(
            "configuration imported from {} ({count} provider(s))",
            path.display()
        ),
        _ => format!("已从 {} 导入配置（{count} 个供应商）", path.display()),
    }
}

/// remove 成功。
pub fn removed(lang: Lang, name: &str, id: &str) -> String {
    match lang {
        Lang::En => format!("removed: {name} ({id})"),
        _ => format!("已删除：{name}（{id}）"),
    }
}

/// set-key 成功。
pub fn key_updated(lang: Lang, name: &str, id: &str) -> String {
    match lang {
        Lang::En => format!("key updated: {name} ({id})"),
        _ => format!("已更新 key：{name}（{id}）"),
    }
}

/// query --watch 底部刷新提示。
pub fn watch_hint(lang: Lang, minutes: u64) -> String {
    match lang {
        Lang::En => format!("(refreshes every {minutes} min, Ctrl+C to quit)"),
        _ => format!("（每 {minutes} 分钟刷新，Ctrl+C 退出）"),
    }
}

/// 剪贴板不可用的降级提示。
pub fn clipboard_fail(lang: Lang, msg: &str) -> String {
    match lang {
        Lang::En => format!(
            "clipboard unavailable ({msg}); paste with Shift+Ctrl+V or right-click instead: "
        ),
        _ => format!("剪贴板不可用（{msg}），请用 Shift+Ctrl+V 或右键粘贴："),
    }
}

/// template test：条目不是 template 类型。
pub fn not_template_entry(lang: Lang, id: &str) -> String {
    match lang {
        Lang::En => format!("entry {id} is not of the template kind"),
        _ => format!("条目 {id} 不是 template 类型"),
    }
}

/// script test：条目不是 script 类型。
pub fn not_script_entry(lang: Lang, id: &str) -> String {
    match lang {
        Lang::En => format!("entry {id} is not of the script kind"),
        _ => format!("条目 {id} 不是 script 类型"),
    }
}

/// devsmoke：key 文件不可读。
pub fn smoke_unreadable(lang: Lang, path: &std::path::Path, e: &std::io::Error) -> String {
    match lang {
        Lang::En => format!(
            "cannot read {} ({e}); run from the repo root or pass --key-file",
            path.display()
        ),
        _ => format!(
            "无法读取 {}（{e}）；在仓库根运行或用 --key-file 指定",
            path.display()
        ),
    }
}

/// devsmoke：失败汇总。
pub fn smoke_total_fail(lang: Lang, n: usize) -> String {
    match lang {
        Lang::En => format!("{n} platform(s) failed/unknown"),
        _ => format!("共 {n} 个平台失败/未知"),
    }
}

// ---- pricing（带参：头部行/来源/时段/下次切换/结果） ------------------------

/// `quota pricing show` 头部行：名称（id）· 峰谷标签 [· 模型] [· 币种/MTokens]。
pub fn pricing_header(
    lang: Lang,
    name: &str,
    id: &str,
    kind_label: &str,
    model_label: Option<&str>,
    unit: Option<&str>,
) -> String {
    let mut parts = vec![kind_label.to_string()];
    parts.extend(model_label.map(str::to_string));
    parts.extend(unit.map(str::to_string));
    match lang {
        Lang::En => format!("{name} ({id}) · {}", parts.join(" · ")),
        _ => format!("{name}（{id}）· {}", parts.join(" · ")),
    }
}

/// 定价来源行：预置（native · 模型 id）或自定义。
pub fn pricing_source(lang: Lang, preset: Option<(&str, &str)>) -> String {
    match (lang, preset) {
        (Lang::En, Some((native, model))) => {
            format!("Pricing source: preset ({native} · {model})")
        }
        (Lang::En, None) => "Pricing source: custom".into(),
        (_, Some((native, model))) => format!("定价来源：预置（{native} · {model}）"),
        (_, None) => "定价来源：自定义".into(),
    }
}

/// 时段行：`高峰时段（UTC+08:00）：周一至周五 09:00–12:00、14:00–18:00`。
pub fn pricing_windows_line(lang: Lang, tz_desc: &str, windows_desc: &str) -> String {
    match lang {
        Lang::En => format!("Peak windows ({tz_desc}): {windows_desc}"),
        _ => format!("高峰时段（{tz_desc}）：{windows_desc}"),
    }
}

/// 下次切换行：`下次切换：08-19 12:00 → 空闲`。
pub fn pricing_next_change(lang: Lang, datetime: &str, kind_label: &str) -> String {
    match lang {
        Lang::En => format!("Next change: {datetime} → {kind_label}"),
        _ => format!("下次切换：{datetime} → {kind_label}"),
    }
}

/// pricing set 成功。
pub fn pricing_saved(lang: Lang, id: &str) -> String {
    match lang {
        Lang::En => format!("peak pricing saved ({id})"),
        _ => format!("峰谷定价已保存（{id}）"),
    }
}

/// pricing clear 成功。
pub fn pricing_cleared(lang: Lang, id: &str) -> String {
    match lang {
        Lang::En => format!("custom peak pricing cleared ({id}; presets apply again)"),
        _ => format!("峰谷定价自定义已清除（{id}，预置重新生效）"),
    }
}

/// pricing model：未知平台 id。
pub fn pricing_model_provider_unknown(lang: Lang, provider: &str) -> String {
    match lang {
        Lang::En => format!("unknown provider: {provider} (run `quota natives` for available ids)"),
        _ => format!("未知平台：{provider}（运行 quota natives 查看可用 id）"),
    }
}

/// pricing model add 成功。
pub fn pricing_model_saved(lang: Lang, provider: &str, id: &str) -> String {
    match lang {
        Lang::En => format!("custom model saved: {provider} / {id}"),
        _ => format!("自定义模型已保存：{provider} / {id}"),
    }
}

/// pricing model remove 成功。
pub fn pricing_model_removed(lang: Lang, provider: &str, id: &str) -> String {
    match lang {
        Lang::En => format!("custom model removed: {provider} / {id}"),
        _ => format!("自定义模型已删除：{provider} / {id}"),
    }
}

/// pricing model remove：模型不存在。
pub fn pricing_model_not_found(lang: Lang, provider: &str, id: &str) -> String {
    match lang {
        Lang::En => format!("custom model not found: {provider} / {id}"),
        _ => format!("自定义模型不存在：{provider} / {id}"),
    }
}

// ---- update（检测/下载/启动提示） ------------------------------------------

/// 已是最新（含当前版本号）。
pub fn update_up_to_date(lang: Lang) -> String {
    use quota_core::VERSION;
    match lang {
        Lang::En => format!("Already up to date ({VERSION})"),
        _ => format!("已是最新版本（{VERSION}）"),
    }
}

/// 发现新版本（远端 + 当前版本号）。
pub fn update_found(lang: Lang, version: &str) -> String {
    use quota_core::VERSION;
    match lang {
        Lang::En => format!("New version {version} found (current {VERSION})"),
        _ => format!("发现新版本 {version}（当前 {VERSION}）"),
    }
}

/// --check 模式下的安装包信息行。
pub fn update_asset_info(lang: Lang, name: &str, size: u64) -> String {
    match lang {
        Lang::En => format!("installer: {name} ({size} bytes)"),
        _ => format!("安装包：{name}（{size} 字节）"),
    }
}

/// 下载确认 prompt。
pub fn update_confirm(lang: Lang, path: &std::path::Path) -> String {
    match lang {
        Lang::En => format!("Download to {}?", path.display()),
        _ => format!("下载到 {}？", path.display()),
    }
}

/// 下载完成路径。
pub fn update_saved(lang: Lang, path: &std::path::Path) -> String {
    match lang {
        Lang::En => format!("Saved to {}", path.display()),
        _ => format!("已保存至 {}", path.display()),
    }
}

/// 启动钩子的 stderr 一行提示（所有 stdout 输出完成后）。
pub fn update_hint_available(lang: Lang, version: &str) -> String {
    match lang {
        Lang::En => format!("New version {version} available; run `quota update` to upgrade"),
        _ => format!("发现新版本 {version}：运行 quota update 更新"),
    }
}

/// 读到代理端口时的一行提示（让用户知道更新流量走了代理）。
pub fn update_proxy_note(lang: Lang, port: u16) -> String {
    match lang {
        Lang::En => format!("Using local proxy 127.0.0.1:{port} for update checks and downloads"),
        _ => format!("检测与下载经本机代理 127.0.0.1:{port}"),
    }
}

// ---- clap help 运行时翻译 --------------------------------------------------

/// 用选定语言覆盖 clap 命令面的 help 文案。
///
/// derive 的 doc comment 是编译期中文默认；本函数对两个语言都做覆盖，
/// 使文案表成为唯一事实源（doc comment 仅保留源码可读性）。
/// `dev-smoke` 子命令仅在 debug 构建存在，release 下 `mut_subcommand`
/// 对不存在的名字是 no-op。
pub fn apply_help_lang(cmd: Command, lang: Lang) -> Command {
    let tr = |k: T| t(lang, k);
    cmd.about(tr(T::HelpAbout))
        .mut_arg("config", |a| a.help(tr(T::HelpConfig)))
        .mut_arg("lang", |a| a.help(tr(T::HelpLang)))
        .mut_subcommand("list", |c| {
            c.about(tr(T::HelpList))
                .mut_arg("json", |a| a.help(tr(T::HelpListJson)))
        })
        .mut_subcommand("query", |c| {
            c.about(tr(T::HelpQuery))
                .mut_arg("ids", |a| a.help(tr(T::HelpQueryIds)))
                .mut_arg("json", |a| a.help(tr(T::HelpQueryJson)))
                .mut_arg("watch", |a| a.help(tr(T::HelpQueryWatch)))
                .mut_arg("interval", |a| a.help(tr(T::HelpQueryInterval)))
        })
        .mut_subcommand("add", |c| {
            c.about(tr(T::HelpAdd))
                .mut_arg("json", |a| a.help(tr(T::HelpAddJson)))
        })
        .mut_subcommand("edit", |c| {
            c.about(tr(T::HelpEdit))
                .mut_arg("id", |a| a.help(tr(T::HelpEditId)))
                .mut_arg("enable", |a| a.help(tr(T::HelpEditEnable)))
                .mut_arg("disable", |a| a.help(tr(T::HelpEditDisable)))
        })
        .mut_subcommand("remove", |c| {
            c.about(tr(T::HelpRemove))
                .mut_arg("id", |a| a.help(tr(T::HelpRemoveId)))
                .mut_arg("yes", |a| a.help(tr(T::HelpRemoveYes)))
        })
        .mut_subcommand("set-key", |c| {
            c.about(tr(T::HelpSetKey))
                .mut_arg("id", |a| a.help(tr(T::HelpSetKeyId)))
        })
        .mut_subcommand("natives", |c| c.about(tr(T::HelpNatives)))
        .mut_subcommand("template", |c| {
            c.about(tr(T::HelpTemplate)).mut_subcommand("test", |c| {
                c.about(tr(T::HelpTemplateTest))
                    .mut_arg("entry", |a| a.help(tr(T::HelpTemplateEntry)))
                    .mut_arg("json", |a| a.help(tr(T::HelpTemplateJson)))
                    .mut_arg("base_url", |a| a.help(tr(T::HelpTemplateBaseUrl)))
            })
        })
        .mut_subcommand("vault", |c| {
            c.about(tr(T::HelpVault))
                .mut_subcommand("status", |c| c.about(tr(T::HelpVaultStatus)))
        })
        .mut_subcommand("config", |c| {
            c.about(tr(T::HelpConfigTransfer))
                .mut_subcommand("export", |c| {
                    c.about(tr(T::HelpConfigExport))
                        .mut_arg("output", |a| a.help(tr(T::HelpConfigExportOutput)))
                        .mut_arg("yes", |a| a.help(tr(T::HelpConfigExportYes)))
                })
                .mut_subcommand("import", |c| {
                    c.about(tr(T::HelpConfigImport))
                        .mut_arg("input", |a| a.help(tr(T::HelpConfigImportInput)))
                        .mut_arg("yes", |a| a.help(tr(T::HelpConfigImportYes)))
                })
        })
        .mut_subcommand("pricing", |c| {
            c.about(tr(T::HelpPricing))
                .mut_subcommand("show", |c| {
                    c.about(tr(T::HelpPricingShow))
                        .mut_arg("id", |a| a.help(tr(T::HelpPricingShowId)))
                        .mut_arg("json", |a| a.help(tr(T::HelpPricingShowJson)))
                })
                .mut_subcommand("set", |c| {
                    c.about(tr(T::HelpPricingSet))
                        .mut_arg("id", |a| a.help(tr(T::HelpPricingSetId)))
                })
                .mut_subcommand("clear", |c| {
                    c.about(tr(T::HelpPricingClear))
                        .mut_arg("id", |a| a.help(tr(T::HelpPricingClearId)))
                })
                .mut_subcommand("model", |c| {
                    c.about(tr(T::HelpPricingModel))
                        .mut_subcommand("list", |c| {
                            c.about(tr(T::HelpPricingModelList))
                                .mut_arg("provider", |a| {
                                    a.help(tr(T::HelpPricingModelListProvider))
                                })
                                .mut_arg("json", |a| a.help(tr(T::HelpPricingModelListJson)))
                        })
                        .mut_subcommand("add", |c| {
                            c.about(tr(T::HelpPricingModelAdd))
                                .mut_arg("provider", |a| a.help(tr(T::HelpPricingModelAddProvider)))
                        })
                        .mut_subcommand("remove", |c| {
                            c.about(tr(T::HelpPricingModelRemove))
                                .mut_arg("provider", |a| {
                                    a.help(tr(T::HelpPricingModelRemoveProvider))
                                })
                                .mut_arg("id", |a| a.help(tr(T::HelpPricingModelRemoveId)))
                        })
                })
        })
        .mut_subcommand("update", |c| {
            c.about(tr(T::HelpUpdate))
                .mut_arg("check", |a| a.help(tr(T::HelpUpdateCheck)))
                .mut_arg("yes", |a| a.help(tr(T::HelpUpdateYes)))
                .mut_arg("output", |a| a.help(tr(T::HelpUpdateOutput)))
        })
        .mut_subcommand("dev-smoke", |c| {
            c.about(tr(T::HelpDevSmoke))
                .mut_arg("key_file", |a| a.help(tr(T::HelpDevSmokeKeyFile)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：Zh 与 En 的代表性文案均非空且互不相同；System 按中文兜底。
    #[test]
    fn two_languages_diverge_and_system_falls_back_to_zh() {
        for key in [
            T::Err,
            T::ColName,
            T::ColKeySet,
            T::OkNoData,
            T::InvalidPrefix,
            T::Transient,
            T::ListEmpty,
            T::QueryNoEntries,
            T::ApiKeyEncRejected,
            T::VaultStoreNotInit,
            T::HelpAbout,
            T::HelpTemplateJson,
            T::Dash,
        ] {
            let zh = t(Lang::Zh, key);
            let en = t(Lang::En, key);
            assert!(!zh.is_empty(), "{key:?} 中文为空");
            assert!(!en.is_empty(), "{key:?} 英文为空");
            assert_ne!(zh, en, "{key:?} 两语言文案相同");
        }
        assert_eq!(t(Lang::System, T::Err), t(Lang::Zh, T::Err));
    }

    /// 契约：带参文案的双语形态（分隔符随语言变化）。
    #[test]
    fn parameterized_texts_render_both_languages() {
        assert_eq!(added(Lang::Zh, "DS", "ab1"), "已添加：DS（id: ab1）");
        assert_eq!(added(Lang::En, "DS", "ab1"), "added: DS (id: ab1)");
        assert_eq!(entry_not_found(Lang::Zh, "x1"), "找不到条目 x1");
        assert_eq!(entry_not_found(Lang::En, "x1"), "entry x1 not found");
        assert_eq!(state_changed(Lang::Zh, true, "n", "x1"), "已启用：n（x1）");
        assert_eq!(
            state_changed(Lang::En, false, "n", "x1"),
            "disabled: n (x1)"
        );
        assert_eq!(
            watch_hint(Lang::En, 5),
            "(refreshes every 5 min, Ctrl+C to quit)"
        );
        assert_eq!(watch_hint(Lang::Zh, 5), "（每 5 分钟刷新，Ctrl+C 退出）");
    }

    /// 契约：clap help 运行时翻译——两语言的 about/参数 help 均被覆盖。
    #[test]
    fn help_renders_in_both_languages() {
        use clap::CommandFactory;

        let zh = apply_help_lang(crate::Cli::command(), Lang::Zh)
            .render_help()
            .to_string();
        assert!(zh.contains("多平台 AI 账户余额监视器的命令行前端"), "{zh}");
        assert!(zh.contains("列出全部供应商条目及状态"), "{zh}");
        assert!(zh.contains("配置文件路径"), "{zh}");

        let en = apply_help_lang(crate::Cli::command(), Lang::En)
            .render_help()
            .to_string();
        assert!(
            en.contains("Command-line frontend for the multi-platform AI account balance monitor"),
            "{en}"
        );
        assert!(en.contains("List all provider entries with status"), "{en}");
        assert!(en.contains("Interface language for this run"), "{en}");
        // 英文 help 不残留中文子命令描述
        assert!(!en.contains("列出全部供应商条目"), "{en}");
        assert!(!en.contains("凭据保险库"), "{en}");
    }

    /// 契约：嵌套子命令（template test / vault status）与全局参数的 help 也被覆盖。
    #[test]
    fn nested_subcommand_help_translated() {
        use clap::CommandFactory;

        let en_cmd = apply_help_lang(crate::Cli::command(), Lang::En);
        let tpl = en_cmd
            .find_subcommand("template")
            .expect("template 子命令存在")
            .find_subcommand("test")
            .expect("template test 存在");
        assert!(
            tpl.get_about()
                .unwrap()
                .to_string()
                .contains("Static validation"),
            "{:?}",
            tpl.get_about()
        );

        let vault = en_cmd.find_subcommand("vault").unwrap();
        assert!(
            vault
                .get_about()
                .unwrap()
                .to_string()
                .contains("Credential vault")
        );

        let json_arg = en_cmd
            .find_subcommand("query")
            .unwrap()
            .get_arguments()
            .find(|a| a.get_id().as_str() == "json")
            .unwrap();
        assert!(
            json_arg
                .get_help()
                .unwrap()
                .to_string()
                .contains("Output JSON")
        );
    }
}
