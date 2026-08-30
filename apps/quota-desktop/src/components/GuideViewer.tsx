// 配置指引渲染器：guideDocs 收集的 md 文档（轻量 Markdown 子集，guideMd 解析）
// → 结构化 UI。以嵌套 DialogShell 呈现（从 EditDialog 的平台块打开，焦点圈/
// Esc/移动端全屏与返回键关闭由 DialogShell 内建）。文档 h1 在弹窗语境下降级为
// h2（弹窗标题已承担一级语境）。外链经 openConsoleUrl 打开（http/https 白名单
// 与控制台直达同口径）；图片走 bundle 资产映射，未命中渲染占位（预留期目录为空）。
import { useMemo, useState } from "react";
import { api } from "../api";
import { useLang } from "../i18n";
import { GUIDE_DOCS, bundleImageSrc } from "./guideDocs";
import { parseGuideMd, type GuideBlock, type GuideInline } from "./guideMd";
import { Button, DialogShell } from "./ui";

export function GuideViewer({
  docKey,
  onClose,
}: {
  docKey: string;
  onClose: () => void;
}) {
  const { t } = useLang();
  // 行内外链打开失败提示（白名单拒绝/系统拉起失败；ProviderCard 同款语义）
  const [linkError, setLinkError] = useState(false);
  const blocks = useMemo(() => parseGuideMd(GUIDE_DOCS[docKey] ?? ""), [docKey]);

  return (
    <DialogShell
      title={t("edit.guideButton")}
      onClose={onClose}
      closeLabel={t("titlebar.close")}
      size="lg"
      className="qt-dialog-guide"
      footer={
        <Button onClick={onClose}>{t("common.close")}</Button>
      }
    >
      {linkError && <p className="qt-inline-error">{t("card.consoleOpenFailed")}</p>}
      <div className="qt-guide-content">
        {blocks.map((block, i) => (
          <GuideBlockView
            key={i}
            block={block}
            onOpenLink={(href) => {
              setLinkError(false);
              api.openConsoleUrl(href).catch(() => setLinkError(true));
            }}
          />
        ))}
      </div>
    </DialogShell>
  );
}

function GuideBlockView({
  block,
  onOpenLink,
}: {
  block: GuideBlock;
  onOpenLink: (href: string) => void;
}) {
  switch (block.kind) {
    case "heading": {
      // 文档内 h1/h2/h3 → 弹窗内 h2/h3/h4
      const Tag = block.level === 1 ? "h2" : block.level === 2 ? "h3" : "h4";
      return (
        <Tag className="qt-guide-h">
          <InlineTokens tokens={block.inline} onOpenLink={onOpenLink} />
        </Tag>
      );
    }
    case "paragraph":
      return (
        <p className="qt-guide-p">
          <InlineTokens tokens={block.inline} onOpenLink={onOpenLink} />
        </p>
      );
    case "list": {
      const items = block.items.map((item, i) => (
        <li key={i}>
          <InlineTokens tokens={item} onOpenLink={onOpenLink} />
        </li>
      ));
      return block.ordered ? (
        <ol className="qt-guide-list">{items}</ol>
      ) : (
        <ul className="qt-guide-list">{items}</ul>
      );
    }
    case "code":
      return (
        <pre className="qt-guide-code">
          <code>{block.text}</code>
        </pre>
      );
    case "quote":
      return (
        <blockquote className="qt-guide-quote">
          <InlineTokens tokens={block.lines} onOpenLink={onOpenLink} />
        </blockquote>
      );
    case "hr":
      return <hr className="qt-guide-hr" />;
  }
}

function InlineTokens({
  tokens,
  onOpenLink,
}: {
  tokens: GuideInline[];
  onOpenLink: (href: string) => void;
}) {
  const { t } = useLang();
  return (
    <>
      {tokens.map((token, i) => {
        switch (token.kind) {
          case "strong":
            return <strong key={i}>{token.text}</strong>;
          case "code":
            return (
              <code key={i} className="qt-guide-code-inline">
                {token.text}
              </code>
            );
          case "link":
            return (
              <button
                type="button"
                key={i}
                className="qt-guide-link"
                onClick={() => onOpenLink(token.href)}
              >
                {token.text}
              </button>
            );
          case "image": {
            const src = bundleImageSrc(token.src);
            return src ? (
              <img
                key={i}
                src={src}
                alt={token.alt}
                className="qt-guide-img"
                loading="lazy"
              />
            ) : (
              <span key={i} className="qt-guide-img-missing">
                {t("edit.guideImageMissing", { alt: token.alt })}
              </span>
            );
          }
          default:
            return <span key={i}>{token.text}</span>;
        }
      })}
    </>
  );
}
