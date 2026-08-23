# QuotaTray

**[English](README.en.md)** | 中文

托盘常驻的多平台 AI 账户余额监视器：预置官方平台查询，声明式模板自助接入其余平台，凭据全程密文、不落明文。

![OS](https://img.shields.io/badge/OS-Windows-blue) ![License](https://img.shields.io/badge/License-MIT-green)

## 为什么需要 QuotaTray

如果你同时在 DeepSeek、Kimi、OpenRouter 等多个 AI 平台有账户，余额和配额散落在各家控制台里，查看需要逐个登录网页。

QuotaTray 把这件事压缩成一眼：常驻系统托盘，图标即余额状态，悬停或打开菜单即可看到全部平台的余额与已用百分比。

与其他余额工具的关键差异是**凭据安全**：

- API Key 在普通配置中以 AES-256-GCM 密文存储，加密它的机器主密钥由操作系统凭据库托管，**机器主密钥永不导出**；
- 普通配置文件即使被拷走（备份、网盘同步、共享屏幕），离开本机也无法解密；显式生成的跨机器迁移包携带一次性迁移密钥，须按明文凭据同等保护；
- 项目只做"读余额"，不写入任何 CLI 工具的配置文件——这是凭据可以全程密文的前提。

## 功能一览

**托盘与界面（桌面端）**

- 托盘圆环图标：分层叠弧展示各条目，余额低于阈值变色告警
- 托盘菜单逐条展示余额/已用百分比与更新时间，峰谷定价另起两行
- 悬停详情面板：余额优先展示，可快速切换圆环数据源账户与计价模型
- 主窗口卡片列表：添加/编辑条目、模板编辑器（带校验与试查）、峰谷定价结构化编辑
- 明暗主题三态（浅色/深色/跟随系统）、中英双语三态、自定义标题栏
- keep-last-good：查询失败时在时限内继续展示上次成功结果；重启后快照先行，无空窗期

**命令行（quota-cli，与 GUI 平级共享同一核心）**

```text
quota natives                  # 列出预置平台
quota add                      # 交互式添加供应商（掩码输入 key）
quota query                    # 并行查询全部启用条目，表格输出
quota query --watch            # 轮询模式
quota pricing show <id>        # 查看条目峰谷定价与当前时段判定
quota template test --json     # 模板静态校验 + 真实试查
quota update --check           # 检测新版本
```

- 全命令支持 `--json` 输出，供脚本消费
- 退出码三分：`0` 全部成功 / `1` 存在确定性失败 / `2` 仅瞬时失败
- 文案中英双语三态（`--lang zh|en|system`）

**峰谷定价**

- 按「周几 + 时间段」划分高峰/空闲时段，两档各三价：缓存命中/未命中/输出（每 MTokens）
- DeepSeek 官方峰谷价格随版本内置，条目可字段级自定义（留空即回退预置）
- 自定义模型库：按平台增补模型及其价格，条目定价可选用
- 托盘与 CLI 均展示当前时段判定与下次翻转时间

**更新检测**

- 定期检测 GitHub release 新版本（频率与时刻可配，托盘菜单提示新版本行）
- 手动触发检测与安装包下载（下载到系统下载目录，不自动安装）

## 预置平台

| 平台 | 站点 | 说明 |
|---|---|---|
| DeepSeek | — | 单站双币，余额接口返回币种 |
| SiliconFlow | 国内站 / 国际站 | CNY / USD |
| OpenRouter | — | remaining = credits − usage |
| Kimi Open Platform | 国内站 / 国际站 | 余额 + 代金券/现金拆分展示 |
| Kimi Code | kimi.com/code / kimi.ai/code | 5 小时 + 周额度窗口，RFC3339 重置时间 |
| 智谱 / Z.ai | 双站 | GLM Coding Plan 用量（多窗口），裸 key |

Kimi Code 使用 MoonshotAI 官方客户端采用的用量端点；智谱 / Z.ai 使用非公开文档端点，其余为官方公开接口。自动化测试全 mock，不依赖真实账号。**未预置的平台用声明式模板接入**（见下节）。

## 自定义查询：声明式模板

多数平台的余额接口是"一个 GET + 鉴权头 + 取字段 ± 算术"，用 JSON 描述即可接入，无需写代码：

```json
{
  "request": {
    "url": "{{baseUrl}}/v1/user/info",
    "headers": { "Authorization": "Bearer {{apiKey}}" }
  },
  "extract": {
    "remaining": "$.data.totalBalance",
    "unit": { "const": "CNY" }
  },
  "transforms": [
    { "op": "multiply", "field": "remaining", "by": 0.01 }
  ],
  "windows": []
}
```

- `extract` 用 JSONPath 子集（`$.a.b[0]`）取值或直接给常量
- `transforms` 提供受限算术（乘/除/加/减/取整），执行期无 eval
- `windows` 支持从同构额度数组展开多窗口；Kimi Code 这类异构响应由预置平台实现处理
- 保存时静态校验；URL 仅允许 HTTPS 且须与 `{{baseUrl}}` 同源（loopback 除外）

可运行示例见 [examples/templates/](examples/templates/)：覆盖单对象取数（字符串数字）、双站 `{{baseUrl}}`、总额/已用展示、多窗口展开等形态，均可用 `quota template test` 试查验证。

更复杂的平台（多请求聚合、特殊签名）计划由 QuickJS 沙箱脚本兜底，见[路线图](#路线图)。

## 安装

### Windows

从 [Releases](https://github.com/ONEGAYI/QuotaTray/releases) 下载 NSIS 安装包（`*-setup.exe`）安装。

### 从源码构建

要求：Rust stable、Node.js、pnpm。

```bash
# 桌面端安装包（apps/quota-desktop/dist 下产出 NSIS）
cd apps/quota-desktop
pnpm install
pnpm tauri build

# 仅 CLI
cargo build -p quota-cli --release
```

### 清理开发目录

Windows 在仓库根目录运行 `clean`，不传级别时可交互选择：

```powershell
.\clean 1              # 轻量：增量/Vite 缓存与生成物
.\clean 2              # 标准：再清理完整 target/debug，保留 release
.\clean 3              # 深度：完整 target + node_modules + 生成物
.\clean 3 -WhatIf      # 只预览目标，不删除
```

清理器只操作仓库内的固定白名单路径，不会删除源码、`.git`、开发密钥、
`.zcode` 或未提交文件。Level 3 后需在 `apps/quota-desktop` 重新执行
`pnpm install`，Rust 依赖也会在下次构建时完整重编译。

## 安全设计

密钥分层如下：

```
系统凭据库（Windows 凭据管理器）
  └─ 主密钥：32 字节纯随机，首次运行生成，永不落盘明文
        │ AES-256-GCM
        ▼
~/.quotatray/config.json 中的凭据字段（v1:<base64>，带版本号）
```

- 主密钥每台机器独立，与源码零关联
- 密文格式含版本号，未来算法升级可平滑迁移
- GCM 认证标签保证完整性，篡改即解密失败
- 日志与错误信息中的凭据一律掩码显示（`sk-****<尾4位>`）
- 前端/GUI 永不接收明文凭据：查询在本地后端完成，GUI 只展示结果；编辑凭据走"写入专用"通道，不回显

跨机器迁移使用 `.qtray-export` 私有二进制容器。core 每次导出都会生成新的
32 字节一次性迁移密钥，将源凭据转写并整体认证加密；导入时再转写到目标机器
主密钥。**迁移包虽然不可直接阅读，但因携带一次性迁移密钥，敏感级别等同明文
凭据**，应避免同步到不受信任位置，并在迁移完成后删除。

已知边界：同机同用户进程读取系统凭据库不在防御范围（与浏览器保存密码同一水位）；内存攻击与本机恶意软件超出桌面工具防线。

## 路线图

- [ ] QuickJS 沙箱脚本查询（`{request, extractor}` 协议，内存/CPU 限额，无网络无文件系统）
- [ ] 更多预置平台
- [ ] 更新自动安装

## 致谢

余额查询的统一结果模型与错误双轨分类参考了 [cc-switch](https://github.com/farion1231/cc-switch)（MIT 许可）的实践，感谢其开源。

## 许可证

[MIT](LICENSE) © 2026 ONEGAYI
