//! Transmission RPC —— 移植 `src/lib/downloaders.js:78-106`。
//!
//! `POST /transmission/rpc`；首次调用常返回 **409** 并在响应头带
//! `X-Transmission-Session-Id`（它的 CSRF 防护），取到该 id 后带上**重发一次**即可。
//! 可选 Basic Auth。

use reqwest::header::{HeaderName, CONTENT_TYPE};
use reqwest::{Response, StatusCode};
use serde_json::{json, Value};

use crate::{http, json_or_text, net_err, status_err, with_path, Config, PushOutcome, TIMEOUT};

/// 会话头。用 `from_static` 要求**全小写**（HeaderName 就是这么规范的）。
const SESSION_HEADER: &str = "x-transmission-session-id";

/// 发一次 RPC。409 → 取会话 id 重发**一次**；拿不到 id 就当错误（照 JS 的 rethrow）。
async fn call(cfg: &Config, body: &Value) -> Result<Value, String> {
    let url = with_path(&cfg.url, "/transmission/rpc");

    let resp = send(&url, cfg, body, None).await?;
    if resp.status() != StatusCode::CONFLICT {
        if !resp.status().is_success() {
            return Err(status_err(resp.status()));
        }
        return json_or_text(resp).await;
    }

    let sid = resp
        .headers()
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // 409 但没有会话头 → JS 是 `throw e`（axios 的通用文案），这里等价地报状态码。
    let Some(sid) = sid else {
        return Err(status_err(resp.status()));
    };

    let resp = send(&url, cfg, body, Some(&sid)).await?;
    if !resp.status().is_success() {
        return Err(status_err(resp.status()));
    }
    json_or_text(resp).await
}

/// 造并发出一次请求。`sid` 为 `Some` 时带上会话头（重发那一次）。
async fn send(
    url: &str,
    cfg: &Config,
    body: &Value,
    sid: Option<&str>,
) -> Result<Response, String> {
    let mut req = http()
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .timeout(TIMEOUT);

    if let Some(sid) = sid {
        req = req.header(HeaderName::from_static(SESSION_HEADER), sid);
    }

    // JS：`(cfg.user || cfg.pass) ? { username, password } : undefined`
    // —— 两者都空就**完全不带**认证，而不是带空的 Basic。
    if !cfg.user.is_empty() || !cfg.pass.is_empty() {
        req = req.basic_auth(&cfg.user, Some(&cfg.pass));
    }

    req.json(body).send().await.map_err(net_err)
}

/// 复刻 JS 的 `if (data && data.result && data.result !== 'success') throw`。
///
/// ⚠️ 这是**软检查**：`result` 缺失 / 空串 / 不是字符串，一律**放行**。
/// 于是「2xx + 非 JSON 正文」在 JS 里算**成功**（axios 把解析不了的正文留成字符串，
/// 字符串没有 `.result`）—— 这里用 [`json_or_text`] 把正文退化成 `Value::String`，
/// 于是同样放行。**别"顺手改成严格解析"**，那会和 JS 分叉。
fn check_result(data: &Value) -> Result<(), String> {
    if let Some(r) = data.get("result").and_then(|v| v.as_str()) {
        if !r.is_empty() && r != "success" {
            return Err(format!("Transmission: {r}"));
        }
    }
    Ok(())
}

pub(crate) async fn push(cfg: &Config, magnet: &str) -> Result<PushOutcome, String> {
    let data = call(
        cfg,
        &json!({ "method": "torrent-add", "arguments": { "filename": magnet } }),
    )
    .await?;
    check_result(&data)?;
    // JS 这里硬编码 200（不取真实状态码）。
    Ok(PushOutcome { status: 200 })
}

pub(crate) async fn test(cfg: &Config) -> Result<(), String> {
    let data = call(cfg, &json!({ "method": "session-get" })).await?;
    check_result(&data)
}
