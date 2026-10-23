//! 测试共用的本地一次性 HTTP 服务。
//!
//! provider 把端点指向它，于是测试能在 **CI 里完全不碰外网** 地跑完
//! 「解析 fixture / 空结果 / HTTP 错误 / 非 JSON 正文」等路径。
//! 另有 [`fixture`] —— 读 `test/fixtures/` 里那份真实快照。
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 读一份 fixture（`test/fixtures/<name>`，相对 crate 根往上两级）。
///
/// 原来 9 个 provider 测试文件里各有一份逐字节相同的拷贝（6 行 × 9），
/// 2026-10-10 收到这里。
///
/// fixture **永久保留在仓库里**，不随「边搬边删」删掉 —— 它是 golden 数据源。
/// 注意读的是**原始字符串**，要不要 `serde_json::from_str` 由调用方决定。
pub fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 fixture {}: {e}", p.display()))
}

/// 读一份 fixture 的**原始字节**。
///
/// 给非 UTF-8 站点用（`megapeer` 是 windows-1251）：[`fixture`] 走 `read_to_string`，
/// 碰到非法 UTF-8 直接失败；这类 fixture 必须原样取字节，再让「该解码的人」去解码 ——
/// 测试里把它塞进 [`oneshot_bytes`] 并带 `charset=…` 响应头，
/// reqwest 就会按那个编码解 —— 与真实站点同一条路径。
pub fn fixture_bytes(name: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("读不到 fixture {}: {e}", p.display()))
}

/// 起一个只服务一次请求的服务，响应体固定为 `body`；返回可直接当端点用的 base url。
///
/// 需要检查**请求长什么样**（比如 POST 的 JSON body）时用 [`oneshot_capture`]。
pub async fn oneshot(status: u16, body: &str) -> String {
    oneshot_capture(status, body).await.0
}

/// 同 [`oneshot`]，另外把收到的**原始请求文本**（头 + body）通过第二个返回值交出来。
///
/// 用途：钉住请求契约 —— 比如 knaben 的 POST body 必须和 JS 版逐字段一致，
/// 服务端只认那些 snake_case 名字，写错了在真实环境才炸。
pub async fn oneshot_capture(status: u16, body: &str) -> (String, Arc<Mutex<String>>) {
    oneshot_capture_bytes(status, "application/json", body.as_bytes()).await
}

/// 同 [`oneshot`]，但 **Content-Type 可指定、body 是原始字节**。
///
/// `megapeer`（windows-1251）测试要用：真实站点响应头带 `charset=windows-1251`，
/// `reqwest` 的 `res.text()` 按它解码 —— 这里用同样的头，把解码路径也测进去。
pub async fn oneshot_bytes(status: u16, content_type: &str, body: &[u8]) -> String {
    oneshot_capture_bytes(status, content_type, body).await.0
}

/// [`oneshot_capture`] 的字节版：Content-Type 可指定，另外交出原始请求文本。
pub async fn oneshot_capture_bytes(
    status: u16,
    content_type: &str,
    body: &[u8],
) -> (String, Arc<Mutex<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_vec();
    // ⚠️ `tokio::spawn` 的任务要求 'static —— body / content_type 都要先变成
    // 拥有所有权的值，否则借用会「逃出函数」（E0521）。
    let content_type = content_type.to_string();
    let seen: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let seen_out = Arc::clone(&seen);

    tokio::spawn(async move {
        let Ok((mut sock, _)) = listener.accept().await else {
            return;
        };

        // 读到「没有新数据」为止：POST 的头和 body 是分两次到达的，只 read 一次会截断。
        let mut raw: Vec<u8> = Vec::new();
        let mut buf = [0u8; 2048];
        loop {
            match tokio::time::timeout(Duration::from_millis(150), sock.read(&mut buf)).await {
                Ok(Ok(0)) | Err(_) => break,
                Ok(Ok(n)) => {
                    raw.extend_from_slice(&buf[..n]);
                    // 头 + 声明长度的 body 都到齐就收工
                    if let Some(head_end) = find_subslice(&raw, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&raw[..head_end]).to_lowercase();
                        let want = head
                            .split("content-length:")
                            .nth(1)
                            .and_then(|s| {
                                s.split(|c: char| c == '\r' || c == '\n' || c == ' ')
                                    .find(|t| !t.is_empty())
                            })
                            .and_then(|s| s.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if raw.len() >= head_end + 4 + want {
                            break;
                        }
                    }
                }
                Ok(Err(_)) => break,
            }
        }
        *seen.lock().unwrap() = String::from_utf8_lossy(&raw).into_owned();

        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = sock.write_all(head.as_bytes()).await;
        let _ = sock.write_all(&body).await;
        let _ = sock.flush().await;
    });

    (format!("http://{addr}/"), seen_out)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 只回一个 **302 + `Location`** 的服务 —— 专测「不跟随跳转、自己读 Location」。
///
/// 场景：`anirena` 的磁力链接是个 302 到 `magnet:` 的地址（对应 JS 的
/// `maxRedirects: 0` + 读 `res.headers.location`）。reqwest 默认会跟过去，
/// 然后因为 `magnet:` 不是 http 协议而报错 —— 所以这个桩必须**不跟**才对得上。
pub async fn oneshot_redirect(location: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let location = location.to_string();

    tokio::spawn(async move {
        let Ok((mut sock, _)) = listener.accept().await else {
            return;
        };
        let mut buf = [0u8; 2048];
        let _ = sock.read(&mut buf).await;
        let resp = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = sock.write_all(resp.as_bytes()).await;
    });

    format!("http://{addr}")
}
