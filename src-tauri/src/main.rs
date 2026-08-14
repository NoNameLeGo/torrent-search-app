#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

// CREATE_NO_WINDOW：避免给 server.exe（node）弹出控制台黑窗口。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct SidecarProc(Mutex<Option<std::process::Child>>);

// app 退出时杀掉 server 子进程（std::process::Child 的 drop 不 kill，
// 与原来 tauri-plugin-shell CommandChild 的行为对齐，避免残留孤儿进程）。
impl Drop for SidecarProc {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("failed to bind a free port");
    listener.local_addr().unwrap().port()
}

fn wait_for_health(port: u16) -> bool {
    for _ in 0..120 {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            // 加读写超时，避免后端无响应时 read 永久阻塞主线程。
            let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
            let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
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

// 把诊断 HTML 渲染成 data: URL，避免在无后端时弹出裸 127.0.0.1 连接错误。
fn data_url_of_html(html: &str) -> tauri::Url {
    let mut out = String::with_capacity(html.len() * 3);
    for b in html.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    let raw = format!("data:text/html;charset=utf-8,{}", out);
    raw.parse().unwrap_or_else(|_| {
        "data:text/html,%E5%90%8E%E7%AB%AF%E5%90%AF%E5%8A%A8%E5%A4%B1%E8%B4%A5"
            .parse()
            .unwrap()
    })
}

// 渲染错误页 HTML（不依赖后端），主窗口直接导航过去呈现给用户。
fn error_html(title: &str, body: &str) -> String {
    let safe_title = title.replace('<', "&lt;").replace('>', "&gt;");
    let safe_body = body.replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset='utf-8'><title>{safe_title}</title></head>\
         <body style='margin:0;background:#0f1115;color:#e6e6e6;font-family:system-ui,Segoe UI,sans-serif;padding:28px'>\
         <h2 style='color:#ff6b6b;margin-top:0'>{safe_title}</h2>\
         <p style='color:#9aa0a6'>BT 聚合搜索 · 后端未能正常启动，应用无法加载。</p>\
         <pre style='white-space:pre-wrap;word-break:break-word;background:#161a20;border:1px solid #2a2f37;border-radius:8px;padding:16px;font-size:13px;line-height:1.5'>{safe_body}</pre>\
         </body></html>"
    )
}

// Windows 上 Tauri 的 resource_dir() 返回带 \\?\ 前缀的 verbatim 路径。
// node 的 CJS 加载器无法处理这种路径（path.resolve 会把 \\?\D:\… 解析成
// 裸盘符 D:，lstat 时抛 EISDIR 直接崩溃），传给 server 前必须剥掉前缀。
#[cfg(windows)]
fn plain_path(p: &std::path::Path) -> std::path::PathBuf {
    p.to_string_lossy()
        .strip_prefix(r"\\?\")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(not(windows))]
fn plain_path(p: &std::path::Path) -> std::path::PathBuf {
    p.to_path_buf()
}

// 检查随安装包发布的资源是否齐全；返回缺失项（空 = 齐全）。
fn missing_resources(resource_dir: &std::path::Path) -> Vec<String> {
    let mut missing = Vec::new();
    for rel in ["server.js", "public", "src", "node_modules/express"] {
        if !resource_dir.join(rel).exists() {
            missing.push(rel.to_string());
        }
    }
    missing
}

// 创建主窗口，并拦截 Magnet 链接交给系统默认下载工具处理
// （WebView2 不会自动调起 magnet:，等价于 Electron 的 shell.openExternal）
fn create_main_window(app: &tauri::AppHandle, url: WebviewUrl) -> WebviewWindow {
    WebviewWindowBuilder::new(app, "main", url)
        .title("BT 聚合搜索")
        .inner_size(1280.0, 800.0)
        .min_inner_size(900.0, 600.0)
        .background_color(tauri::webview::Color(15, 17, 21, 255))
        .on_navigation(|url| {
            if url.scheme() == "magnet" {
                // 用系统默认程序（如 qBittorrent）打开磁力链接，不在 webview 内导航
                let _ = tauri_plugin_opener::open_url(url.to_string(), None::<&str>);
                false
            } else {
                true
            }
        })
        .build()
        .expect("failed to build main window")
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 第二实例启动 → 聚焦已存在的窗口（避免重复开后端）
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(SidecarProc(Mutex::new(None)))
        .setup(|app| {
            // 开发模式：sidecar 由 beforeDevCommand 拉起，固定监听 3000
            #[cfg(debug_assertions)]
            {
                let _main = create_main_window(
                    app.handle(),
                    WebviewUrl::External("http://127.0.0.1:3000/".parse().unwrap()),
                );
            }

            // 发布模式：立即建主窗口显示加载页，拉起 sidecar（随机端口）后就绪，
            // 成功则导航到真实地址，失败则导航到错误页——避免「无窗口常驻进程」
            // （窗口建立失败或被静默吞掉时用户什么都看不到，只剩进程占着）。
            #[cfg(not(debug_assertions))]
            {
                let resource_dir = plain_path(
                    &app
                        .path()
                        .resource_dir()
                        .expect("failed to resolve resource dir"),
                );

                let main_window = create_main_window(
                    app.handle(),
                    WebviewUrl::External(data_url_of_html(
                        "<!doctype html><html><head><meta charset='utf-8'></head>\
                         <body style='margin:0;background:#0f1115;color:#e6e6e6;font-family:system-ui,Segoe UI,sans-serif;display:flex;align-items:center;justify-content:center;height:100vh'>\
                         <p style='color:#9aa0a6'>BT 聚合搜索 · 正在启动后端…</p></body></html>",
                    )),
                );

                // 资源自查：node_modules 等若没打进安装包，sidecar 一启动就会
                // MODULE_NOT_FOUND 退出，而窗口却仍指向一个没人监听的 127.0.0.1
                // 端口，表现为「127.0.0.1 错误」。先拦下来，把缺失项直接告诉用户。
                let missing = missing_resources(&resource_dir);
                if !missing.is_empty() {
                    let _ = main_window.navigate(data_url_of_html(&error_html(
                        "安装包资源缺失",
                        &format!(
                            "以下资源未随安装包提供，后端无法启动：\n\n{}\n\n\
                             通常是构建/打包阶段未把 node_modules 打进 NSIS 资源目录。\n\
                             请重新构建（beforeBuildCommand 现已包含资源校验）。",
                            missing.join("\n")
                        ),
                    )));
                    return Ok(());
                }

                let port = free_port();
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

                let server_exe = resource_dir.join("server.exe");
                let mut cmd = Command::new(&server_exe);
                cmd.arg(&server_arg)
                    .arg("--port")
                    .arg(port.to_string())
                    .arg("--public-dir")
                    .arg(&public_arg)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .stdin(Stdio::null());

                #[cfg(windows)]
                cmd.creation_flags(CREATE_NO_WINDOW);

                // 用 std::process::Command 直接 spawn，而不是 tauri-plugin-shell 的
                // sidecar（后者在 setup 阶段走 tokio async_runtime 读管道，会导致
                // server 进程起不来、主线程卡死）。
                let mut child = cmd.spawn().expect("failed to spawn server process");

                // 收集 server 的 stdout/stderr，健康检查失败时直接展示给用户，
                // 而不是弹出一个裸的 127.0.0.1 连接错误。
                let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
                if let Some(stdout) = child.stdout.take() {
                    let log2 = log.clone();
                    std::thread::spawn(move || {
                        for line in BufReader::new(stdout).lines() {
                            if let Ok(l) = line {
                                let mut g = log2.lock().unwrap();
                                g.push_str(&l);
                                g.push('\n');
                            }
                        }
                    });
                }
                if let Some(stderr) = child.stderr.take() {
                    let log2 = log.clone();
                    std::thread::spawn(move || {
                        for line in BufReader::new(stderr).lines() {
                            if let Ok(l) = line {
                                let mut g = log2.lock().unwrap();
                                g.push_str(&l);
                                g.push('\n');
                            }
                        }
                    });
                }

                // Keep the child alive for the app lifetime (dropping would kill it).
                if let Some(state) = app.try_state::<SidecarProc>() {
                    *state.0.lock().unwrap() = Some(child);
                }

                if !wait_for_health(port) {
                    let captured = log.lock().unwrap().clone();
                    let body = if captured.trim().is_empty() {
                        "后端进程在没有任何输出的情况下退出（可能是 sidecar 二进制缺失，\
                         或 node 运行时启动失败）。"
                            .to_string()
                    } else {
                        format!(
                            "后端在约 18 秒内未通过健康检查（/api/health）。\n\
                             以下为后端进程输出，通常是定位 127.0.0.1 连接失败的关键：\n\n{}",
                            captured
                        )
                    };
                    let _ = main_window.navigate(data_url_of_html(&error_html("后端启动失败", &body)));
                    return Ok(());
                }

                let _ = main_window.navigate(
                    format!("http://127.0.0.1:{port}/").parse().expect("invalid sidecar url"),
                );
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
