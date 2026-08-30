// 配置指引文档与打包图片资产的唯一收集点（vite import 管线）。
//
// 进包方式（预研 §六定案）：md 经 `?raw` eager glob 编译期内联为字符串；
// 图片源放 docs/assets/bundle/（目录暂无图片资产，所有者 2026-08-30 定），
// 经 `?url` glob 引用后由 vite 复制进 dist/assets 哈希命名——dev/build/
// 桌面/Android/便携版天然一致，文档内引用名与打包名经本模块映射解耦。
//
// 语言组织（2026-08-30 所有者定）：指引按 docs/guide/{zh,en}/ 语言子目录
// 存放，两语言目录内**同名文件**（英文命名，语义对齐）；UI 侧按当前语言
// 经 GUIDE_FOR_PROVIDER 选档，请求语言文件缺失时回退另一语言
// （resolveGuideDoc，按收录表存在性判定，不依赖手写登记）。
//
// 注意：语言子目录下所有 .md 都会被收集进应用包（UI 可见性另由
// GUIDE_FOR_PROVIDER 显式映射控制）；草稿/说明文件勿放这些目录。
//
// 文档内图片引用路径约定：相对仓库 docs/ 目录，如 `assets/bundle/foo.png`；
// 查表未命中时渲染占位（guideImageSrc 返回 null）。

import type { GuideInline } from "./guideMd";
import type { UiLang } from "../i18n";

const zhDocModules = import.meta.glob("../../../../docs/guide/zh/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const enDocModules = import.meta.glob("../../../../docs/guide/en/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const imageModules = import.meta.glob(
  "../../../../docs/assets/bundle/*.{png,jpg,jpeg,svg,webp}",
  { query: "?url", import: "default", eager: true },
) as Record<string, string>;

/** 指引文档表：语言 →（文件名含扩展名 → 原文）。 */
export const GUIDE_DOCS: Readonly<Record<UiLang, Readonly<Record<string, string>>>> = {
  zh: toMap(zhDocModules),
  en: toMap(enDocModules),
};

/** 打包图片资产表：文件名 → vite 产物 URL。 */
export const BUNDLE_ASSETS: Readonly<Record<string, string>> = toMap(imageModules);

/** native 平台 id → 指引文档名（两语言目录内同名文件）；新增平台指引只需在此登记。 */
export const GUIDE_FOR_PROVIDER: Readonly<Record<string, string>> = {
  aliyun_bss: "aliyun-balance-setup-guide.md",
};

/** 按平台与 UI 语言解析指引文档；请求语言的文件未收录时回退另一语言，
 *  均未收录或平台无映射返回 null。收录表可注入（纯函数，测试单语回退用）。 */
export function resolveGuideDoc(
  providerId: string,
  lang: UiLang,
  docs: Readonly<Record<UiLang, Readonly<Record<string, string>>>> = GUIDE_DOCS,
): { lang: UiLang; key: string } | null {
  const key = GUIDE_FOR_PROVIDER[providerId];
  if (!key) return null;
  if (docs[lang][key]) return { lang, key };
  const other: UiLang = lang === "zh" ? "en" : "zh";
  return docs[other][key] ? { lang: other, key } : null;
}

/** 文档内图片引用 → 打包 URL；未命中返回 null（组件渲染占位）。
 *  接受两种引用形态：完整相对路径（assets/bundle/foo.png）与裸文件名。 */
export function bundleImageSrc(src: string): string | null {
  const name = fileNameOf(src);
  return BUNDLE_ASSETS[name] ?? null;
}

function toMap(modules: Record<string, string>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(modules).map(([path, content]) => [fileNameOf(path), content]),
  );
}

function fileNameOf(path: string): string {
  const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return idx === -1 ? path : path.slice(idx + 1);
}

/** 从行内 token 流中取第一个图片 token（GuideViewer 便捷读取）。 */
export function firstImageOf(tokens: GuideInline[]): GuideInline | null {
  return tokens.find((t) => t.kind === "image") ?? null;
}
