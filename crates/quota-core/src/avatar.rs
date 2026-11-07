//! 条目头像（`ProviderEntry::avatar`）的取值契约与纯函数校验。
//!
//! 头像有三种取值形态，统一用带前缀的字符串表达，便于旧配置兼容与
//! 未来增量扩展（新增形态只需加前缀，不需改结构体）：
//!
//! - 缺省（字段不落盘）：跟随平台品牌图（native 的官方 logo；模板/脚本
//!   走名称启发或首字母回退）
//! - `brand:<slug>`：手动指定内置品牌图（slug 与前端资产表对应，如
//!   `brand:deepseek`）
//! - `data:image/png;base64,...` / `data:image/svg+xml;base64,...`：
//!   用户上传图，规整后内嵌进配置
//!
//! 预留形态 `lib:<id>`（引用未来「我的头像库」）不在本模块受理：当前版本
//! 遇到即按非法值拒绝，避免半实现状态被写入配置。
//!
//! 校验只在这里做一次（命令层调用），落盘与导出无需重复校验：配置够用
//! 即合法，读到非法值按「跟随平台」回退，不阻断整份配置加载。

use base64::Engine as _;

/// 头像字符串（含 data URL 前缀）的长度上限。
///
/// 直接约束配置与导出包体积：256 KiB 下单张规整后的 PNG 通常 20~160 KiB，
/// 20 条目最坏情形仍远低于 `MAX_EXPORT_SIZE`（16 MiB）。
pub(crate) const AVATAR_MAX_LEN: usize = 256 * 1024;

/// 内置品牌图引用前缀，后接前端资产表的 slug。
pub const BRAND_PREFIX: &str = "brand:";

/// 用户上传图的 data URL 前缀（base64 编码，MIME 由校验时的载荷解析判定）。
pub const DATA_PREFIX: &str = "data:image/";

/// 受理的图片 MIME（仅这两类；其余格式在上传规整阶段转成 PNG）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarMime {
    Png,
    Svg,
}

