import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// 端口契约：desktop:dev（scripts/dev.mjs）探测后注入 QUOTA_DEV_PORT 并同步覆盖
// tauri devUrl；strictPort 保持 true——外层已探测，绑不上即快速失败而非漂移，
// 避免 vite 实际端口与 tauri.conf 的 devUrl 不一致导致窗口空白。
// Tauri 约定：不清屏，构建目标对齐 WebView2。
const devPort = Number(process.env.QUOTA_DEV_PORT) || 1420;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: devPort,
    strictPort: true,
  },
  build: {
    target: "chrome110",
  },
});
