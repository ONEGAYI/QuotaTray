// 多窗口形态：POST + Bearer 头，配额窗口为数组、每窗口带重置时刻。
// 演示模板 DSL 覆盖不了的场景——窗口字段异构（每个窗口不同取数逻辑）、
// 剩余额度需本地推导（total − used）、reset_at 相对时刻换算。
function request() {
  return {
    method: "POST",
    url: "{{baseUrl}}/v1/quota/usage",
    headers: {
      "Authorization": "Bearer {{apiKey}}",
      "Content-Type": "application/json"
    },
    body: JSON.stringify({ granularity: "windows" })
  };
}

function extract(resp) {
  const rows = [];
  for (const w of resp.windows) {
    rows.push({
      plan_name: w.name,                    // "five_hour" / "week"
      total: w.limit,
      used: w.used,
      remaining: w.limit - w.used,          // 字段间运算：脚本无此限制（模板受限）
      unit: w.unit,                         // "%"
      reset_at: Date.parse(w.reset_time)    // RFC3339 → epoch 毫秒，供倒计时
    });
  }
  return rows;
}
