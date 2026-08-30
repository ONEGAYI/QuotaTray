// 配置指引文档与打包图片资产的唯一收集点（vite import 管线）。
//
// 进包方式（预研 §六定案）：md 经 `?raw` eager glob 编译期内联为字符串；
// 图片源放 docs/assets/bundle/（目录暂无图片资产，所有者 2026-08-30 定），
// 经 `?url` glob 引用后由 vite 复制进 dist/assets 哈希命名——dev/build/
// 桌面/Android/便携版天然一致，文档内引用名与打包名经本模块映射解耦。
//
// 注意：docs/guide/ 目录下所有 .md 都会被收集进应用包（UI 可见性另由
// GUIDE_FOR_PROVIDER 显式映射控制）；草稿/说明文件勿放该目录。
//
// 文档内图片引用路径约定：相对仓库 docs/ 目录，如 `assets/bundle/foo.png`；
// 查表未命中时渲染占位（guideImageSrc 返回 null）。

import type { GuideInline } from "./guideMd";

const docModules = import.meta.glob("../../../../docs/guide/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const imageModules = import.meta.glob(
  "../../../../docs/assets/bundle/*.{png,jpg,jpeg,svg,webp}",
  { query: "?url", import: "default", eager: true },
) as Record<string, string>;

/** 指引文档表：文件名（含扩展名）→ 原文。 */
export const GUIDE_DOCS: Readonly<Record<string, string>> = Object.fromEntries(
  Object.entries(docModules).map(([path, content]) => [fileNameOf(path), content]),
);

/** 打包图片资产表：文件名 → vite 产物 URL。 */
export const BUNDLE_ASSETS: Readonly<Record<string, string>> = Object.fromEntries(
  Object.entries(imageModules).map(([path, url]) => [fileNameOf(path), url]),
);

/** native 平台 id → 指引文档名；新增平台指引只需在此登记。 */
export const GUIDE_FOR_PROVIDER: Readonly<Record<string, string>> = {
  aliyun_bss: "阿里云余额监控配置指引.md",
};

/** 文档内图片引用 → 打包 URL；未命中返回 null（组件渲染占位）。
 *  接受两种引用形态：完整相对路径（assets/bundle/foo.png）与裸文件名。 */
export function bundleImageSrc(src: string): string | null {
  const name = fileNameOf(src);
  return BUNDLE_ASSETS[name] ?? null;
}

function fileNameOf(path: string): string {
  const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return idx === -1 ? path : path.slice(idx + 1);
}

/** 从行内 token 流中取第一个图片 token（GuideViewer 便捷读取）。 */
export function firstImageOf(tokens: GuideInline[]): GuideInline | null {
  return tokens.find((t) => t.kind === "image") ?? null;
}
