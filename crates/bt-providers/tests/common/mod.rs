//! 测试共用的本地一次性 HTTP 服务。
//!
//! provider 把端点指向它，于是测试能在 **CI 里完全不碰外网** 地跑完
//! 「解析 fixture / 空结果 / HTTP 错误 / 非 JSON 正文」四条路径。
#![allow(dead_code)]

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 起一个只服务一次请求的服务，响应体固定为 `body`；返回可直接当端点用的 base url。
pub async fn oneshot(status: u16, body: &str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_string();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            // 先把请求读掉，避免响应发太早导致客户端 RST
            let mut tmp = [0u8; 1024];
            let _ = sock.read(&mut tmp).await;
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
