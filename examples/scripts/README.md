# JS 脚本查询示例

这里是 [脚本查询](../../README.md#自定义查询js-脚本) 的可运行示例，覆盖两种典型形态。脚本里永远只写 `{{apiKey}}` / `{{baseUrl}}` 变量占位，**不落任何真实密钥**——变量在执行期由宿主做代码字符串层面替换，脚本可以安全分享。

## 脚本协议

沙箱内定义两个全局函数（QuickJS 环境，无网络/文件系统，内存 16MiB、单次执行 5 秒 CPU 上限）：

```js
function request() {
  // 返回请求描述：{ method?: "GET"|"POST", url: string, headers?: {}, body?: string }
  return { url: "https://api.example.com/me?key={{apiKey}}" };
}
function extract(resp) {
  // resp = 已解析的响应 JSON；返回单对象或数组（多窗口），字段：
  // plan_name / total / used / remaining / unit / reset_at / is_valid /
  // invalid_message / extra——数值字段至少提供其一
  return [{ remaining: resp.balance, unit: "CNY" }];
}
```

## 示例一览

| 文件 | 演示的接口形态 | 覆盖的能力 |
|---|---|---|
| `basic.js` | GET + query 参数注入 key，单对象响应 | 最小闭环、字符串数字解析、`is_valid` 透传 |
| `multi-window.js` | POST + Bearer 头 + `{{baseUrl}}`，数组式配额窗口 | 多窗口、字段间运算（模板 DSL 不支持）、`reset_at` 倒计时 |

两个示例指向 `api.example.com` / 需 `--base-url`，请替换为真实端点后使用；本地联调可用任意 loopback 服务（脚本 URL 安全规则与模板一致：HTTPS 与 loopback 默认放行）。

## 试查方法

`quota script test --json` 接受**纯 JS 文件**或脚本配置 JSON（`{"code": "...", "allowInsecure": true?}`）两种 stdin 形态：

```bash
# 直接重定向 .js 文件；脚本引用 {{apiKey}} 时按提示输入 key（掩码，不落盘）
quota script test --json < basic.js

# multi-window.js 使用 {{baseUrl}}，需提供站点地址
quota script test --json --base-url https://api.example.com < multi-window.js

# 或先 add 为条目（向导选「script —— 自定义 JS 脚本」，粘贴代码），
# key 走 setkey 加密存储，之后复用密文试查
quota script test --entry my-quota
```

试查先做保存期同款校验（干跑：假变量替换后执行脚本、验证双函数与 `request()` 产物形状），再走引擎完整链路发起一次真实请求；退出码三分（0 成功 / 1 确定性失败 / 2 瞬时失败）。

## 何时用脚本而非模板

- **模板够用就别用脚本**：单对象/多窗口取数、常数算术、字段映射，模板 DSL 零代码更安全；
- 脚本的增量能力：**字段间运算**（如 `limit − used`）、循环/条件聚合、响应裁剪重组、`Date.parse` 等标准库调用；
- 脚本同样受 URL 安全（HTTPS/loopback，`allowInsecure` 显式放开）与凭据红线约束：错误消息中的回显密钥会被统一打码。

## 已知边界

- 终端多行粘贴（向导 / `script test --json` 交互输入）以**单独一行 `.`** 结束——JS 代码中的空行照常保留；单独一行仅含 `.` 被视为结束符，代码中请避免该形态（管道重定向喂入则读到 EOF，无此约定）；
- `reset_at` 只接受整数毫秒或整数字符串；运算产生的浮点值（如 `Date.parse(...) / 1 + 0.5`）会被安静丢弃（倒计时宁缺毋错）；
- 沙箱无 `fetch`/`setTimeout`/`require`：HTTP 由宿主执行，脚本只做纯计算；
- 数值字段经 JSON 往返，`NaN`/`Infinity` 会安静落为空值（显示 `-`），不会污染序列化。
