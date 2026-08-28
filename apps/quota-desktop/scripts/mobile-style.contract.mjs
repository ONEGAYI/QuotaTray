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

test("Android 控制台直达图标钮满足 44px 触摸热区（T-010）", () => {
  assert.match(
    css,
    /body\.qt-mobile-runtime \.qt-icon-btn\.qt-console-btn\s*\{[^}]*width:\s*44px;[^}]*height:\s*44px;/s,
  );
});
