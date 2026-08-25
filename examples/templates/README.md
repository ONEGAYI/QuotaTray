# 声明式模板示例

这里是 [声明式模板](../../README.md#自定义查询声明式模板) 的可运行示例，覆盖几种典型的运营商接口形态。模板里永远只写 `{{apiKey}}` 变量，**不落任何真实密钥**。

## 示例一览

| 文件 | 演示的接口形态 | 覆盖的 DSL 能力 |
|---|---|---|
| `deepseek.json` | GET + Bearer 头，余额为字符串数字 | 单对象取数、字符串数字解析（`"110.00"`）、币种取自响应路径、`isValid` 布尔 |
| `siliconflow.json` | 双站平台（国内/国际共用 API 路径） | `{{baseUrl}}` 变量、`const` 常量、`round` 变换 |
| `openrouter.json` | 只有总额与已用、无剩余字段 | `total`/`used` 双字段展示、受限算术的边界（见下） |
| `multi-window.json` | 数组式多配额窗口 | `windowsFrom` 多窗口展开、loopback 端点（默认放行 http） |
| `newapi.json` | NewAPI 系中转站（one-api 系，需自备站点实测） | `divide` 币值换算（quota÷500000=USD）、`invalidMessage` 失效透出、自定义鉴权头 |

> 预置平台（DeepSeek、SiliconFlow 等）无需模板即可使用——以上以它们为例是为了让示例可用真实平台验证语法。

## NewAPI 系鉴权双要素

`newapi.json` 面向 one-api 系中转站（PackyCode、88Code 等数十家），两处值需改成自己的：

- `Authorization` 的 key 位填站点的**系统访问令牌**（站点个人设置生成，不是 sk- 推理 key）；
- `New-Api-User` 头填你的**用户数字 ID**（模板中的 `"1"` 仅为占位，严格 NewAPI 校验该头，缺失或不符会 401）。

`quota` / `used_quota` 为站内记账单位，按 one-api 惯例 `÷500000` 换算为 USD；个别站点自定义比率时请同步修改 `divide` 的 `by`。

## 试查方法

```bash
# 交互模式：粘贴模板 JSON、空行结束，按提示输入 key（掩码，不落盘）
quota template test --json < deepseek.json

# siliconflow.json 使用 {{baseUrl}}，需提供站点地址
quota template test --json --base-url https://api.siliconflow.cn < siliconflow.json

# 或先 add 为条目（key 走 setkey 加密存储），之后复用密文试查
quota template test --entry my-sf
```

`template test` 会先做静态校验（路径语法、变量名、模式一致性），再发起一次真实请求并输出结果；退出码三分（0 成功 / 1 确定性失败 / 2 瞬时失败）。

## 已知边界

- **transforms 不支持字段间运算**：算术操作数只能是常数（如 `sub` 减去固定值）。OpenRouter 的 `remaining = total_credits − total_usage` 属于字段相减，模板无法表达，示例只展示 `total` 与 `used`——此类平台建议使用内置实现（`quota add` 选 openrouter 预置）。
- **多窗口共用同一映射**：`windowsFrom` 数组的每个元素套用 `windows[0]` 的取数规则，行名统一取其 `name`，适合同构配额（多模型/多额度池）；异构窗口（每个窗口不同取数路径）暂不支持，请拆成多个条目。历史存储（M5）对数组产出的同名多行按出现顺序消歧为 `名称` / `名称#2` 各自记录时间线——键的稳定性依赖响应数组顺序稳定，顺序对调时时间线标签互换。
- **URL 安全**：默认仅允许 HTTPS 与 loopback；其他 http 端点需在模板中显式 `"allowInsecure": true`。

## 本地联调多窗口示例

`multi-window.json` 指向 loopback 端点，可用任意静态服务器联调。例如用 Python 起一个返回固定 JSON 的服务：

```bash
python -c "import http.server,json;h=http.server.HTTPServer(('127.0.0.1',8931),type('H',(http.server.BaseHTTPRequestHandler,),{'do_GET':lambda s:(s.send_response(200),s.send_header('Content-Type','application/json'),s.end_headers(),s.wfile.write(json.dumps({'windows':[{'used':12.3456,'limit':100,'unit':'USD'},{'used':0.5,'limit':5,'unit':'USD'}]}).encode())))}));h.serve_forever()"
```

然后另开终端执行 `quota template test --json < multi-window.json`，输入任意非空 key 即可看到两条窗口数据。
