import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const css = await readFile(new URL("../src/index.css", import.meta.url), "utf8");

test("Android 宽视口仍以显式详情按钮控制卡片展开", () => {
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-details-toggle\s*\{[^}]*display:\s*inline-flex;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-provider-card:not\(\.is-expanded\):hover \.qt-provider-secondary[^}]*max-height:\s*0;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-provider-card\.is-expanded \.qt-provider-secondary[^}]*max-height:\s*1400px;/s,
  );
});

test("Android 通用文字按钮与图标按钮具有按压反馈", () => {
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-btn:not\(:disabled\):active,[\s\S]*body\.qt-mobile-runtime \.qt-icon-btn:not\(:disabled\):active\s*\{[^}]*opacity:/,
  );
});

test("Android 控制台直达为 trailing 文字按钮且满足 44px 命中区（T-010）", () => {
  // 文字按钮：视觉小（13px 字 + 16px 图标）、命中区 ≥44px、默认无实心底
  assert.match(
    css,
    /\.qt-console-text-btn\s*\{[^}]*min-height:\s*44px;[^}]*margin-left:\s*auto;/s,
  );
  assert.match(
    css,
    /\.qt-console-text-btn:active\s*\{[^}]*background:/s,
  );
  // 所在的 route 行转 flex 使按钮 trailing 靠右，label 保持省略号
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-provider-route\s*\{[^}]*display:\s*flex;[^}]*align-items:\s*center;/s,
  );
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-provider-route-label[^{]*\{[^}]*flex:\s*1;[^}]*min-width:\s*0;/s,
  );
});

test("Android 更新页主行动按钮满足 44px 命中区（T-010）", () => {
  // 检测/下载/安装是更新页唯一主行动（对话框 footer 外），2026-08-29
  // 审查修复补齐；与 dialog-footer 的 44px 同口径
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-update-status \.qt-btn\s*\{[^}]*min-height:\s*44px;/s,
  );
});
