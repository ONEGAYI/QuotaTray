/** 条目头像的取值解析与展示决策（纯逻辑，无 React 依赖）。
 *
 * 头像取值三形态（契约见 core `avatar` 模块与 CONTEXT.md「条目头像」词条）：
 * - 缺省：跟随平台品牌图（native 官方 logo；模板/脚本按名称启发）
 * - `brand:<slug>`：手动指定内置品牌图
 * - `data:image/...`：用户上传图（已由后端规整为 ≤256 KiB）
 *
 * 解析顺序：显式覆盖 → 平台默认 → 首字母回退。任何一层解析失败都静默降到
 * 下一层——坏值不能让卡片显示破图，也不能阻断整份配置渲染。
 *
 * 展示形态（cover 铺满 / contain 留边 / 首字母）由 `resolveAvatar` 一次判定，
 * 消费点直接取 `className`，不各自重复判别 kind——三处渲染点（卡片、悬停
 * 列表、详情抽屉）曾因此漏掉悬停面板的 `is-custom`。 */

import { BRAND_SLUGS, brandIconUrl, isLightLogo, providerIconUrl, templateProviderIconUrl } from "./providerIcon";

/** 头像的渲染来源判别结果。 */
export type AvatarSource =
  /** 用户上传图或显式选中的品牌图（cover 铺满） */
  | { kind: "custom"; url: string; className: string }
  /** 跟随平台品牌图（contain 留边，按品牌图规范） */
  | { kind: "platform"; url: string; className: string }
  /** 无图可渲染：首字母文本回退 */
  | { kind: "initials"; className: "" };

/** `brand:` 前缀（与 core `avatar::BRAND_PREFIX` 成对）。 */
const BRAND_PREFIX = "brand:";
/** data URL 前缀（与 core `avatar::DATA_PREFIX` 成对）。 */
const DATA_PREFIX = "data:image/";

/** 取头像值的形态：`brand` / `data` / 未设置。 */
export function avatarValueKind(value: string | undefined | null): "brand" | "data" | "none" {
  const trimmed = (value ?? "").trim();
  if (!trimmed) return "none";
  if (trimmed.startsWith(BRAND_PREFIX)) return "brand";
  if (trimmed.startsWith(DATA_PREFIX)) return "data";
  return "none";
}

/** 取品牌键（`brand:<slug>` 的 slug）；非品牌形态返回 null。 */
export function avatarBrandKey(value: string | undefined | null): string | null {
  const trimmed = (value ?? "").trim();
  if (!trimmed.startsWith(BRAND_PREFIX)) return null;
  const slug = trimmed.slice(BRAND_PREFIX.length);
  return slug.length > 0 ? slug : null;
}

/** 内置可选头像清单（模态窗挑选面板的数据源）。
 *
 * 顺序即展示顺序：先放最常被自定义的平台。 */
export function builtinAvatarChoices(): { slug: string; url: string }[] {
  return BRAND_SLUGS.map((slug) => ({ slug, url: brandIconUrl(slug) })).filter(
    (choice): choice is { slug: string; url: string } => choice.url !== null,
  );
}

/** 拼内层容器的展示类：填充形态与浅底变体。
 *
 * 三处渲染点共用这一份判定，避免「卡片改了、悬停面板忘了」的分叉。
 *
 * 填充分三档而非两档：
 * - `is-upload`：用户照片，cover 铺满（像相册头像）
 * - `is-brand`：手动选中的内置品牌图，仍 contain 留边——品牌 SVG 的
 *   viewBox 差异极大（openrouter 401×294、novita 24×15），cover 会裁得
 *   认不出，且挑图格里是 contain，选中后突变很突兀
 * - 无前缀：跟随平台品牌图，同 contain
 */
function innerClassName(shape: "upload" | "brand" | "platform", light: boolean): string {
  const shapeClass = shape === "upload" ? " is-upload" : shape === "brand" ? " is-brand" : "";
  const tone = light ? " is-light-logo" : "";
  return `${shapeClass}${tone}`;
}

/** 解析条目的头像渲染来源。
 *
 * `nativeId` 为 null 表示模板/脚本条目（无平台品牌 id）。
 */
export function resolveAvatar(options: {
  avatar?: string | null;
  nativeId: string | null;
  entryName: string;
}): AvatarSource {
  const { avatar, nativeId, entryName } = options;
  const kind = avatarValueKind(avatar);

  // 显式覆盖优先：坏值静默降级到平台默认（不显示破图）
  if (kind === "brand") {
    const slug = avatarBrandKey(avatar);
    const url = slug ? brandIconUrl(slug) : null;
    if (url) {
      return {
        kind: "custom",
        url,
        className: innerClassName("brand", isLightLogo(slug ?? "")),
      };
    }
  }
  if (kind === "data") {
    // data URL 由后端校验过 MIME/体积；这里只做形态判别，不再重复校验
    return {
      kind: "custom",
      url: (avatar ?? "").trim(),
      className: innerClassName("upload", false),
    };
  }

  // 平台默认
  const platformUrl = nativeId ? providerIconUrl(nativeId) : templateProviderIconUrl(entryName);
  if (platformUrl) {
    const light = nativeId ? isLightLogo(nativeId) : false;
    return { kind: "platform", url: platformUrl, className: innerClassName("platform", light) };
  }
  return { kind: "initials", className: "" };
}

/** 模态窗中「恢复默认」是否可用：当前无覆盖时置灰（避免用户点了个没反应
 *  的按钮）。 */
export function canResetAvatar(avatar: string | undefined | null): boolean {
  return avatarValueKind(avatar) !== "none";
}
