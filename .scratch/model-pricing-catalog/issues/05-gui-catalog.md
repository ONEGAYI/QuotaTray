# T-05 接入桌面和 Android 手动更新

状态：blocked  
Blocked by: T-03  
规格：[§5、§7、§9、§10 V-08](../spec.md)  
解锁：T-06。

## 目标

用户在设置中手动更新后，卡片、模型列表和桌面托盘立即使用新目录，
已打开的编辑草稿保持不变。Android 使用同一数据和状态。

## 范围与责任

负责 Tauri state/commands/lib/tray 的目录装配与 IPC；
frontend api/types/queries/providerPricing、PricingSection、SettingsDialog、
ProviderCard 等必要展示、双语字典与测试。
修改样式前必须读 frontend-style-spec 对应 references；按需更新规范和移动缺口文档。

1. Tauri 初始化有效目录快照，并提供 status / update 命令。
2. native_meta_dtos 从当前目录生成模型信息；兼容双币种和自定义模型库。
3. 设置更新区域显示目录来源、版本、检查时间、错误和“立即更新”。
4. 成功切换目录后广播事件，失效 native-metas，重建托盘定价和峰谷信息。
5. 前端镜像解析与 core 的 active/retired/missing、自定义覆盖保持一致。
6. 编辑会话固定基准快照；收到更新保留草稿，提示重新打开可使用新数据。

## 验收

- [ ] 手动更新 mock 目录后，已打开主窗无需重启即显示新模型、新价和对应状态。
- [ ] 桌面托盘与主窗使用同一 revision；失效缓存不会触发真实余额查询作为副作用。
- [ ] Android 入口可触摸操作，状态可见，不依赖 hover。
- [ ] retired 当前选项保留并显示已下架；missing 显示未知，不出现默认模型价格。
- [ ] 正在编辑的模型和手填值不被更新重置，也不保存一整份旧官方价格到用户配置。
- [ ] 失败不清空已展示价格；不会错误显示“更新成功”或要求重启。
- [ ] 前端/core 对照测试覆盖币种、订阅、整档覆盖、未知与下架。
- [ ] 中英文文案、状态 DTO 与类型镜像一致。

## 验证

从 IPC、queries 事件处理和前端视图入口验证，补一轮桌面沙箱及 Android 模拟器冒烟。
Rust fmt/clippy；前端 pnpm lint --fix、相关测试、pnpm build。
触及 Android cfg 时完成相应交叉 clippy 或记录 CI 证据。

## 接手提示

代码库可能有其他人的改动；不回退他人修改，尤其当前已有未提交的桌面 Cargo.toml / Cargo.lock 改动。
只实现手动闭环，自动调度放 T-06；不得借此启动真实更新发布。
完成后记录截图/冒烟证据、提交/PR 和测试结果。
