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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_string();
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
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = sock.write_all(head.as_bytes()).await;
        let _ = sock.write_all(body.as_bytes()).await;
        let _ = sock.flush().await;
    });

    (format!("http://{addr}/"), seen_out)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
