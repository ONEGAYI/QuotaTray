//! GUI Rust 侧 i18n：语言解析 + 中英文案表。
//!
//! 覆盖范围：托盘菜单/条目行/相对时间，以及 IPC 命令的错误串。
//! 与前端 `src/i18n/zh.ts` / `en.ts` 是平行双实现（成对约定），
//! 两端文案语义保持一致——修改任一侧须同步另一侧。
//!
//! 组织方式：静态文案集中在 [`Texts`]（每语言一个 const 实例），
//! 带参数的文案（相对时间、拼接错误等）作为 [`Lang`] 的方法。

/// 界面语言（`system` 在解析时折叠为具体语言）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// 静态文案表（无参数部分）。
pub struct Texts {
    /// 托盘：暂无启用的供应商
    pub no_enabled_providers: &'static str,
    /// 托盘：立即刷新
    pub refresh_now: &'static str,
    /// 托盘：打开主窗口
    pub open_main: &'static str,
    /// 托盘：退出
    pub quit: &'static str,
    /// 托盘：尚无数据
    pub no_data: &'static str,
    /// 托盘：配置读取失败提示
    pub config_error: &'static str,
    /// is_valid=false 且平台未给原因时的兜底
    pub no_invalid_reason: &'static str,
    /// 托盘子菜单标题：图标显示
    pub icon_source: &'static str,
    /// 图标显示子菜单的「自动」项
    pub icon_source_auto: &'static str,
    /// 瞬时失败且无新鲜旧值时的行尾文案（⟳ 前缀由调用方加）
    pub network_fluctuation: &'static str,
    /// keep-last-good 行尾附加（⟳ 前缀由调用方加）
    pub unreachable: &'static str,
    /// 已失效行前缀（⚠ 前缀由调用方加），后接冒号与原因
    pub invalid_prefix: &'static str,
    /// 数据既无百分比也无余额时的兜底行
    pub fetched: &'static str,
}
const ZH: Texts = Texts {
    no_enabled_providers: "暂无启用的供应商",
    refresh_now: "立即刷新",
    open_main: "打开主窗口",
    quit: "退出",
    no_data: "尚无数据",
    config_error: "配置读取失败，请检查 config.json",
    no_invalid_reason: "未说明原因",
    icon_source: "图标显示",
    icon_source_auto: "自动（第一个启用条目）",
    network_fluctuation: "网络波动",
    unreachable: "暂不可达",
    invalid_prefix: "已失效：",
    fetched: "已获取",
};

const EN: Texts = Texts {
    no_enabled_providers: "No enabled providers",
    refresh_now: "Refresh now",
    open_main: "Open main window",
    quit: "Quit",
    no_data: "No data yet",
    config_error: "Failed to read config.json",
    no_invalid_reason: "No reason given",
    icon_source: "Icon shows",
    icon_source_auto: "Auto (first enabled entry)",
    network_fluctuation: "Network issue",
    unreachable: "Unreachable",
    invalid_prefix: "Invalid: ",
    fetched: "Fetched",
};

impl Lang {
    /// 解析 settings.language：`zh`/`en` 直取；`system` 或未知值按系统 locale
    /// 检测（zh 前缀 → Zh，其余含检测失败 → En）。
    pub fn parse(s: &str) -> Self {
        match s {
            "zh" => Self::Zh,
            "en" => Self::En,
            _ => Self::from_system(),
        }
    }

    /// 系统语言检测：locale 以 zh 开头 → 中文；否则（含检测失败）→ 英文。
    pub fn from_system() -> Self {
        match sys_locale::get_locale().as_deref() {
            Some(l) if l.to_ascii_lowercase().starts_with("zh") => Self::Zh,
            _ => Self::En,
        }
    }

