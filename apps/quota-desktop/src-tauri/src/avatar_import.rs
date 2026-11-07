//! 自定义头像的上传规整：读用户选的文件 → 校验/缩放/编码 → 落成
//! `brand:` 或 data URL 字符串，交给 `set_provider_avatar` 落盘。
//!
//! 分工约定（见 CONTEXT.md「条目头像」词条）：
//! - **core**（`quota_core::avatar`）只做纯字符串级校验：前缀、体积上限、
//!   base64 合法性、SVG 内容白名单——CLI 也能校验配置文件，不依赖图片库
//! - **desktop**（本模块）负责重量级规整：解码、等比缩放、重编码 PNG。
//!   用户传几 MB 的 PNG 截图也直接可用，不需要自己先压缩（受理格式限
//!   PNG 与 SVG；JPEG/WebP 按不支持拒绝）
//!
//! 所有函数都是纯函数（入参字节 + 选项，出参字符串），便于契约测试；
//! 只有 [`read_avatar_file`] 触磁盘/SAF。

use quota_core::avatar::{self, AvatarMime};

/// 规整后的最长边上限（px）。
///
/// 42×42 的卡片容器在 2x/3x 高分屏下最多吃到 ~126px；256px 留足余量，
/// 同时把典型照片压到 20~160 KiB，让 20 个条目仍远低于导出包 16 MiB 上限。
pub const AVATAR_MAX_EDGE: u32 = 256;

/// 单次读取用户文件的硬上限（原始字节）。
///
/// 只用于拦住「误选了 2 GB 视频」这类输入；真正的体积约束是规整后的
/// data URL 长度（core 的 `avatar::AVATAR_MAX_LEN`，256 KiB）。
pub const AVATAR_MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AvatarImportError {
    #[error("头像文件过大：{0} 字节，上限 {AVATAR_MAX_SOURCE_BYTES} 字节")]
    SourceTooLarge(usize),
    #[error("无法识别的图片格式（仅支持 PNG 与 SVG）")]
    UnknownFormat,
    #[error("图片解码失败：{0}")]
    Decode(String),
    #[error("图片编码失败：{0}")]
    Encode(String),
    #[error("图片内容不合规：{0}")]
    Invalid(String),
}

impl From<avatar::AvatarError> for AvatarImportError {
    fn from(e: avatar::AvatarError) -> Self {
        AvatarImportError::Invalid(e.to_string())
    }
}

/// 用户选中文件的内容判别结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Png,
    Svg,
}

/// 按字节魔数判别格式，不看扩展名（用户常把 SVG 存成 .txt、或把 PNG
/// 改名成 .svg）。
pub fn detect_format(bytes: &[u8]) -> Option<SourceFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(SourceFormat::Png);
    }
    // SVG 是文本：跳过 BOM 与前置空白/XML 声明后找 <svg
    let head = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(head);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if text.starts_with("<?xml") || text.starts_with("<svg") || text.starts_with("<!DOCTYPE svg") {
        return Some(SourceFormat::Svg);
    }
    None
}

/// 把用户选中的文件字节规整成可落盘的头像取值。
///
/// - PNG：解码 → 长边超过 [`AVATAR_MAX_EDGE`] 时等比缩小 → 重编码 PNG
/// - SVG：只做内容白名单校验后原样内嵌（保持矢量清晰度）
///
/// 产出的字符串已通过 `quota_core::avatar::validate`，调用方可直接落盘。
pub fn normalize(bytes: &[u8]) -> Result<String, AvatarImportError> {
    if bytes.len() > AVATAR_MAX_SOURCE_BYTES {
        return Err(AvatarImportError::SourceTooLarge(bytes.len()));
    }
    let format = detect_format(bytes).ok_or(AvatarImportError::UnknownFormat)?;
    let value = match format {
        SourceFormat::Svg => normalize_svg(bytes)?,
        SourceFormat::Png => normalize_png(bytes)?,
    };
    // 兜底：规整产出必须过 core 校验（含 256 KiB 上限），否则不落盘
    avatar::validate(&value)?;
    Ok(value)
}

/// SVG：白名单校验后原样转 base64。
///
/// 不缩小也不重编码：SVG 的体积取决于节点数而非尺寸，缩放没有收益，重编码
/// 反而要引入渲染依赖并丢失矢量清晰度。
fn normalize_svg(bytes: &[u8]) -> Result<String, AvatarImportError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AvatarImportError::Invalid("SVG 不是合法 UTF-8".into()))?;
    avatar::validate_svg(text)?;
    Ok(avatar::to_data_url(AvatarMime::Svg, bytes))
}