impl AvatarMime {
    /// data URL 中的 MIME 片段。
    pub fn as_str(self) -> &'static str {
        match self {
            AvatarMime::Png => "image/png",
            AvatarMime::Svg => "image/svg+xml",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AvatarError {
    #[error("头像内容为空")]
    Empty,
    #[error("头像图片过大：{len} 字节，上限 {max} 字节")]
    TooLong { len: usize, max: usize },
    #[error("头像取值缺少可识别的前缀（应为 brand: 或 data:image/）")]
    UnknownScheme,
    #[error("内置品牌键非法：{0}（只允许小写字母、数字、下划线与连字符）")]
    InvalidBrandKey(String),
    #[error("头像 data URL 格式非法：{0}")]
    InvalidDataUrl(String),
    #[error("不支持的图片类型：{0}（仅支持 PNG 与 SVG）")]
    UnsupportedMime(String),
    #[error("头像 base64 载荷无法解码")]
    InvalidBase64,
    #[error("SVG 含禁止内容（{0}），请换用导出为 PNG 的版本")]
    ForbiddenSvg(&'static str),
}

/// 校验头像取值，合法则通过。
///
/// 不做形态分流：调用方（`set_provider_avatar`、上传规整）只关心「能不能
/// 落盘」；形态判别在前端 `avatarView` 做，core 不复制一份。
///
/// 空串按「未设置」处理：调用方应把空值折成 `None`（清除头像），
/// 因此空串走到这里视为非法，避免空字符串落盘。
pub fn validate(value: &str) -> Result<(), AvatarError> {
    if value.trim().is_empty() {
        return Err(AvatarError::Empty);
    }
    if value.len() > AVATAR_MAX_LEN {
        return Err(AvatarError::TooLong {
            len: value.len(),
            max: AVATAR_MAX_LEN,
        });
    }
    if let Some(slug) = value.strip_prefix(BRAND_PREFIX) {
        return validate_brand_key(slug);
    }
    match value.strip_prefix(DATA_PREFIX) {
        Some(rest) => parse_data_url(rest).map(|_| ()),
        None => Err(AvatarError::UnknownScheme),
    }
}

/// 品牌键的形状校验：只允许 `[a-z0-9_-]+`。
///
/// 不做「必须是已登记品牌」的存在性校验——品牌资产表归前端，若 core 另存
/// 一份名单，新增品牌就要双侧同步否则版本错配。未知臂牌由前端回退到
/// 「跟随平台」，读者不会看到坏图。
fn validate_brand_key(slug: &str) -> Result<(), AvatarError> {
    if slug.is_empty()
        || !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    {
        return Err(AvatarError::InvalidBrandKey(slug.to_string()));
    }
    Ok(())
}

/// 解析 `data:image/` 之后的载荷：`<mime>;base64,<payload>`。
fn parse_data_url(rest: &str) -> Result<AvatarMime, AvatarError> {
    let (head, payload) = rest
        .split_once(',')
        .ok_or_else(|| AvatarError::InvalidDataUrl("缺少逗号分隔".into()))?;
    let (mime_part, encoding) = head
        .split_once(';')
        .ok_or_else(|| AvatarError::InvalidDataUrl("缺少 ;base64 声明".into()))?;
    if !encoding.eq_ignore_ascii_case("base64") {
        return Err(AvatarError::InvalidDataUrl(format!(
            "不支持的编码方式 {encoding}"
        )));
    }
    let mime = match mime_part.to_ascii_lowercase().as_str() {
        "png" => AvatarMime::Png,
        "svg+xml" => AvatarMime::Svg,
        other => return Err(AvatarError::UnsupportedMime(other.to_string())),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| AvatarError::InvalidBase64)?;
    if bytes.is_empty() {
        return Err(AvatarError::InvalidDataUrl("载荷为空".into()));
    }
    if mime == AvatarMime::Svg {
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| AvatarError::InvalidDataUrl("SVG 不是合法 UTF-8".into()))?;
        validate_svg(text)?;
    }
    Ok(mime)
}

/// 渲染进 `<img>` 的 SVG 内容白名单校验。
///
/// 浏览器的 `<img>` 不会执行 SVG 内脚本，CSP 也只放行 `self` 与 `data:`；
/// 这层校验是纵深防御——挡住「用户从网上下载了带脚本/外链的 SVG 就顺手
/// 传进来」这一档，代价是几个子串扫描。正常导出工具（Figma/Illustrator）
/// 的产物不受影响：现有 12 个内置品牌 SVG 全部通过。
pub fn validate_svg(svg: &str) -> Result<(), AvatarError> {
    // 小写化后再扫：XML 大小写敏感，但大写变体不是合法脚本元素；宁可多拒
    let lower = svg.to_ascii_lowercase();
    if !lower.contains("<svg") {
        return Err(AvatarError::InvalidDataUrl("未找到 <svg> 根元素".into()));
    }
    if lower.contains("<script") {
        return Err(AvatarError::ForbiddenSvg("<script> 标签"));
    }
    if lower.contains("<foreignobject") {
        return Err(AvatarError::ForbiddenSvg("<foreignObject> 标签"));
    }
    if lower.contains("<iframe") {
        return Err(AvatarError::ForbiddenSvg("<iframe> 标签"));
    }
    if lower.contains("<embed") {
        return Err(AvatarError::ForbiddenSvg("<embed> 标签"));
    }
    if lower.contains("javascript:") {
        return Err(AvatarError::ForbiddenSvg("javascript: 伪协议"));
    }
    if has_event_handler_attr(&lower) {
        return Err(AvatarError::ForbiddenSvg("on* 事件属性"));
    }
    if has_external_href(&lower) {
        return Err(AvatarError::ForbiddenSvg("外链 href"));
    }
    Ok(())
}

/// 扫描 `on<name>=` 形式的事件属性（要求前面是空白，避免误伤属性值文本）。
fn has_event_handler_attr(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i].is_ascii_whitespace() && bytes[i + 1] == b'o' && bytes[i + 2] == b'n' {
            let mut j = i + 3;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric()) {
                j += 1;
            }
            // 至少一个字符的属性名，且紧跟等号（允许等号前空白）
            if j > i + 3 {
                let mut k = j;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b'=' {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// 扫描指向外部资源的 `href`（`http:`、`https:`、协议相对 `//`）。
fn has_external_href(lower: &str) -> bool {
    let mut rest = lower;
    while let Some(pos) = rest.find("href") {
        rest = &rest[pos + 4..];
        let after = rest.trim_start();
        let after = match after.strip_prefix('=') {
            Some(v) => v.trim_start(),
            // xlink:href / href 后可能有属性值写法差异，保守跳过该次
            None => continue,
        };
        let value = after.trim_start_matches(['"', '\'']);
        if value.starts_with("http:") || value.starts_with("https:") || value.starts_with("//") {
            return true;
        }
    }
    false
}

/// 组装 PNG/SVG 的 data URL（上传规整的产出侧，供 desktop 与测试共用）。
pub fn to_data_url(mime: AvatarMime, bytes: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!(
        "{DATA_PREFIX}{};base64,{encoded}",
        mime.as_str().trim_start_matches("image/")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_data_url(payload: &[u8]) -> String {
        to_data_url(AvatarMime::Png, payload)
    }

    fn svg_data_url(svg: &str) -> String {
        to_data_url(AvatarMime::Svg, svg.as_bytes())
    }

    /// 契约：品牌键与 data URL 两种形态都能通过。
    #[test]
    fn accepts_brand_and_data_forms() {
        assert_eq!(validate("brand:deepseek"), Ok(()));
        assert_eq!(validate(&png_data_url(b"\x89PNG fake")), Ok(()));
        assert_eq!(
            validate(&svg_data_url(
                r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#
            )),
            Ok(())
        );
    }

    /// 契约：空前缀、未知前缀、坏品牌键一律拒绝（不留半实现形态落盘）。
    #[test]
    fn rejects_malformed_forms() {
        assert_eq!(validate(""), Err(AvatarError::Empty));
        assert_eq!(validate("   "), Err(AvatarError::Empty));
        assert_eq!(validate("lib:abc"), Err(AvatarError::UnknownScheme));
        assert_eq!(
            validate("https://example.com/a.png"),
            Err(AvatarError::UnknownScheme)
        );
        assert_eq!(
            validate("brand:DeepSeek"),
            Err(AvatarError::InvalidBrandKey("DeepSeek".into()))
        );
        assert_eq!(
            validate("brand:"),
            Err(AvatarError::InvalidBrandKey(String::new()))
        );
    }

    /// 契约：体积上限按落盘的字符串长度计，超出即拒（保住导出包余量）。
    #[test]
    fn rejects_oversized_value() {
        let oversized = format!("brand:{}", "a".repeat(AVATAR_MAX_LEN));
        assert!(matches!(
            validate(&oversized),
            Err(AvatarError::TooLong { .. })
        ));
    }

    /// 契约：base64 坏载荷与不支持的 MIME 分别给出确定性错误。
    #[test]
    fn rejects_bad_payload_and_mime() {
        assert_eq!(
            validate("data:image/png;base64,!!!not-base64!!!"),
            Err(AvatarError::InvalidBase64)
        );
        assert_eq!(
            validate("data:image/gif;base64,R0lGODlh"),
            Err(AvatarError::UnsupportedMime("gif".into()))
        );
        assert_eq!(
            validate("data:image/png,abc"),
            Err(AvatarError::InvalidDataUrl("缺少 ;base64 声明".into()))
        );
    }

    /// 契约：SVG 危险内容被拒；正常导出 SVG 不受影响。
    #[test]
    fn svg_whitelist_blocks_dangerous_content() {
        let cases = [
            (r#"<svg><script>alert(1)</script></svg>"#, "<script> 标签"),
            (r#"<svg><foreignObject/></svg>"#, "<foreignObject> 标签"),
            (r#"<svg onload="alert(1)"/>"#, "on* 事件属性"),
            (
                r#"<svg><a href="https://evil.example/x"/></svg>"#,
                "外链 href",
            ),
            (
                r#"<svg><a href="javascript:alert(1)"/></svg>"#,
                "javascript: 伪协议",
            ),
        ];
        for (svg, reason) in cases {
            assert_eq!(
                validate(&svg_data_url(svg)),
                Err(AvatarError::ForbiddenSvg(reason)),
                "应拒绝：{svg}"
            );
        }
    }

    /// 契约：正常导出风格的真实 SVG 片段能通过白名单（防止校验误伤）。
    ///
    /// 内置品牌资产的整体校验归 desktop（资产在前端目录，core 不反向
    /// 依赖应用资产）；这里只锁「白名单不误伤合法结构」这一层。
    #[test]
    fn realistic_svg_passes_whitelist() {
        let samples = [
            // 单色路径（多数品牌 logo 的形态）
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 3h18v18H3z" fill="#242422"/></svg>"##,
            // 渐变 + defs + 内联样式
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 50 50"><defs><linearGradient id="g"><stop offset="0" stop-color="#fff"/></linearGradient></defs><rect width="50" height="50" fill="url(#g)"/></svg>"##,
            // Figma 导出常见：XML 声明 + DOCTYPE + 注释
            r#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd"><svg xmlns="http://www.w3.org/2000/svg"><!-- logo --><circle cx="5" cy="5" r="4"/></svg>"#,
            // 片段内引用（不是外链）：应放行
            r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><path id="p" d="M0 0h10v10z"/></defs><use href="#p"/></svg>"##,
        ];
        for svg in samples {
            validate_svg(svg).unwrap_or_else(|e| panic!("合法 SVG 被误拒：{e}\n{svg}"));
        }
    }
}
