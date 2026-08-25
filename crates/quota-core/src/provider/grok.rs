//! Grok 订阅（SuperGrok）credits 用量——CLI 凭据复用。
//!
//! 凭据只读 `~/.grok/auth.json`（grok CLI 登录后生成）：顶层为
//! scope → 条目 的 map，`https://auth.x.ai::` 前缀（OIDC/SuperGrok）
//! 优先、`/sign-in`（legacy）兜底，条目 `key` 字段即 Bearer token。
//! token 不刷新（grok CLI 自刷，约 7 天），过期由服务端兜底。
//!
//! 查询走 gRPC-web：`POST grok.com/grok_api_v2.GrokBuildBilling/
//! GetGrokCreditsConfig`，body 为 5 字节空 data 帧，带 Origin/Referer/
//! `x-grpc-web`/`x-user-agent: connect-es` 整组浏览器伪装头（cc-switch
//! 实证缺一不可）。响应无公开 .proto——帧拆分后对 data 帧做通用
//! protobuf 递归扫描，按字段路径启发提取已用百分比（fixed32）与
//! 重置时间戳（varint，Unix 秒）。**响应体必须走 HttpResponse.raw
//! 字节保真通道**（body 是 lossy UTF-8，会损坏二进制）。
//!
//! 契约移植自 cc-switch subscription_grok.rs（CodexBar 移植链），
//! 未经真机验证。

use async_trait::async_trait;
use serde_json::Value;

use super::{NativeMeta, NativeProvider};
use crate::config::{Credentials, PlanVariant};
use crate::http::{HttpClient, HttpRequest, Method};
use crate::model::{QueryError, UsageData};

const RELOGIN_HINT: &str = "Grok 凭据已失效，请在 Grok CLI 中重新登录后再查询";
const BILLING_ENDPOINT: &str = "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";
const OIDC_SCOPE_PREFIX: &str = "https://auth.x.ai::";
const LEGACY_SESSION_SCOPE: &str = "https://accounts.x.ai/sign-in";

/// 合理 Unix 秒区间（重置时间戳候选过滤；上界覆盖到 2036 年）。
const RESET_SECS_MIN: u64 = 1_700_000_000;
const RESET_SECS_MAX: u64 = 2_100_000_000;
/// 递归扫描的最大嵌套深度（响应层级远小于此，防病态嵌套）。
const MAX_SCAN_DEPTH: usize = 4;

pub struct Grok;

#[async_trait]
impl NativeProvider for Grok {
    fn meta(&self) -> NativeMeta {
        NativeMeta {
            id: "grok",
            name: "Grok 订阅",
        }
    }

    async fn query(
        &self,
        _creds: &Credentials,
        http: &dyn HttpClient,
        _variant: PlanVariant,
    ) -> Result<Vec<UsageData>, QueryError> {
        let token = read_grok_token().map_err(QueryError::deterministic)?;
        query_with_token(&token, http).await
    }
}

fn read_grok_token() -> Result<String, String> {
    let Some(home) = dirs::home_dir() else {
        return Err("无法定位用户主目录".into());
    };
    let path = home.join(".grok").join("auth.json");
    let content = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "未找到 {}，请先在本机安装 Grok CLI 并登录后再添加本平台",
            path.display()
        )
    })?;
    parse_grok_token(&content)
}

