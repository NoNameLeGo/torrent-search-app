//! Gopeed REST（默认端口 9999）—— 移植 `src/lib/downloaders.js:140-162`。
//!
//! `POST /api/v1/tasks`，body `{ req: { url: <magnet> } }`。
//! 设了 API Token 就用 `X-Api-Token` 头；设了 Basic Auth（config 里的
//! username/password）就走 auth —— 二者按用户所填**择一**（可以同时带，照抄 JS）。

use reqwest::header::CONTENT_TYPE;
use reqwest::RequestBuilder;
use serde_json::json;

use crate::{http, json_or_text, net_err, status_err, with_path, Config, PushOutcome, TIMEOUT};

const TOKEN_HEADER: &str = "X-Api-Token";

/// 造 `/api/v1/tasks` 的请求。
///
/// ⚠️ `post = false`（`test` 那次）**不带 `Content-Type`** —— 照抄 JS，
/// 那边 `test` 里写的是 `const headers = {}`，只加 token。别"顺手补上"。
fn tasks_request(cfg: &Config, url: &str, post: bool) -> RequestBuilder {
    let mut req = if post {
        http().post(url).header(CONTENT_TYPE, "application/json")
    } else {
        http().get(url)
    };

    if !cfg.token.is_empty() {
        req = req.header(TOKEN_HEADER, &cfg.token);
    }
    // JS：`(cfg.user || cfg.pass) ? { username, password } : undefined`
    if !cfg.user.is_empty() || !cfg.pass.is_empty() {
        req = req.basic_auth(&cfg.user, Some(&cfg.pass));
    }

    req.timeout(TIMEOUT)
}

pub(crate) async fn push(cfg: &Config, magnet: &str) -> Result<PushOutcome, String> {
    let url = with_path(&cfg.url, "/api/v1/tasks");

    let resp = tasks_request(cfg, &url, true)
        .json(&json!({ "req": { "url": magnet } }))
        .send()
        .await
        .map_err(net_err)?;

    if !resp.status().is_success() {
        return Err(status_err(resp.status()));
    }
    let status = resp.status().as_u16();

    // Gopeed 返回 `{ code, msg, data }`，`code !== 0` 视为失败。
    let data = json_or_text(resp).await?;

    // JS: `data && typeof data.code === 'number' && data.code !== 0`
    // —— `typeof` 是"是不是数字"，所以非数字的 code（比如字符串 "1"）**不算失败**。
    // 用 `as_f64()` 对齐这一点：字符串 code 拿不到数值 → 放行。
    if let Some(code) = data.get("code").and_then(|c| c.as_f64()) {
        if code != 0.0 {
            // JS: `msg || ('code ' + code)`
            let msg = data
                .get("msg")
                .and_then(|m| m.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("code {code}"));
            return Err(format!("Gopeed: {msg}"));
        }
    }

    Ok(PushOutcome { status })
}

/// 列任务作轻量疎通：能拿到 2xx 即认为可达 + 认证通过，**不产生下载副作用**。
pub(crate) async fn test(cfg: &Config) -> Result<(), String> {
    let url = with_path(&cfg.url, "/api/v1/tasks");

    let resp = tasks_request(cfg, &url, false)
        .send()
        .await
        .map_err(net_err)?;

    if !resp.status().is_success() {
        return Err(status_err(resp.status()));
    }
    Ok(())
}
