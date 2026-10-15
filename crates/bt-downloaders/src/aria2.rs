//! aria2 / Motrix JSON-RPC —— 移植 `src/lib/downloaders.js:113-133`。
//!
//! Motrix 底层就是 aria2，接口一致，只有默认端口不同（Motrix 16800 / aria2 6800）。
//! `POST /jsonrpc`；若设了 `rpc-secret`，需作为**首个参数** `token:<secret>` 传入。

use reqwest::header::CONTENT_TYPE;
use serde_json::{json, Value};

use crate::{
    http, js_truthy, json_or_text, net_err, status_err, with_path, Config, PushOutcome, TIMEOUT,
};

/// 发一次 JSON-RPC 并取出 `result`。
///
/// 请求体照 JS：`{ jsonrpc:'2.0', id:'ts', method, params }`，
/// 其中 `params = [...secret, ...业务参数]` —— **token 是平铺进数组的**，不是嵌套的。
async fn call(cfg: &Config, method: &str, params: Vec<Value>) -> Result<Value, String> {
    let url = with_path(&cfg.url, "/jsonrpc");

    let mut all: Vec<Value> = Vec::new();
    if !cfg.token.is_empty() {
        all.push(Value::String(format!("token:{}", cfg.token)));
    }
    all.extend(params);

    let resp = http()
        .post(&url)
        .header(CONTENT_TYPE, "application/json")
        .timeout(TIMEOUT)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "ts",
            "method": method,
            "params": all,
        }))
        .send()
        .await
        .map_err(net_err)?;

    if !resp.status().is_success() {
        return Err(status_err(resp.status()));
    }

    let data = json_or_text(resp).await?;

    // JS: `if (r.data && r.data.error)` —— 是**真值**判断，不是"存在即错误"：
    // `error: 0` / `error: ""` / `error: false` 在 JS 里都是假 → 不算错。
    if let Some(err) = data.get("error") {
        if js_truthy(err) {
            // JS: `error.message || JSON.stringify(error)`
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(err).unwrap_or_default());
            return Err(format!("aria2: {msg}"));
        }
    }

    Ok(data.get("result").cloned().unwrap_or(Value::Null))
}

pub(crate) async fn push(cfg: &Config, magnet: &str) -> Result<PushOutcome, String> {
    // ⚠️ **双层数组**：`aria2.addUri` 的**第一个参数本身是「URL 数组」**，
    // 而 JSON-RPC 的 `params` 也是数组 —— JS 写的就是 `[[magnet]]`。
    // 少一层就变成"把字符串当 URL 列表"，aria2 会报参数错。
    call(cfg, "aria2.addUri", vec![json!([magnet])]).await?;
    // JS 这里硬编码 200。
    Ok(PushOutcome { status: 200 })
}

pub(crate) async fn test(cfg: &Config) -> Result<(), String> {
    call(cfg, "aria2.getVersion", Vec::new()).await.map(|_| ())
}
