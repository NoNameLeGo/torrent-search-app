//! 测试共用的**脚本化**本地 HTTP 服务。
//!
//! `bt-providers` 那份 `oneshot_*` 只服务**一次**请求，这里不够用 ——
//! 下载器的流程普遍是多跳的：
//!
//! - qB：`/auth/login`（拿 SID）→ `/torrents/add`
//! - Transmission：第一次 409 + 会话头 → 带头发第二次
//! - 探测：第一个候选失败、第二个成功
//!
//! 所以这里按**脚本**依次响应：第 i 个请求回 `script[i]`，并把**每一个**收到的
//! 原始请求文本记下来（用来钉请求长什么样 —— 比如 token 有没有平铺进 params、
//! 重发那次有没有带上会话头）。
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 一条脚本化响应。
#[derive(Debug, Clone)]
pub struct Resp {
    pub status: u16,
    /// 允许重复的头名 —— qB 的 `set-cookie` 就靠这个发多条。
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn new(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }
}

/// 起一个按脚本依次响应的小服务。
///
/// 返回 `(base_url, 收到的原始请求列表)`。base_url **不带结尾斜杠**
/// （与用户填 `http://localhost:8080` 的形状一致）。
pub async fn scripted(script: Vec<Resp>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_out = Arc::clone(&seen);

    tokio::spawn(async move {
        for resp in script {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let raw = read_request(&mut sock).await;
            seen.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&raw).into_owned());

            let mut head = format!("HTTP/1.1 {} X\r\n", resp.status);
            for (k, v) in &resp.headers {
                head.push_str(&format!("{k}: {v}\r\n"));
            }
            head.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n",
                resp.body.len()
            ));

            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(&resp.body).await;
            let _ = sock.flush().await;
        }
    });

    (format!("http://{addr}"), seen_out)
}

/// 起一个**永远连不上**的地址：绑一个端口再立刻释放，拿它的地址。
///
/// 用来测「连不上」那条路径（而不是去赌某个固定端口没人监听）。
pub async fn dead_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}")
}

/// 读完整一个请求（头 + 声明长度的 body）。
///
/// POST 的头和 body 是可能分两次到的，只 read 一次会截断 —— 这与
/// `bt-providers/tests/common` 里踩过的坑同源。
async fn read_request(sock: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut raw: Vec<u8> = Vec::new();
    let mut buf = [0u8; 2048];
    loop {
        match tokio::time::timeout(Duration::from_millis(200), sock.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(n)) => {
                raw.extend_from_slice(&buf[..n]);
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
    raw
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 从记录的原始请求里取出 body 部分（`\r\n\r\n` 之后）。
pub fn body_of(raw: &str) -> &str {
    match raw.find("\r\n\r\n") {
        Some(i) => &raw[i + 4..],
        None => "",
    }
}

/// 记录里第 `n` 个请求（0 基）的原始文本。
pub fn nth(seen: &Arc<Mutex<Vec<String>>>, n: usize) -> String {
    seen.lock().unwrap()[n].clone()
}
