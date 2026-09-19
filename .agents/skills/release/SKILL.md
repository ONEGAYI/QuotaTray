---
name: release
description: QuotaTray 项目发布惯例与合规固定文本。凡准备或执行版本发布——bump workspace 版本、运行 .\package 打包、推发布 tag、编写版本 CHANGELOG / Release notes、更新 README 下载说明或便携包内说明——动笔前必读本技能。含 Portable 固定安全提示、ARM64 Preview 声明、Android Preview 声明等逐字固定文本与 notes 组装顺序；固定文本不得缩写、改写、凭记忆复述或仅以链接代替。
---

# QuotaTray 发布惯例

2026-09-19 自 AGENTS.md「发布惯例」节外迁（渐进式披露：AGENTS.md 原位留必读
指针，逐字固定文本全文以本技能为准）。

本章是本项目对通用发布规范的附加门禁。生成 Release notes、README 下载说明和发布
资产时必须同时遵守；固定文本不得缩写、改写或仅以链接代替。

### 基础产物

- 每个 Release 必须附带桌面端 x64 安装包与便携 zip：先把 workspace `Cargo.toml`
  版本号改为目标版本，再于仓库根运行 `.\package`（内部执行 `pnpm tauri build` 并
  组装全部资产）；上传 `target/release/bundle/nsis/*-setup.exe` 与
  `target/release/dist/*-portable.zip`。
- **Android 资产口径**（2026-08-29 更新，签名链已就绪）：推纯三段版本 tag
  （`vX.Y.Z`；含 `-` 后缀的预发布/测试 tag 在 job 层不进入构建，其他非三段
  tag 在断言步失败）后 `android-release.yml` 自动构建固定
  密钥签名的 APK（`QuotaTray_<版本>_android-arm64.apk`），Release 不存在时创建
  草稿并上传；正式发布时人工对同一 tag 执行 `gh release edit` 补全 notes，桌面
  资产经 `gh release upload` 上传（Android 资产由 CI 注入，桌面资产仍走本地
  `.\package` 打包）。Release notes 与 README 提及 Android 端时必须注明 Preview
  状态（真实设备完整验收未完成）。
  versionCode 实际由 workspace `Cargo.toml` 版本派生（`android-tauri.mjs` 经
  `--config` 注入，tauri-cli 按公式 `major*1000000 + minor*1000 + patch` 写入
  `tauri.properties`）；CI 断言该值等于 tag 派生值，因此**推 tag 前必须先 bump
  workspace 版本**，不一致时构建在断言步失败。段位约束：minor 与 patch 不得达到
  1000（与高位版本碰撞），major 不得超过 2147（Android versionCode 为 int32）。
  本项目不发 pre-release tag：tauri 派生忽略 pre 段，rc 与正式版会同
  versionCode，覆盖升级语义无法区分。
- 打包脚本已验证包内 GUI/CLI 的 PE 架构与资产名称一致（`scripts/package.ps1`
  逐 exe 断言 Machine 字段，契约测试 `scripts/package.tests.ps1`）；更新选择
  不得跨架构、跨安装/便携形态回退（core 资产选择器已实现精确匹配）。

### ARM64 Preview 声明（Windows on ARM）

- 本节条款仅约束 **WoA 资产**（`*-arm64-preview*.zip` 与 NSIS）；Android APK 资产
  是独立维度，不触发本节声明，其 Preview 口径见「Android 资产口径」与 Android
  Preview 声明。
- 在真实 WoA 完整验收并经项目所有者重新确认前，所有 WoA ARM64 资产名必须含
  `preview`，README 下载项必须写作“ARM64（预览版）”。
- 只要本次 Release 包含 WoA ARM64 资产，Release notes 与 README 下载节都必须原样包含：

> 🧪 **ARM64 预览版**：ARM64 构建已通过交叉编译与产物架构检查，但尚未完成真实 Windows on ARM 设备的完整运行验收。该资产仅供预览和反馈，不应视为稳定支持。

### Android Preview 声明

- 只要本次 Release 包含 Android APK 资产，Release notes 与 README 下载节都必须
  原样包含下段固定文本（与 README.md 的 Android 小节声明逐字一致，README.en.md
  对应英文版）：

> 🧪 **Android 预览版**：Android 端已在模拟器完成冒烟验收，但尚未完成真实设备的完整运行验收。该资产仅供预览和反馈，不应视为稳定支持。

### Portable 固定安全提示

- 从首次提供 Portable 资产起，**每个 Release** 的 notes 都必须在完整 CHANGELOG 内容
  之后原样追加下段文本；即使该版本未修改 Portable 功能，也不得省略。
- README 的 Portable 下载说明与便携包内说明（中文 `便携版说明.txt` 原样中文、
  英文 `PORTABLE-README.txt` 内固定提示与 README.en.md 逐字一致）必须原样展示
  下段文本。GUI 首启确认页为唯一例外（2026-08-27 所有者确认）：正文精简为核心
  警示两行，完整原文收进问号图标点击展开（悬停展开因卡片居中重排引发闪烁
  回路而弃用；文案键 `portable.noticeFull` 的值仍为下段原文），「取得显式
  确认」的要求不变：

> ⚠️ **便携版安全提示**：便携版会将用于解密凭据的主密钥保存在 `Data/portable.key`。虽然配置中的凭据仍以 AES-GCM 密文存储，但密钥与密文位于同一便携目录，因此整个 `Data/` 目录的保密级别等同明文凭据。请勿将其上传网盘、提交版本库或交给他人；若存储介质遗失或目录泄露，请立即轮换其中使用的全部 API Key。

- 使用 `gh release create --notes` 时，notes 顺序为：版本 CHANGELOG 完整内容 → Portable
  固定安全提示 → ARM64 Preview 声明（本次含 WoA ARM64 资产时）→ Android Preview
  声明（本次含 APK 资产时）。
