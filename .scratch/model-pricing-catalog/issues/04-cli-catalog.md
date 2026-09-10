# T-04 接入 CLI 目录命令

状态：blocked  
Blocked by: T-03  
规格：[§7、§9、§10 V-10](../spec.md)  
解锁：T-06。

## 目标

CLI 用户可以查看目录状态、主动更新目录，现有定价命令使用相同缓存。
本票先完成手动闭环，自动补检由 T-06 加入。

## 范围与责任

负责 apps/quota-cli/src/ctx.rs、main.rs、cmd/pricing.rs、cmd/pricing_models.rs、
新目录命令（可独立模块）、render/texts 与测试。
只消费已合入 core API。

1. 新增 quota pricing catalog status / update，均支持 --json。
2. pricing show / model list 改为读取有效目录，不再直接从硬编码预置构造。
3. --config、安装模式和便携模式的数据根遵守现有 CLI 解析规则。
4. status 只读本地；update 显式联网并使用既有代理配置。
5. 清楚区分当前目录可用与最近更新失败；手动更新失败返回非零退出码。

## 验收

- [ ] mock 发布新目录后执行 update，再执行 pricing show，能读到新价和版本。
- [ ] status 和既有 JSON 定价命令不联网且 stdout 只有预期 JSON。
- [ ] source 既有语义不变；新增载体、revision、生命周期与核验信息正确。
- [ ] 已下架/未知模型遵守 T-02；手填价格仍优先。
- [ ] --config 与便携数据根不把缓存误写到默认安装目录。
- [ ] update 无变化退出 0，失败返回既有约定的非零码，busy 结果明确。
- [ ] 不改变 config.json、迁移包和凭据。
- [ ] 帮助信息与中英文输出可用。

## 验证

优先通过 CLI 参数解析、命令执行入口和临时目录串联测试。
网络 mock；检查 stdout / stderr / 退出码，不只测试私有格式化函数。
Rust fmt、workspace all-targets clippy 与相关 CLI/core 测试通过。

## 接手提示

代码库可能有其他人的改动；不回退他人修改。
此票不增加 CLI 后台守护进程或调度器，不重构安装包 update 命令。
完成后记录调用示例、提交/PR 和测试证据。