    pub fn texts(&self) -> &'static Texts {
        match self {
            Self::Zh => &ZH,
            Self::En => &EN,
        }
    }

    /// 相对时间文案：`刚刚` / `N 秒前` / `N 分钟前` / `N 小时前` / `N 天前`。
    /// 与前端 `src/display.ts` 的 `relativeTime` 语义成对（分档边界一致）。
    pub fn relative_time(&self, secs: u64) -> String {
        match secs {
            0..=9 => match self {
                Self::Zh => "刚刚".into(),
                Self::En => "just now".into(),
            },
            10..=59 => match self {
                Self::Zh => format!("{secs} 秒前"),
                Self::En => format!("{secs}s ago"),
            },
            60..=3599 => match self {
                Self::Zh => format!("{} 分钟前", secs / 60),
                Self::En => format!("{}m ago", secs / 60),
            },
            3600..=86_399 => match self {
                Self::Zh => format!("{} 小时前", secs / 3600),
                Self::En => format!("{}h ago", secs / 3600),
            },
            _ => match self {
                Self::Zh => format!("{} 天前", secs / 86_400),
                Self::En => format!("{}d ago", secs / 86_400),
            },
        }
    }

    /// 多窗口行缺省窗口名：`窗口2` / `Window 2`。
    pub fn window_name(&self, n: usize) -> String {
        match self {
            Self::Zh => format!("窗口{n}"),
            Self::En => format!("Window {n}"),
        }
    }

    /// 剩余余额文案：`剩余 62.97 CNY` / `Left 62.97 CNY`。
    pub fn remaining_text(&self, amount: &str, unit: Option<&str>) -> String {
        let core = match unit {
            Some(u) => format!("{amount} {u}"),
            None => amount.to_string(),
        };
        match self {
            Self::Zh => format!("剩余 {core}"),
            Self::En => format!("Left {core}"),
        }
    }

    /// 已用百分比文案：`已用 42%` / `Used 42%`。
    pub fn used_text(&self, percent: &str) -> String {
        match self {
            Self::Zh => format!("已用 {percent}"),
            Self::En => format!("Used {percent}"),
        }
    }

    // ---- IPC 命令错误串（带参数） ----------------------------------------

    pub fn err_id_name_empty(&self) -> String {
        match self {
            Self::Zh => "id 与名称不能为空".into(),
            Self::En => "id and name must not be empty".into(),
        }
    }

    pub fn err_unknown_native(&self, provider: &str) -> String {
        match self {
            Self::Zh => format!("未知的预置平台 id：{provider}"),
            Self::En => format!("Unknown native provider id: {provider}"),
        }
    }

    pub fn err_entry_not_found(&self, id: &str) -> String {
        match self {
            Self::Zh => format!("条目 {id} 不存在"),
            Self::En => format!("Entry {id} does not exist"),
        }
    }

    pub fn err_entry_not_enabled(&self, id: &str) -> String {
        match self {
            Self::Zh => format!("条目 {id} 不存在或未启用"),
            Self::En => format!("Entry {id} does not exist or is disabled"),
        }
    }

    pub fn err_mobile_cli_credentials(&self) -> String {
        match self {
            Self::Zh => "该平台依赖桌面官方 CLI 的本机登录文件，Android 端无法读取".into(),
            Self::En => {
                "This provider depends on a desktop CLI login file that Android cannot read".into()
            }
        }
    }

    pub fn err_reorder_mismatch(&self) -> String {
        match self {
            Self::Zh => "排序列表与现有条目不一致，请刷新后重试".into(),
            Self::En => "Order list does not match existing entries; refresh and retry".into(),
        }
    }

    pub fn err_encrypt_failed(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!("凭据加密失败：{e}"),
            Self::En => format!("Credential encryption failed: {e}"),
        }
    }

    pub fn err_template_json(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!("模板/脚本 JSON 解析失败：{e}"),
            Self::En => format!("Failed to parse template/script JSON: {e}"),
        }
    }

    pub fn err_template_needs_key(&self) -> String {
        match self {
            Self::Zh => "该查询引用了 {{apiKey}}，试查前请填写 API key".into(),
            Self::En => "This query references {{apiKey}}; fill in the API key first".into(),
        }
    }

    pub fn err_template_needs_key2(&self) -> String {
        match self {
            Self::Zh => "该查询引用了 {{apiKey2}}，试查前请填写第二凭据".into(),
            Self::En => {
                "This query references {{apiKey2}}; fill in the second credential first".into()
            }
        }
    }

    pub fn err_settings_save(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!("设置写入失败：{e}"),
            Self::En => format!("Failed to write settings: {e}"),
        }
    }

    pub fn err_autostart_apply(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!(
                "其余设置已保存，但开机自启未能应用：{e}（请重试保存，或重新切换一次自启开关）"
            ),
            Self::En => format!(
                "Other settings saved, but autostart could not be applied: {e} \
                 (retry saving, or toggle autostart once more)"
            ),
        }
    }

    /// 便携形态拒绝开启自启动（后端硬门禁文案）。
    pub fn err_autostart_portable(&self) -> String {
        match self {
            Self::Zh => "便携版不支持开机自启：启动项会指向可移除介质，拔盘后残留无效注册表项"
                .to_string(),
            Self::En => {
                "Portable builds do not support autostart: the launch entry would point to removable media and leave a dead registry entry after unplug"
                    .to_string()
            }
        }
    }

    /// zip 分发形态拒绝运行安装包（更新走手动覆盖引导）。
    pub fn err_update_install_portable(&self) -> String {
        match self {
            Self::Zh => "当前构建使用 zip 更新包：下载完成后退出应用，将 zip 内容解压覆盖到程序目录"
                .to_string(),
            Self::En => {
                "This build updates via zip: quit the app, then extract the downloaded zip over the application directory"
                    .to_string()
            }
        }
    }

    pub fn err_autostart_toggle(&self, enable: bool, e: &dyn std::fmt::Display) -> String {
        match (self, enable) {
            (Self::Zh, true) => format!("开启开机自启失败：{e}"),
            (Self::Zh, false) => format!("关闭开机自启失败：{e}"),
            (Self::En, true) => format!("Failed to enable autostart: {e}"),
            (Self::En, false) => format!("Failed to disable autostart: {e}"),
        }
    }

    // ---- 峰谷定价（托盘信息行 + IPC 错误） ---------------------------------

    /// 托盘峰谷行 1：类型 + 模型标签（`⚡ 高峰 · V4 Flash`）。
    /// 入参保持 i18n 层纯净（不引 core 类型）：is_peak + 已格式化标签。
    pub fn peak_status_line(&self, is_peak: bool, model_label: Option<&str>) -> String {
        let kind = match (self, is_peak) {
            (Self::Zh, true) => "⚡ 高峰",
            (Self::Zh, false) => "空闲",
            (Self::En, true) => "⚡ Peak",
            (Self::En, false) => "Off-peak",
        };
        match model_label {
            Some(m) => format!("{kind} · {m}"),
            None => kind.into(),
        }
    }

    pub fn subscription_pricing_line(&self) -> &'static str {
        match self {
            Self::Zh => "订阅积分制",
            Self::En => "Subscription credits",
        }
    }

    /// 托盘峰谷行 2：当前档三价 `命中 0.1 · 未命中 3 · 输出 9 CNY/Mtok`。
    /// 缺价字段由调用方过滤后传 None；全 None 由调用方决定不显示本行。
    pub fn peak_prices_line(
        &self,
        hit: Option<&str>,
        miss: Option<&str>,
        out: Option<&str>,
        currency: Option<&str>,
    ) -> String {
        let label = |zh: &str, en: &str, v: &str| match self {
            Self::Zh => format!("{zh} {v}"),
            Self::En => format!("{en} {v}"),
        };
        let mut parts = Vec::new();
        if let Some(v) = hit {
            parts.push(label("命中", "Hit", v));
        }
        if let Some(v) = miss {
            parts.push(label("未命中", "Miss", v));
        }
        if let Some(v) = out {
            parts.push(label("输出", "Out", v));
        }
        let mut line = parts.join(" · ");
        if let Some(c) = currency {
            line.push_str(&format!(" {c}/Mtok"));
        }
        line
    }

    /// upsert 的峰谷配置校验错误。
    pub fn err_pricing_invalid(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!("峰谷定价配置无效：{e}"),
            Self::En => format!("Invalid peak pricing: {e}"),
        }
    }

    // ---- 更新检测（M4-b） ----------------------------------------------------

    /// 托盘菜单「新版本可用」信息行（disabled 项，⟳ 前缀）。
    pub fn update_available(&self, version: &str) -> String {
        match self {
            Self::Zh => format!("⟳ 新版本 v{version} 可用（设置 · 更新）"),
            Self::En => format!("⟳ New version v{version} available (Settings · Update)"),
        }
    }

    /// 「更新就绪」系统通知标题（主窗不可见、自动下载完成后发送）。
    pub fn update_ready_title(&self) -> String {
        match self {
            Self::Zh => "QuotaTray 更新就绪".into(),
            Self::En => "QuotaTray update ready".into(),
        }
    }

    /// 「更新就绪」系统通知正文：引导打开主窗（通知点击唤主窗在部分
    /// 平台不可用，正文自带引导路径兜底）。
    pub fn update_ready_body(&self, version: &str) -> String {
        match self {
            Self::Zh => format!("新版本 v{version} 已下载完成，点击托盘图标打开主窗安装"),
            Self::En => format!(
                "New version v{version} downloaded. Click the tray icon to open QuotaTray and install"
            ),
        }
    }

    pub fn err_update_client(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!("HTTP 客户端初始化失败：{e}"),
            Self::En => format!("Failed to build an HTTP client: {e}"),
        }
    }

    pub fn err_update_not_checked(&self) -> String {
        match self {
            Self::Zh => "尚未检测更新（先点「立即检查」）".into(),
            Self::En => "No update check yet (click \"Check now\" first)".into(),
        }
    }

    pub fn err_update_no_asset(&self) -> String {
        match self {
            Self::Zh => "该版本没有可下载的安装包，请到发布页获取".into(),
            Self::En => "No downloadable installer for this version; see the release page".into(),
        }
    }

    pub fn err_update_download(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!("下载失败：{e}"),
            Self::En => format!("Download failed: {e}"),
        }
    }

    pub fn err_update_mkdir(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!("下载目录创建失败：{e}"),
            Self::En => format!("Failed to create the download directory: {e}"),
        }
    }

    pub fn err_update_bad_asset(&self) -> String {
        match self {
            Self::Zh => "安装包资产名异常（含路径成分或非 exe），已拒绝保存".into(),
            Self::En => {
                "Suspicious installer asset name (path parts or non-exe); refused to save".into()
            }
        }
    }

    pub fn err_update_unsafe_dir(&self) -> String {
        match self {
            Self::Zh => "下载目录异常（可能被替换为链接指向他处），已中止".into(),
            Self::En => "The download directory appears to be a link to elsewhere; aborted".into(),
        }
    }

    pub fn err_update_save(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!("安装包写入失败：{e}"),
            Self::En => format!("Failed to write the installer: {e}"),
        }
    }

    pub fn err_update_not_downloaded(&self) -> String {
        match self {
            Self::Zh => "尚未下载安装包（先点「下载安装包」）".into(),
            Self::En => "Installer not downloaded yet (click \"Download installer\" first)".into(),
        }
    }

    pub fn err_update_installer_missing(&self) -> String {
        match self {
            Self::Zh => "安装包文件已丢失（临时目录可能被清理），请重新下载".into(),
            Self::En => {
                "The installer file is missing (the temp directory may have been cleaned); \
please download again"
                    .into()
            }
        }
    }

    pub fn err_update_run(&self, e: &dyn std::fmt::Display) -> String {
        match self {
            Self::Zh => format!("安装包启动失败：{e}"),
            Self::En => format!("Failed to launch the installer: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：显式 zh/en 直取；system/未知值回系统检测（返回两个合法变体之一）。
    #[test]
    fn parse_known_and_system_fallback() {
        assert_eq!(Lang::parse("zh"), Lang::Zh);
        assert_eq!(Lang::parse("en"), Lang::En);
        assert!(matches!(Lang::parse("system"), Lang::Zh | Lang::En));
        assert!(matches!(Lang::parse("fr"), Lang::Zh | Lang::En));
    }

    /// 契约：更新就绪通知双语（标题 + 带版本正文）。
    #[test]
    fn update_ready_notification_both_langs() {
        assert_eq!(Lang::Zh.update_ready_title(), "QuotaTray 更新就绪");
        assert_eq!(Lang::En.update_ready_title(), "QuotaTray update ready");
        assert_eq!(
            Lang::Zh.update_ready_body("0.8.0"),
            "新版本 v0.8.0 已下载完成，点击托盘图标打开主窗安装"
        );
        assert_eq!(
            Lang::En.update_ready_body("0.8.0"),
            "New version v0.8.0 downloaded. Click the tray icon to open QuotaTray and install"
        );
    }

    /// 契约：相对时间分档双语（边界与前端 display.ts 成对：0-9 刚刚、
    /// 10-59 秒、60-3599 分、3600-86399 时、其余天）。
    #[test]
    fn relative_time_buckets_both_langs() {
        let cases = [
            (5u64, "刚刚", "just now"),
            (30, "30 秒前", "30s ago"),
            (59, "59 秒前", "59s ago"),
            (60, "1 分钟前", "1m ago"),
            (180, "3 分钟前", "3m ago"),
            (3599, "59 分钟前", "59m ago"),
            (3600, "1 小时前", "1h ago"),
            (7200, "2 小时前", "2h ago"),
            (86_399, "23 小时前", "23h ago"),
            (86_400, "1 天前", "1d ago"),
            (172_800, "2 天前", "2d ago"),
        ];
        for (secs, zh, en) in cases {
            assert_eq!(Lang::Zh.relative_time(secs), zh, "zh @{secs}");
            assert_eq!(Lang::En.relative_time(secs), en, "en @{secs}");
        }
    }

    /// 契约：行内格式化双语（剩余/已用/窗口名）。
    #[test]
    fn line_formats_both_langs() {
        assert_eq!(
            Lang::Zh.remaining_text("62.97", Some("CNY")),
            "剩余 62.97 CNY"
        );
        assert_eq!(
            Lang::En.remaining_text("62.97", Some("CNY")),
            "Left 62.97 CNY"
        );
        assert_eq!(Lang::Zh.remaining_text("5.00", None), "剩余 5.00");
        assert_eq!(Lang::Zh.used_text("42%"), "已用 42%");
        assert_eq!(Lang::En.used_text("42%"), "Used 42%");
        assert_eq!(Lang::Zh.window_name(2), "窗口2");
        assert_eq!(Lang::En.window_name(2), "Window 2");
    }

    /// 契约：峰谷行双语（类型/标签/三价与单位；缺价字段跳过）。
    #[test]
    fn peak_lines_both_langs() {
        assert_eq!(
            Lang::Zh.peak_status_line(true, Some("V4 Pro")),
            "⚡ 高峰 · V4 Pro"
        );
        assert_eq!(
            Lang::En.peak_status_line(false, Some("V4 Pro")),
            "Off-peak · V4 Pro"
        );
        assert_eq!(Lang::Zh.peak_status_line(false, None), "空闲");
        assert_eq!(Lang::En.peak_status_line(true, None), "⚡ Peak");
        assert_eq!(Lang::Zh.subscription_pricing_line(), "订阅积分制");
        assert_eq!(Lang::En.subscription_pricing_line(), "Subscription credits");
        assert_eq!(
            Lang::Zh.peak_prices_line(Some("0.3"), Some("9"), Some("27"), Some("CNY")),
            "命中 0.3 · 未命中 9 · 输出 27 CNY/Mtok"
        );
        assert_eq!(
            Lang::En.peak_prices_line(Some("0.3"), Some("9"), Some("27"), Some("CNY")),
            "Hit 0.3 · Miss 9 · Out 27 CNY/Mtok"
        );
        // 缺价字段跳过、无币种不加后缀
        assert_eq!(
            Lang::Zh.peak_prices_line(None, Some("9"), None, None),
            "未命中 9"
        );
    }

    /// 契约：中英文案表均非空且互不相等（防一侧漏配后回落到另一语言）。
    #[test]
    fn texts_nonempty_and_distinct() {
        let zh = Lang::Zh.texts();
        let en = Lang::En.texts();
        let fields: [(&'static str, &'static str, &'static str); 13] = [
            (
                "no_enabled_providers",
                zh.no_enabled_providers,
                en.no_enabled_providers,
            ),
            ("refresh_now", zh.refresh_now, en.refresh_now),
            ("open_main", zh.open_main, en.open_main),
            ("quit", zh.quit, en.quit),
            ("no_data", zh.no_data, en.no_data),
            ("config_error", zh.config_error, en.config_error),
            (
                "no_invalid_reason",
                zh.no_invalid_reason,
                en.no_invalid_reason,
            ),
            ("icon_source", zh.icon_source, en.icon_source),
            ("icon_source_auto", zh.icon_source_auto, en.icon_source_auto),
            (
                "network_fluctuation",
                zh.network_fluctuation,
                en.network_fluctuation,
            ),
            ("unreachable", zh.unreachable, en.unreachable),
            ("invalid_prefix", zh.invalid_prefix, en.invalid_prefix),
            ("fetched", zh.fetched, en.fetched),
        ];
        for (name, z, e) in fields {
            assert!(!z.is_empty(), "{name} zh 不应为空");
            assert!(!e.is_empty(), "{name} en 不应为空");
            assert_ne!(z, e, "{name} 双语不应相同");
        }
    }
}
