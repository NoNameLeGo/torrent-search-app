#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};

const SIDECAR: &str = "server";

struct SidecarProc(Mutex<Option<CommandChild>>);

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("failed to bind a free port");
    listener.local_addr().unwrap().port()
}

fn wait_for_health(port: u16) -> bool {
    for _ in 0..120 {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = s.write_all(
                b"GET /api/health HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            );
            let mut buf = [0u8; 512];
            if let Ok(n) = s.read(&mut buf) {
                let text = String::from_utf8_lossy(&buf[..n]);
                if text.contains("\"ok\"") {
                    return true;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(SidecarProc(Mutex::new(None)))
        .setup(|app| {
            // 开发模式：sidecar 由 beforeDevCommand 拉起，固定监听 3000
            #[cfg(debug_assertions)]
            {
                let _main = WebviewWindowBuilder::new(
                    app,
                    "main",
                    WebviewUrl::External("http://127.0.0.1:3000/".parse().unwrap()),
                )
                .title("BT 聚合搜索")
                .inner_size(1280.0, 800.0)
                .min_inner_size(900.0, 600.0)
                .background_color(tauri::webview::Color(15, 17, 21, 255))
                .build()
                .expect("failed to build main window (dev)");
            }

            // 发布模式：在 setup 里拉起 sidecar（随机端口），就绪后创建窗口
            #[cfg(not(debug_assertions))]
            {
                let port = free_port();
                let resource_dir = app
                    .path()
                    .resource_dir()
                    .expect("failed to resolve resource dir");
                let public_dir = resource_dir.join("public");
                let public_arg = public_dir
                    .to_str()
                    .expect("public dir path is not valid UTF-8")
                    .to_string();
                let server_js = resource_dir.join("server.js");
                let server_arg = server_js
                    .to_str()
                    .expect("server.js path is not valid UTF-8")
                    .to_string();

                let (mut rx, child) = app
                    .shell()
                    .sidecar(SIDECAR)
                    .expect("sidecar 'server' is not configured in tauri.conf.json (bundle.externalBin)")
                    .args([
                        server_arg,
                        "--port".to_string(),
                        port.to_string(),
                        "--public-dir".to_string(),
                        public_arg,
                    ])
                    .spawn()
                    .expect("failed to spawn sidecar server process");

                // Keep the child alive for the app lifetime (dropping would kill it).
                if let Some(state) = app.try_state::<SidecarProc>() {
                    *state.0.lock().unwrap() = Some(child);
                }

                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        match event {
                            CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes) => {
                                let line = String::from_utf8_lossy(&bytes);
                                let _ = handle.emit("server-log", line.to_string());
                            }
                            _ => {}
                        }
                    }
                });

                if !wait_for_health(port) {
                    eprintln!("sidecar server did not become healthy in time");
                }

                // 用动态端口直接创建窗口（Tauri v2 的 WebviewWindow 没有 load()）
                let _main = WebviewWindowBuilder::new(
                    app,
                    "main",
                    WebviewUrl::External(
                        format!("http://127.0.0.1:{port}/").parse().expect("invalid sidecar url"),
                    ),
                )
                .title("BT 聚合搜索")
                .inner_size(1280.0, 800.0)
                .min_inner_size(900.0, 600.0)
                .background_color(tauri::webview::Color(15, 17, 21, 255))
                .build()
                .expect("failed to build main window (release)");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