/// PNG：解码、必要时等比缩小、重编码。
///
/// 已经足够小时仍重编码一次：用户上传的 PNG 常带大块元数据（EXIF/ICC），
/// 重编码能顺手丢掉，让落盘字节可预期。
fn normalize_png(bytes: &[u8]) -> Result<String, AvatarImportError> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|e| AvatarImportError::Decode(e.to_string()))?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(AvatarImportError::Invalid("图片尺寸为 0".into()));
    }
    let scaled = if w.max(h) > AVATAR_MAX_EDGE {
        // 等比缩放到长边 = AVATAR_MAX_EDGE，用 Lanczos3 保细节
        img.resize(
            AVATAR_MAX_EDGE,
            AVATAR_MAX_EDGE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };
    let mut out = Vec::new();
    scaled
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| AvatarImportError::Encode(e.to_string()))?;
    Ok(avatar::to_data_url(AvatarMime::Png, &out))
}

/// 结果为 `true` 表示「跟随平台品牌图」（清除自定义头像）。
///
/// 空串/纯空白是前端的「恢复默认」语义，不是错误。
pub fn is_clear_request(value: &str) -> bool {
    value.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成一张纯色 PNG 用于测试（避免引外部 fixture）。
    fn make_png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([12, 34, 56, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// 契约：按魔数判别格式，不看扩展名。
    #[test]
    fn detects_format_from_magic_bytes() {
        assert_eq!(detect_format(&make_png(2, 2)), Some(SourceFormat::Png));
        assert_eq!(
            detect_format(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#),
            Some(SourceFormat::Svg)
        );
        assert_eq!(
            detect_format(b"\xef\xbb\xbf  <?xml version=\"1.0\"?><svg/>"),
            Some(SourceFormat::Svg)
        );
        assert_eq!(detect_format(b"not an image at all"), None);
        assert_eq!(detect_format(b""), None);
    }

    /// 契约：超大 PNG 等比缩到长边 256，且产出能过 core 校验。
    #[test]
    fn downscales_large_png_to_max_edge() {
        let big = make_png(1024, 512);
        let value = normalize(&big).unwrap();
        avatar::validate(&value).unwrap();
        let decoded = decode_data_url(&value);
        let img = image::load_from_memory(&decoded).unwrap();
        assert_eq!(img.width().max(img.height()), AVATAR_MAX_EDGE);
        // 等比：1024×512 → 256×128
        assert_eq!((img.width(), img.height()), (256, 128));
    }

    /// 契约：小图不放大（避免糊图占体积）。
    #[test]
    fn keeps_small_png_size() {
        let value = normalize(&make_png(48, 48)).unwrap();
        let img = image::load_from_memory(&decode_data_url(&value)).unwrap();
        assert_eq!((img.width(), img.height()), (48, 48));
    }

    /// 契约：SVG 原样内嵌（矢量清晰度不因上传损失），并过白名单。
    #[test]
    fn embeds_svg_as_is() {
        let svg =
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle r="5"/></svg>"#;
        let value = normalize(svg.as_bytes()).unwrap();
        assert_eq!(value, avatar::to_data_url(AvatarMime::Svg, svg.as_bytes()));
        avatar::validate(&value).unwrap();
    }

    /// 契约：带脚本的 SVG 在规整阶段就被拦，不落成 data URL。
    #[test]
    fn rejects_scripted_svg() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
        let err = normalize(svg.as_bytes()).unwrap_err();
        assert!(
            matches!(err, AvatarImportError::Invalid(_)),
            "实际：{err:?}"
        );
    }

    /// 契约：无法识别的输入给出确定性错误，不 panic。
    #[test]
    fn rejects_unknown_format() {
        let err = normalize(b"GIF89a....").unwrap_err();
        assert_eq!(err, AvatarImportError::UnknownFormat);
    }

    /// 契约：源文件超过硬上限时先拒，不进解码器。
    #[test]
    fn rejects_oversized_source() {
        let mut bytes = vec![0u8; AVATAR_MAX_SOURCE_BYTES + 1];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        let err = normalize(&bytes).unwrap_err();
        assert_eq!(err, AvatarImportError::SourceTooLarge(bytes.len()));
    }

    /// 契约：空白输入是「恢复默认」的清除语义，不是错误。
    #[test]
    fn blank_means_clear() {
        assert!(is_clear_request(""));
        assert!(is_clear_request("   "));
        assert!(!is_clear_request("brand:openai"));
    }

    /// 测试助手：取回 data URL 的原始字节。
    fn decode_data_url(value: &str) -> Vec<u8> {
        let (_, payload) = value.split_once(',').unwrap();
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap()
    }

    /// 契约：现有内置品牌 SVG 全部通过白名单（防止校验误伤自身资产）。
    ///
    /// 放在 desktop 侧：资产在前端目录，core 不该反向依赖应用的资源。
    /// 目录缺失时**失败**而非跳过——静默放行等于这条契约不存在。
    #[test]
    fn brand_assets_pass_svg_whitelist() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/assets/providers");
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("品牌资产目录不可读 {}: {e}", dir.display()));
        let mut checked = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("svg") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            quota_core::avatar::validate_svg(&text)
                .unwrap_or_else(|e| panic!("内置品牌 SVG 未通过白名单 {}: {e}", path.display()));
            checked += 1;
        }
        assert_eq!(checked, 16, "应收敛到全部 16 个品牌 SVG，实际 {checked}");
    }
}
