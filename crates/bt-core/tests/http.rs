//! `src/lib/http.js` 的对照测试。
//!
//! **全部无外网**：测试自己起一个只服务一次请求的本地 HTTP 服务，
//! 这样成功、HTTP 错误、超时、连接失败四条路径都能在 CI 里稳定复现。

use std::time::Duration;

use bt_core::http::{pick_ua, HttpClient, ReqOpts, USER_AGENTS};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 起一个只服务一次请求的本地服务，返回 base url。
async fn oneshot(status: u16, body: &str, delay: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_string();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            read_head(&mut sock).await;
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(body.as_bytes()).await;
            let _ = sock.flush().await;
        }
    });
    format!("http://{addr}/")
}

/// 把收到的请求头原样回显到响应体里，用来断言请求真的带了哪些头。
async fn echo_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let req = read_head(&mut sock).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                req.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(req.as_bytes()).await;
            let _ = sock.flush().await;
        }
    });
    format!("http://{addr}/")
}

/// 读到请求头结束（`\r\n\r\n`）为止，避免响应发太早导致客户端 RST。
async fn read_head(sock: &mut tokio::net::TcpStream) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        match sock.read(&mut tmp).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

#[derive(serde::Deserialize, Debug, PartialEq)]
struct Demo {
    a: i32,
    b: Vec<String>,
}

// ---- get_text -------------------------------------------------------------

#[tokio::test]
async fn get_text_returns_body_and_status() {
    let url = oneshot(200, "hello 世界", Duration::ZERO).await;
    let r = HttpClient::new().get_text(&url, None).await;

    assert_eq!(r.html.as_deref(), Some("hello 世界"), "UTF-8 正文");
    assert_eq!(r.status, Some(200));
    assert_eq!(r.error, None);
    assert!(r.is_ok());
}

#[tokio::test]
async fn get_text_http_error_drops_status_like_js() {
    // JS 的 catch 分支把 status 硬编码成 null，即便 axios 的 e.response 有状态码。
    // 这里刻意保留同样行为，好让两边能逐字对照。
    let url = oneshot(404, "nope", Duration::ZERO).await;
    let r = HttpClient::new().get_text(&url, None).await;

    assert_eq!(r.html, None);
    assert_eq!(
        r.status, None,
        "error 路径上 status 必须是 None（与 JS 版一致）"
    );
    assert_eq!(
        r.error.as_deref(),
        Some("Request failed with status code 404"),
        "错误文案要对齐 axios 的 message"
    );
    assert!(!r.is_ok());
}

#[tokio::test]
async fn get_text_server_error_also_drops_body() {
    let url = oneshot(503, "unavailable", Duration::ZERO).await;
    let r = HttpClient::new().get_text(&url, None).await;

    assert_eq!(r.html, None, "5xx 也一样：不回正文");
    assert_eq!(
        r.error.as_deref(),
        Some("Request failed with status code 503")
    );
}

// ---- get_json / post_json -------------------------------------------------