/// 纯函数：scope map → Bearer token。OIDC 条目优先（key 非空才算
/// 候选——残缺 OIDC 不遮蔽健康 legacy）。
fn parse_grok_token(content: &str) -> Result<String, String> {
    let v: Value =
        serde_json::from_str(content).map_err(|e| format!("auth.json 不是有效 JSON：{e}"))?;
    let map = v
        .as_object()
        .ok_or_else(|| "auth.json 顶层应为 scope → 条目的对象".to_string())?;
    let key_of = |entry: &Value| {
        entry
            .get("key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let mut oidc: Option<String> = None;
    let mut legacy: Option<String> = None;
    for (scope, entry) in map {
        let Some(key) = key_of(entry) else { continue };
        if scope.starts_with(OIDC_SCOPE_PREFIX) && oidc.is_none() {
            oidc = Some(key);
        } else if (scope == LEGACY_SESSION_SCOPE || scope.contains("/sign-in")) && legacy.is_none()
        {
            legacy = Some(key);
        }
    }
    oidc.or(legacy)
        .ok_or_else(|| "auth.json 中没有可用凭据条目（缺少非空 key，未登录？）".to_string())
}

async fn query_with_token(
    token: &str,
    http: &dyn HttpClient,
) -> Result<Vec<UsageData>, QueryError> {
    let req = HttpRequest {
        method: Method::Post,
        url: BILLING_ENDPOINT.into(),
        headers: vec![
            ("Authorization".into(), format!("Bearer {token}")),
            ("Origin".into(), "https://grok.com".into()),
            ("Referer".into(), "https://grok.com/?_s=usage".into()),
            ("Accept".into(), "*/*".into()),
            ("Content-Type".into(), "application/grpc-web+proto".into()),
            ("x-grpc-web".into(), "1".into()),
            ("x-user-agent".into(), "connect-es/2.1.1".into()),
            ("User-Agent".into(), "QuotaTray".into()),
        ],
        // 空 data 帧：flags=0 + 4 字节大端长度 0
        body: Some("\0\0\0\0\0".into()),
        declared_secrets: vec![token.to_string()],
    };
    let resp = http.execute(req.clone()).await.map_err(|e| match &e {
        crate::http::HttpError::Timeout | crate::http::HttpError::Network(_) => {
            QueryError::transient(e.to_string())
        }
        crate::http::HttpError::InvalidRequest(_) => QueryError::deterministic(e.to_string()),
    })?;

    if resp.status == 401 || resp.status == 403 {
        return Err(QueryError::deterministic(RELOGIN_HINT.to_string()));
    }
    // 瞬时分类与全仓口径对齐：408/429/5xx 可重试（grok.com 在
    // Cloudflare 后，429/502/503 抖动常见）；二进制协议的 body
    // 无可读错误信息，不透传乱码 detail
    let transient = resp.status == 408 || resp.status == 429 || (500..600).contains(&resp.status);
    if transient {
        return Err(QueryError::transient(format!("HTTP {}", resp.status)));
    }
    if !resp.is_success() {
        return Err(QueryError::deterministic(format!("HTTP {}", resp.status)));
    }

    // gRPC 状态可能在 body 的 trailer 帧中（trailers-only 形态在 HTTP
    // 头里的场景本通道不可见，由上面的 HTTP 状态码兜底）
    if let Some((status, message)) = grpc_trailer_status(&resp.raw) {
        if is_grpc_auth_failure(status, message.as_deref()) {
            return Err(QueryError::deterministic(RELOGIN_HINT.to_string()));
        }
        if is_transient_grpc_status(status, message.as_deref()) {
            return Err(QueryError::transient(format!("gRPC 状态 {status}")));
        }
        if status != 0 {
            return Err(QueryError::deterministic(format!("gRPC 状态 {status}")));
        }
    }

    let now_secs = chrono::Utc::now().timestamp().max(0) as u64;
    let (used_percent, reset_at) = extract_billing(&resp.raw, now_secs).ok_or_else(|| {
        QueryError::deterministic("Grok 订阅响应解析失败（protobuf 启发式未命中）".to_string())
    })?;

    Ok(vec![UsageData {
        plan_name: Some("Grok 订阅".into()),
        total: Some(100.0),
        used: Some(used_percent),
        remaining: Some(100.0 - used_percent),
        unit: Some("%".into()),
        reset_at: reset_at.map(|secs| (secs as i64) * 1000),
        ..Default::default()
    }])
}

// ---- gRPC-web 帧与 protobuf 启发式（移植 cc-switch 算法） -----------

/// 拆出全部 data 帧载荷。任一帧长度越界 → 整体判非法（空）；
/// 无有效帧且首字节像合法 tag → 当裸 protobuf 整体扫描。
fn grpc_web_data_frames(bytes: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut i = 0usize;
    while i + 5 <= bytes.len() {
        let flags = bytes[i];
        let len =
            u32::from_be_bytes([bytes[i + 1], bytes[i + 2], bytes[i + 3], bytes[i + 4]]) as usize;
        let start = i + 5;
        // 帧长度越界：整体判非法（清空跳出，由底部裸 protobuf 兜底）
        let Some(end) = start.checked_add(len) else {
            frames.clear();
            break;
        };
        if end > bytes.len() {
            frames.clear();
            break;
        }
        if flags & 0x80 == 0 {
            frames.push(&bytes[start..end]);
        }
        i = end;
    }
    if frames.is_empty() && !bytes.is_empty() {
        let key = bytes[0];
        let field = key >> 3;
        let wire = key & 7;
        if field > 0 && matches!(wire, 0 | 1 | 2 | 5) {
            return vec![bytes];
        }
    }
    frames
}

/// 从 trailer 帧（flags & 0x80）提取 `grpc-status` / `grpc-message`。
fn grpc_trailer_status(bytes: &[u8]) -> Option<(u32, Option<String>)> {
    let mut i = 0usize;
    while i + 5 <= bytes.len() {
        let flags = bytes[i];
        let len =
            u32::from_be_bytes([bytes[i + 1], bytes[i + 2], bytes[i + 3], bytes[i + 4]]) as usize;
        let start = i + 5;
        let Some(end) = start.checked_add(len) else {
            return None;
        };
        if end > bytes.len() {
            return None;
        }
        if flags & 0x80 != 0 {
            let text = String::from_utf8_lossy(&bytes[start..end]);
            let mut status = None;
            let mut message = None;
            for line in text.lines() {
                if let Some((k, v)) = line.split_once(':') {
                    let k = k.trim();
                    let v = percent_decode(v.trim());
                    if k.eq_ignore_ascii_case("grpc-status") {
                        status = v.parse::<u32>().ok();
                    } else if k.eq_ignore_ascii_case("grpc-message") {
                        message = Some(v);
                    }
                }
            }
            if let Some(status) = status {
                return Some((status, message));
            }
        }
        i = end;
    }
    None
}

/// percent 编码解码（服务端可控内容：按字节切再校验 UTF-8）。
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 && i + 2 < bytes.len() + 1 {
            let hex = |b: u8| (b as char).to_digit(16);
            if i + 2 < bytes.len() {
                if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    out.push((h * 16 + l) as u8);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// gRPC 认证失败判定：UNAUTHENTICATED(16) 恒真；PERMISSION_DENIED(7)
/// 按文案关键词判定。
fn is_grpc_auth_failure(status: u32, message: Option<&str>) -> bool {
    if status == 16 {
        return true;
    }
    if status != 7 {
        return false;
    }
    let Some(m) = message else { return false };
    let m = m.to_lowercase();
    m.contains("bad-credentials")
        || m.contains("unauthenticated")
        || (m.contains("oauth2") && m.contains("could not be validated"))
        || (m.contains("access token")
            && ["invalid", "expired", "could not be validated"]
                .iter()
                .any(|k| m.contains(k)))
}

/// 瞬时 gRPC 状态：DEADLINE_EXCEEDED(4)/UNAVAILABLE(14) 恒瞬时；
/// CANCELLED(1) 仅超时类文案。
fn is_transient_grpc_status(status: u32, message: Option<&str>) -> bool {
    match status {
        4 | 14 => true,
        1 => message
            .map(|m| {
                let m = m.to_lowercase();
                m.contains("timeout") || m.contains("deadline") || m.contains("expired")
            })
            .unwrap_or(false),
        _ => false,
    }
}

/// protobuf 扫描产物。
#[derive(Default)]
struct Scan {
    /// (字段路径, varint 值)
    varints: Vec<(Vec<u32>, u64)>,
    /// (字段路径, fixed32 值, 顶层帧内序号)
    fixed32s: Vec<(Vec<u32>, f32, usize)>,
}

fn read_varint(bytes: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        value |= ((b & 0x7f) as u64) << shift.min(63);
        if b & 0x80 == 0 {
            return Some((value, i));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

/// 递归扫描（无 proto，wire type 驱动；length-delimited 在深度内
/// 一律尝试当嵌套消息递归）。varint/length 越界从下一字节重同步；
/// 定宽字段（wire 1/5）越界视为截断消息，放弃本层剩余字节。
fn scan_protobuf(bytes: &[u8], depth: usize, path: &[u32], scan: &mut Scan, order: &mut usize) {
    let mut i = 0usize;
    while i < bytes.len() {
        let field_start = i;
        let Some((key, after_key)) = read_varint(bytes, i) else {
            break;
        };
        if key == 0 {
            i = field_start + 1;
            continue;
        }
        let field = (key >> 3) as u32;
        let wire = (key & 7) as u8;
        let mut sub_path = path.to_vec();
        sub_path.push(field);
        match wire {
            0 => match read_varint(bytes, after_key) {
                Some((value, after)) => {
                    scan.varints.push((sub_path, value));
                    i = after;
                }
                None => i = field_start + 1,
            },
            1 => {
                if after_key + 8 > bytes.len() {
                    break;
                }
                i = after_key + 8;
            }
            2 => {
                let Some((len, after_len)) = read_varint(bytes, after_key) else {
                    i = field_start + 1;
                    continue;
                };
                let Some(end) = after_len.checked_add(len as usize) else {
                    i = field_start + 1;
                    continue;
                };
                if end > bytes.len() {
                    i = field_start + 1;
                    continue;
                }
                if depth < MAX_SCAN_DEPTH {
                    scan_protobuf(&bytes[after_len..end], depth + 1, &sub_path, scan, order);
                }
                i = end;
            }
            5 => {
                if after_key + 4 > bytes.len() {
                    break;
                }
                let v = f32::from_le_bytes([
                    bytes[after_key],
                    bytes[after_key + 1],
                    bytes[after_key + 2],
                    bytes[after_key + 3],
                ]);
                scan.fixed32s.push((sub_path, v, *order));
                *order += 1;
                i = after_key + 4;
            }
            _ => i = field_start + 1,
        }
    }
}

/// 用量周期标记（proto3 零用量特判的旁证）。
fn has_usage_period(varints: &[(Vec<u32>, u64)]) -> bool {
    varints.iter().any(|(p, _)| p.starts_with(&[1, 6]))
        || varints
            .iter()
            .any(|(p, v)| p.as_slice() == [1, 8, 1] && (*v == 1 || *v == 2))
}

/// 启发提取：(已用百分比, Option<重置 Unix 秒>)。
/// - percent：路径末段 field 1 的 fixed32、值域 [0,100]、路径最浅最早
/// - reset：Unix 秒区间内的未来值，路径 [1,5,1] 优先、否则全体最小
/// - 零用量：percent 未命中且 fixed32 全空 + reset 存在 + 周期标记 → 0
fn extract_billing(bytes: &[u8], now_secs: u64) -> Option<(f64, Option<u64>)> {
    let mut scan = Scan::default();
    for frame in grpc_web_data_frames(bytes) {
        let mut order = 0usize; // fixed32 序号在每个顶层帧内独立计数
        scan_protobuf(frame, 0, &[], &mut scan, &mut order);
    }
    let percent = scan
        .fixed32s
        .iter()
        .filter(|(p, v, _)| p.last() == Some(&1) && v.is_finite() && (0.0..=100.0).contains(v))
        .min_by_key(|(p, _, o)| (p.len(), *o))
        .map(|(_, v, _)| *v as f64);

    let resets: Vec<u64> = scan
        .varints
        .iter()
        .filter(|(p, v)| {
            (RESET_SECS_MIN..=RESET_SECS_MAX).contains(v) && *v > now_secs && p.len() > 1
        })
        .map(|(_, v)| *v)
        .collect();
    let reset = scan
        .varints
        .iter()
        .filter(|(p, v)| p.as_slice() == [1, 5, 1] && resets.contains(v))
        .map(|(_, v)| *v)
        .min()
        .or_else(|| resets.iter().copied().min());

    let percent = match percent {
        Some(p) => Some(p),
        None if scan.fixed32s.is_empty() && reset.is_some() && has_usage_period(&scan.varints) => {
            Some(0.0)
        }
        None => None,
    }?;
    Some((percent, reset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::testing::MockHttp;

    const TOKEN: &str = "grok-test-bearer";

    // ---- protobuf 测试构造器 ----
    fn varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out
    }
    fn f_varint(field: u32, v: u64) -> Vec<u8> {
        let mut b = vec![((field << 3) | 0) as u8];
        b.extend(varint(v));
        b
    }
    fn f_f32(field: u32, v: f32) -> Vec<u8> {
        let mut b = vec![((field << 3) | 5) as u8];
        b.extend(v.to_le_bytes());
        b
    }
    fn f_msg(field: u32, inner: &[u8]) -> Vec<u8> {
        let mut b = vec![((field << 3) | 2) as u8];
        b.extend(varint(inner.len() as u64));
        b.extend_from_slice(inner);
        b
    }
    fn grpc_data_frame(payload: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8];
        b.extend((payload.len() as u32).to_be_bytes());
        b.extend_from_slice(payload);
        b
    }
    fn grpc_trailer_frame(text: &str) -> Vec<u8> {
        let mut b = vec![0x80u8];
        b.extend((text.len() as u32).to_be_bytes());
        b.extend_from_slice(text.as_bytes());
        b
    }

    /// 典型响应：message{1: message{1: float(百分比), 5: message{1:
    /// varint(reset)}, 6: message{1: 周期标记}}}（cc-switch 实测形态）。
    fn typical_payload(percent: f32, reset: u64) -> Vec<u8> {
        let inner = [
            f_f32(1, percent),
            f_msg(5, &f_varint(1, reset)),
            f_msg(6, &f_varint(1, 1)),
        ]
        .concat();
        f_msg(1, &inner)
    }

    const FUTURE_RESET: u64 = 2_000_000_000;

    /// 正常路径：帧包裹与裸 protobuf 双形态都能提取。
    #[test]
    fn extracts_percent_and_reset() {
        let payload = typical_payload(42.0, FUTURE_RESET);
        let framed = grpc_data_frame(&payload);
        let now = 1_800_000_000;
        assert_eq!(
            extract_billing(&framed, now),
            Some((42.0, Some(FUTURE_RESET)))
        );
        assert_eq!(
            extract_billing(&payload, now),
            Some((42.0, Some(FUTURE_RESET))),
            "裸 protobuf（无帧头）整体扫描"
        );
    }

    /// 零用量特判：无 fixed32 + reset 存在 + 周期标记 → 0%。
    #[test]
    fn zero_usage_when_no_fixed32_but_period() {
        let inner = [
            f_msg(5, &f_varint(1, FUTURE_RESET)),
            f_msg(6, &f_varint(1, 1)),
        ]
        .concat();
        let framed = grpc_data_frame(&f_msg(1, &inner));
        assert_eq!(
            extract_billing(&framed, 1_800_000_000),
            Some((0.0, Some(FUTURE_RESET)))
        );
    }

    /// percent 候选过滤：越界 float 不误取；同值域多候选取路径最浅
    /// （min_by_key (len, order) 规则被真实锁定——改成 max/最深即红）。
    #[test]
    fn percent_candidates_filtered() {
        let inner = [f_f32(1, 80.0), f_msg(2, &f_f32(1, 30.0))].concat();
        let framed = grpc_data_frame(&f_msg(1, &inner));
        assert_eq!(
            extract_billing(&framed, 1_800_000_000).unwrap().0,
            80.0,
            "浅者胜"
        );
        let inner = [f_f32(1, 150.0), f_msg(2, &f_f32(1, 30.0))].concat();
        let framed = grpc_data_frame(&f_msg(1, &inner));
        assert_eq!(
            extract_billing(&framed, 1_800_000_000).unwrap().0,
            30.0,
            "越界被滤后取深层"
        );
    }

    /// reset 候选过滤：过去时间与区间外值不取。
    #[test]
    fn reset_candidates_filtered() {
        let inner = [
            f_f32(1, 10.0),
            f_msg(5, &f_varint(1, 1_000_000_000)), // 区间外（过去）
            f_msg(6, &f_varint(1, 1)),
        ]
        .concat();
        let framed = grpc_data_frame(&f_msg(1, &inner));
        let (pct, reset) = extract_billing(&framed, 1_800_000_000).unwrap();
        assert_eq!(pct, 10.0);
        assert_eq!(reset, None, "过去时间戳不是重置时刻");
    }

    /// 提取完全失败 → None（确定性解析失败路径）。
    #[test]
    fn garbage_yields_none() {
        assert_eq!(extract_billing(&[], 0), None);
        assert_eq!(extract_billing(&[0xff, 0xff], 0), None);
    }

    /// trailer 解析：grpc-status/grpc-message（含 percent 编码）。
    #[test]
    fn parses_trailer_status() {
        let body = grpc_trailer_frame("grpc-status:16\r\ngrpc-message:bad-credentials");
        assert_eq!(
            grpc_trailer_status(&body),
            Some((16, Some("bad-credentials".into())))
        );
        let encoded = grpc_trailer_frame("grpc-status:7\r\ngrpc-message:unauth%20enticated");
        let (_, msg) = grpc_trailer_status(&encoded).unwrap();
        assert_eq!(msg.as_deref(), Some("unauth enticated"), "percent 解码");
        assert_eq!(grpc_trailer_status(&[0u8; 3]), None, "坏帧返回 None");
    }

    /// gRPC 状态分类。
    #[test]
    fn grpc_status_classification() {
        assert!(is_grpc_auth_failure(16, None));
        assert!(is_grpc_auth_failure(7, Some("Bad-Credentials")));
        assert!(!is_grpc_auth_failure(7, Some("quota exceeded")));
        assert!(!is_grpc_auth_failure(9, None));
        assert!(is_transient_grpc_status(4, None));
        assert!(is_transient_grpc_status(14, None));
        assert!(is_transient_grpc_status(1, Some("context deadline")));
        assert!(!is_transient_grpc_status(1, Some("client cancel")));
        assert!(!is_transient_grpc_status(2, None));
    }

    /// 端到端（ok_raw 精确注入）：单条 Grok 订阅，reset 秒→毫秒。
    #[tokio::test]
    async fn parses_billing_response() {
        let body = grpc_data_frame(&typical_payload(12.5, FUTURE_RESET));
        let data = query_with_token(TOKEN, &MockHttp::ok_raw(&body))
            .await
            .unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].plan_name.as_deref(), Some("Grok 订阅"));
        assert_eq!(data[0].used, Some(12.5));
        assert_eq!(data[0].remaining, Some(87.5));
        assert_eq!(data[0].reset_at, Some(FUTURE_RESET as i64 * 1000));
        assert_eq!(data[0].unit.as_deref(), Some("%"));
    }

    /// HTTP 401/403 → 确定性重登引导。
    #[tokio::test]
    async fn http_auth_failure_hints_relogin() {
        for status in [401u16, 403] {
            let mut mock = MockHttp::ok("");
            mock.status = status;
            let err = query_with_token(TOKEN, &mock).await.unwrap_err();
            assert!(!err.is_transient());
            assert_eq!(err.message(), RELOGIN_HINT);
        }
    }

    /// 提取层完全未命中 → 确定性解析失败（不产假数据）。
    #[tokio::test]
    async fn unparseable_response_is_deterministic() {
        let err = query_with_token(TOKEN, &MockHttp::ok_raw(&[0xff, 0xfe, 0xfd]))
            .await
            .unwrap_err();
        assert!(!err.is_transient());
    }

    /// gRPC 认证失败（trailer 状态 16）→ 确定性重登引导。
    #[tokio::test]
    async fn grpc_auth_failure_hints_relogin() {
        let trailer = grpc_trailer_frame("grpc-status:16\r\ngrpc-message:bad-credentials");
        let mock = MockHttp::ok_raw(&trailer);
        let err = query_with_token(TOKEN, &mock).await.unwrap_err();
        assert!(!err.is_transient());
        assert_eq!(err.message(), RELOGIN_HINT);
    }

    /// gRPC 瞬时状态（4/14）→ transient；HTTP 408 → transient。
    #[tokio::test]
    async fn grpc_transient_statuses() {
        for status in ["4", "14"] {
            let trailer = grpc_trailer_frame(&format!("grpc-status:{status}"));
            let mock = MockHttp::ok_raw(&trailer);
            assert!(
                query_with_token(TOKEN, &mock)
                    .await
                    .unwrap_err()
                    .is_transient(),
                "gRPC {status} 应为瞬时"
            );
        }
        for status in [408u16, 429, 502, 503] {
            let mut mock = MockHttp::ok("");
            mock.status = status;
            assert!(
                query_with_token(TOKEN, &mock)
                    .await
                    .unwrap_err()
                    .is_transient(),
                "HTTP {status} 应为瞬时"
            );
        }
        let mut mock = MockHttp::ok("");
        mock.status = 404;
        assert!(
            !query_with_token(TOKEN, &mock)
                .await
                .unwrap_err()
                .is_transient()
        );
    }

    /// 请求契约：billing 端点 + 空帧 + 伪装头组。
    #[tokio::test]
    async fn hits_billing_endpoint_with_grpc_headers() {
        let mock = MockHttp::ok_raw(&grpc_data_frame(&typical_payload(1.0, FUTURE_RESET)));
        let _ = query_with_token(TOKEN, &mock).await;
        let reqs = mock.captured_requests();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].url, BILLING_ENDPOINT);
        assert_eq!(reqs[0].method, Method::Post);
        assert_eq!(reqs[0].body.as_deref(), Some("\0\0\0\0\0"));
        let header = |name: &str| {
            reqs[0]
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(header("content-type"), Some("application/grpc-web+proto"));
        assert_eq!(header("x-grpc-web"), Some("1"));
        assert_eq!(header("x-user-agent"), Some("connect-es/2.1.1"));
        assert_eq!(header("origin"), Some("https://grok.com"));
        assert_eq!(header("authorization"), Some("Bearer grok-test-bearer"));
    }

    /// 凭据解析：OIDC 优先于 legacy；残缺 OIDC 不遮蔽健康 legacy；
    /// 空 key 条目跳过。
    #[test]
    fn parses_credentials_priority() {
        let both = serde_json::json!({
            "https://accounts.x.ai/sign-in": {"key": "legacy-key"},
            "https://auth.x.ai::client-1": {"key": "oidc-key"}
        })
        .to_string();
        assert_eq!(parse_grok_token(&both).unwrap(), "oidc-key");

        let broken_oidc = serde_json::json!({
            "https://auth.x.ai::client-1": {"key": " "},
            "https://accounts.x.ai/sign-in": {"key": "legacy-key"}
        })
        .to_string();
        assert_eq!(parse_grok_token(&broken_oidc).unwrap(), "legacy-key");

        for bad in [
            "{}",
            r#"{"https://auth.x.ai::c": {}}"#,
            r#"{"other": {"key": "k"}}"#,
            "not json",
        ] {
            assert!(parse_grok_token(bad).is_err(), "{bad} 应解析失败");
        }
    }
}
