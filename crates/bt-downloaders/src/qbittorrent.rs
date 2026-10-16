//! qBittorrent WebUI (api/v2) —— 移植 `src/lib/downloaders.js:32-71`。
//!
//! 先 `/api/v2/auth/login` 拿 session cookie，再带着它调 `/torrents/add`。

use std::time::Duration;

use reqwest::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use reqwest::StatusCode;

use crate::{
    encode_uri_component, header_values, http, net_err, status_err, strip_trailing_slashes, Config,
    PushOutcome, TIMEOUT,
};

/// JS 里 qB 的登录超时是**单独一个更短的值**（6000ms），不是公共的 10000。
const LOGIN_TIMEOUT: Duration = Duration::from_secs(6);

/// 登录失败的文案 —— 与 JS **逐字一致**（含"响应里没有 SID cookie"这一种）。
const LOGIN_FAILED: &str = "登录失败（用户名或密码错误）";

/// 只登录，不做任何下载动作。返回用于 `Cookie:` 头的那个 set-cookie 条目。
async fn login(base: &str, user: &str, pass: &str) -> Result<String, String> {
    let url = format!("{base}/api/v2/auth/login");
    let body = format!(
        "username={}&password={}",
        encode_uri_component(user),
        encode_uri_component(pass)
    );

    let resp = http()
        .post(&url)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .timeout(LOGIN_TIMEOUT)
        .body(body)
        .send()
        .await
        .map_err(net_err)?;

    // JS：只有 **403** 被当成"用户名或密码错误"；其它非 2xx 走 axios 的通用抛。
    // （新版 qB 凭证错就是回 403。）
    if resp.status() == StatusCode::FORBIDDEN {
        return Err(LOGIN_FAILED.to_string());
    }
    if !resp.status().is_success() {
        return Err(status_err(resp.status()));
    }

    // ⚠️⚠️ **判定登录成功的依据是响应头里的 SID cookie，不是响应体字符串。**
    //
    // 旧实现拿响应体去 string-match `/fails|failed/i` —— 跨版本很脆，别退回去。
    // 另一半同样重要：axios 会把多条 set-cookie 塞成数组，**旧实现只看第一个**，
    // 于是"第一条不是 SID"时就误判登录失败。这里遍历**全部**。
    let cookie = header_values(resp.headers(), SET_COOKIE.as_str())
        .into_iter()
        .find(|c| c.to_lowercase().contains("sid="));

    // 注意：这里把**整个 set-cookie 条目**（含 `; path=/` 等属性）原样当作
    // `Cookie:` 头发出去 —— 照抄 JS（`cookie` 就是那条完整字符串）。
    // 不够规范，但服务端都容忍，且"照抄"优先于"顺手修正"。
    cookie.ok_or_else(|| LOGIN_FAILED.to_string())
}

pub(crate) async fn push(cfg: &Config, magnet: &str) -> Result<PushOutcome, String> {
    let base = strip_trailing_slashes(&cfg.url);
    let cookie = login(&base, &cfg.user, &cfg.pass).await?;

    let resp = http()
        .post(format!("{base}/api/v2/torrents/add"))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(COOKIE, cookie)
        .timeout(TIMEOUT)
        .body(format!("urls={}", encode_uri_component(magnet)))
        .send()
        .await
        .map_err(net_err)?;

    // axios 对非 2xx 会抛；reqwest 不会 —— 所以这里必须自己查。
    let status = resp.status();
    if !status.is_success() {
        return Err(status_err(status));
    }

    // ⚠️ **必须读正文**：qB 拒绝一个种子时回的是 **HTTP 200 + 正文 `Fails.`**
    //（不是错误码 —— 它 200/404 两套都会用）。只看状态码会把"什么都没加"
    // 报成「已推送到 qbittorrent（HTTP 200）」并标 Done，最典型的触发是**同一个磁力推两次**。
    let body = resp.text().await.map_err(net_err)?;
    if body.trim().eq_ignore_ascii_case("fails.") {
        return Err("qBittorrent 拒绝了这个种子（Fails.）—— 多半是它已经在列表里了".to_string());
    }

    Ok(PushOutcome {
        status: status.as_u16(),
    })
}

/// 疎通確認：**仅登录**，不产生任何下载副作用（JS 注释原话）。
pub(crate) async fn test(cfg: &Config) -> Result<(), String> {
    let base = strip_trailing_slashes(&cfg.url);
    login(&base, &cfg.user, &cfg.pass).await.map(|_| ())
}
