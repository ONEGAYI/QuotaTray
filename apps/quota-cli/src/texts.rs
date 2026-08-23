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
    BaseUrlPromptAdd,
    KeyPromptSkip,
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

    // ---- edit ----
    PasteHintEdit,
    NamePromptEdit,
    BaseUrlPromptEdit,
    CurrentTemplateLabel,
    PasteNewTplPrompt,
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
    TryQueryEncryptFail,
    TryQueryFail,
    NeedsKeyPrompt,
    KeyEmptyHint,
    /// 试查输出的「有效」标签（是/否复用 Yes/No）。
    LblValid,

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
        T::TemplateOption => "template —— 自定义 JSON 模板",
        T::BaseUrlPromptAdd => "base_url（模板 {{baseUrl}} 变量来源，可空）",
        T::KeyPromptSkip => "API key（直接回车跳过；输入显示为星号）",
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

        T::PasteHintEdit => {
            "粘贴提示：名称 / base_url 输入框请用 Shift+Ctrl+V 或鼠标右键（Ctrl+V 在此不生效）。"
        }
        T::NamePromptEdit => "名称（仅做标识符，回车保持）",
        T::BaseUrlPromptEdit => "base_url（回车保持，输入 - 清空）",
        T::CurrentTemplateLabel => "当前模板：",
        T::PasteNewTplPrompt => "粘贴新模板 JSON（直接空行 = 保持不变）",
        T::EnabledConfirm => "启用该条目",
        T::InvalidTplKeep => "模板无效（保持原模板）：",
        T::EmptyInputKeep => "空输入（保持不变）",

        T::SetKeyPrompt => "输入新的 API key（输入显示为星号）",
        T::KeyEmptyRejected => "输入为空，key 未变更（如需删除条目请用 quota remove）",

        T::NeedEntryOrJson => "需要 --entry <id> 或 --json 之一（quota template test --help 查看）",
        T::StaticCheckOk => "静态校验通过",
        T::TestEntryName => "模板试查",
        T::TryQueryEncryptFail => "试查凭据加密失败：",
        T::TryQueryFail => "试查失败：",
        T::NeedsKeyPrompt => {
            "该模板引用 {{apiKey}}，输入测试用 key（仅本次不落盘；输入显示为星号）"
        }
        T::KeyEmptyHint => "key 为空；无 key 调试请改用 quota template test --entry",
        T::LblValid => "有效",

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
        T::TemplateOption => "template — custom JSON template",
        T::BaseUrlPromptAdd => "base_url (source of the {{baseUrl}} template variable, optional)",
        T::KeyPromptSkip => "API key (press Enter to skip; input is masked)",
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

        T::PasteHintEdit => {
            "Paste tip: use Shift+Ctrl+V or right-click in the name / base_url prompts (Ctrl+V does not work there)."
        }
        T::NamePromptEdit => "Name (display label only; Enter = keep)",
        T::BaseUrlPromptEdit => "base_url (Enter = keep, '-' to clear)",
        T::CurrentTemplateLabel => "Current template:",
        T::PasteNewTplPrompt => "Paste the new template JSON (a blank line = keep unchanged)",
        T::EnabledConfirm => "Enable this entry",
        T::InvalidTplKeep => "invalid template (keeping the current one): ",
        T::EmptyInputKeep => "empty input (kept unchanged)",

        T::SetKeyPrompt => "Enter the new API key (input is masked)",
        T::KeyEmptyRejected => "empty input, key unchanged (use quota remove to delete the entry)",

        T::NeedEntryOrJson => {
            "either --entry <id> or --json is required (see quota template test --help)"
        }
        T::StaticCheckOk => "static validation passed",
        T::TestEntryName => "template test",
        T::TryQueryEncryptFail => "test credential encryption failed: ",
        T::TryQueryFail => "live query failed: ",
        T::NeedsKeyPrompt => {
            "this template references {{apiKey}}; enter a test key (this run only, not persisted; input is masked)"
        }
        T::KeyEmptyHint => "key is empty; for keyless debugging use quota template test --entry",
        T::LblValid => "valid",

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
