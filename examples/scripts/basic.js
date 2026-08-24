// 基础形态：GET + query 参数注入 key，响应为单对象、余额是字符串数字。
// 与 deepseek.json 模板示例同款接口，演示脚本协议的最小闭环。
function request() {
  return {
    method: "GET",
    url: "https://api.example.com/user/balance?key={{apiKey}}",
    headers: { "Accept": "application/json" }
  };
}

function extract(resp) {
  return {
    remaining: resp.balance,          // "110.00" → 110（字符串数字兼容）
    total: resp.total_balance,
    unit: resp.currency,              // "CNY"
    is_valid: resp.is_available
  };
}