#[tokio::test]
async fn get_json_parses_body() {
    let url = oneshot(200, r#"{"a":1,"b":["x","y"]}"#, Duration::ZERO).await;
    let r = HttpClient::new().get_json::<Demo>(&url, None).await;

    assert_eq!(
        r.data,
        Some(Demo {
            a: 1,
            b: vec!["x".to_string(), "y".to_string()]
        })
    );
    assert_eq!(r.status, Some(200));
    assert_eq!(r.error, None);
}

#[tokio::test]
async fn get_json_bad_shape_is_an_error() {
    // 能解析成 JSON，但不符合期望结构 —— 也必须报错，不能悄悄给半个对象
    let url = oneshot(200, r#"{"a":"not-a-number","b":[]}"#, Duration::ZERO).await;
    let r = HttpClient::new().get_json::<Demo>(&url, None).await;

    assert_eq!(r.data, None);
    assert!(
        r.error.as_deref().unwrap().starts_with("invalid json:"),
        "实际: {:?}",
        r.error
    );
}

#[tokio::test]
async fn get_json_non_json_body_is_an_error() {
    // 与 JS 版的**有意**差异：axios 会把原始字符串当 data 静默返回，
    // provider 直到下一行 `data.results.map` 才炸。这里提前报错。
    let url = oneshot(200, "<html>Cloudflare</html>", Duration::ZERO).await;
    let r = HttpClient::new().get_json::<Demo>(&url, None).await;

    assert_eq!(r.data, None);
    assert!(r.error.as_deref().unwrap().starts_with("invalid json:"));
}

#[tokio::test]
async fn post_json_sends_body_and_parses_response() {
    let url = oneshot(200, r#"{"a":7,"b":[]}"#, Duration::ZERO).await;
    let body = serde_json::json!({ "search_type": "score", "search_query": "ubuntu" });
    let r = HttpClient::new()
        .post_json::<Demo, _>(&url, &body, None)
        .await;

    assert_eq!(r.data, Some(Demo { a: 7, b: vec![] }));
    assert_eq!(r.error, None);
}

// ---- 失败路径 -------------------------------------------------------------

#[tokio::test]
async fn timeout_maps_to_axios_code() {
    let url = oneshot(200, "late", Duration::from_secs(2)).await;
    let client = HttpClient::with_timeout(Duration::from_millis(200));
    let r = client.get_text(&url, None).await;

    assert_eq!(r.error.as_deref(), Some("ECONNABORTED"));
    assert_eq!(r.html, None);
    assert_eq!(r.status, None);
}

#[tokio::test]
async fn connection_refused_is_reported_not_panicked() {
    // 占一个端口再放掉，得到一个确定没人监听的地址
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let r = HttpClient::new()
        .get_text(&format!("http://{addr}/"), None)
        .await;

    assert_eq!(r.error.as_deref(), Some("ECONNREFUSED"));
    assert_eq!(r.html, None);
}

// ---- 请求头 ---------------------------------------------------------------

#[tokio::test]
async fn default_request_carries_pool_ua_and_accept_headers() {
    let url = echo_server().await;
    let r = HttpClient::new().get_text(&url, None).await;
    let req = r.html.expect("echo 应当回显请求");

    assert!(
        USER_AGENTS.iter().any(|ua| req.contains(ua)),
        "应当带上池子里的某个 UA:\n{req}"
    );
    assert!(req.contains("accept-language: en-US,en;q=0.9"), "{req}");
    assert!(req.contains("accept: text/html"), "{req}");
}

#[tokio::test]
async fn opts_header_overrides_pool_ua() {
    // JS: `headers: { 'User-Agent': pickUA(), ...extraHeaders }` —— extra 胜出
    let url = echo_server().await;
    let opts = ReqOpts::default()
        .header("User-Agent", "TEST-UA/1.0")
        .header("X-Test", "yes");
    let r = HttpClient::new().get_text(&url, Some(&opts)).await;
    let req = r.html.expect("echo 应当回显请求");

    assert!(
        req.to_lowercase().contains("user-agent: test-ua/1.0"),
        "{req}"
    );
    assert!(req.to_lowercase().contains("x-test: yes"), "{req}");
    assert!(
        !USER_AGENTS.iter().any(|ua| req.contains(ua)),
        "池里的 UA 必须被覆盖掉:\n{req}"
    );
}

// ---- UA 池 ----------------------------------------------------------------

#[test]
fn pick_ua_cycles_through_the_whole_pool() {
    let seen: Vec<&str> = (0..USER_AGENTS.len()).map(|_| pick_ua()).collect();
    for ua in &seen {
        assert!(USER_AGENTS.contains(ua), "取了池外的 UA: {ua}");
    }
    let mut uniq = seen.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert_eq!(
        uniq.len(),
        USER_AGENTS.len(),
        "轮询下连着取 N 个应当互不相同（JS 是随机；这里改轮询是**有意**的，为了可断言）"
    );
}
