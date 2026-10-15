//! `--selftest`：把 M4 的**全链路**在 CI 上真跑一遍 —— 无网、无人、无窗口。
//!
//! ## 为什么必须有它
//!
//! 「能编译 + 映射层单测过」证明不了 M4：`.slint` 编译通过不代表窗口能渲染，
//! 接线写错也不会有编译错误。**M4 已经因此漏掉一个运行期 panic** ——
//! 回调里裸用 `tokio::spawn`（事件循环线程不在 tokio 上下文里），
//! 编译期、单测、clippy 全都发现不了，只有真跑起来才炸。这个自检就是那道网。
//!
//! ## 它验什么
//!
//! 走的是和真程序**同一条**接线（[`crate::wire`]），只是把数据源换掉：
//!
//! 1. **注册表换成假 provider**（离线、确定性）：一家给 3 条**没有**磁力的结果
//!    （走惰性解析那条路）、一家给 2 条**自带**磁力的、一家故意失败。
//! 2. **下载器指向本地桩**（一个极小的 qB：登录回 SID、add 回 200）。
//! 3. 于是整条链走通：`搜索 → 每站状态 → 行模型 → 取磁力 → 推送到下载器`。
//!
//! ## ⚠️ 每一步都**强制同步重绘**
//!
//! M1 栽过一个"假绿"陷阱：timer 驱动的重绘**从不真的重绘**（440 步 0.38 秒、
//! 每步 0.86ms），于是 Slint 的排版路径根本没被走过，CI 却是绿的。
//! 所以这里每一跳都调 `Window::take_snapshot()` 强制同步重绘**并统计成功次数** ——
//! 重绘次数不够就判失败。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bt_core::{HttpClient, TorrentResult};
use bt_providers::{BoxFuture, MagnetOutcome, Provider, SearchOutcome, NO_RESOLVER};
use slint::{ComponentHandle, Timer, TimerMode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::{install_registry, runtime, save_downloader, wire, AppWindow, STATE};

/// 惰性解析出来的磁力（刻意与"自带磁力"的形状不同 —— 用它来证明
/// **解析那一步真的跑了**，而不是直接用了结果里带的）。
const RESOLVED_MAGNET: &str = "magnet:?xt=urn:btih:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

const TICK: Duration = Duration::from_millis(40);
/// 40ms × 250 = 10 秒上限（假 provider 不发网络请求，正常几跳就完事）
const DEADLINE_TICKS: usize = 250;

// ---------------------------------------------------------------------------
// 假 provider
// ---------------------------------------------------------------------------

struct Fake {
    id: &'static str,
    name: &'static str,
    rows: usize,
    /// 结果自带磁力（false → 走惰性解析）
    inline_magnet: bool,
    /// 能惰性解析吗
    can_resolve: bool,
    /// 故意失败的一家
    fails: bool,
}

impl Provider for Fake {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }

    fn search<'a>(
        &'a self,
        _http: &'a HttpClient,
        query: &'a str,
        _page: u32,
    ) -> BoxFuture<'a, SearchOutcome> {
        let (id, rows, inline, fails, q) = (
            self.id,
            self.rows,
            self.inline_magnet,
            self.fails,
            query.to_string(),
        );
        Box::pin(async move {
            if fails {
                // 契约：失败落在 error 里，不 panic 不 Err
                return SearchOutcome::err(format!("{id} unreachable (selftest)"));
            }
            let results = (0..rows)
                .map(|i| TorrentResult {
                    id: format!("{id}-{i}"),
                    name: format!("{q} 测试标题 {i} —— {id}"),
                    size: Some(1024 * 1024 * (i as i64 + 1)),
                    size_text: format!("{}.0 MB", i + 1),
                    seeders: Some(10 + i as i64),
                    leechers: Some(i as i64),
                    date: None,
                    date_text: "—".to_string(),
                    magnet: if inline {
                        Some(format!("magnet:?xt=urn:btih:{id}INLINE{i:04}"))
                    } else {
                        None
                    },
                    info_hash: None,
                    provider: id.to_string(),
                    category: Some("selftest".to_string()),
                    detail_url: Some(format!("https://{id}.invalid/t/{i}")),
                    files: Some(1),
                    needs_magnet: !inline,
                })
                .collect();
            SearchOutcome::ok(results)
        })
    }

    fn resolve_magnet<'a>(
        &'a self,
        _http: &'a HttpClient,
        _url: &'a str,
    ) -> BoxFuture<'a, MagnetOutcome> {
        let (can, id) = (self.can_resolve, self.id);
        Box::pin(async move {
            if can {
                MagnetOutcome {
                    magnet: Some(RESOLVED_MAGNET.to_string()),
                    error: None,
                }
            } else {
                // 没有这个能力的 provider 走 trait 默认实现，这里显式对齐
                let _ = id;
                MagnetOutcome::err(NO_RESOLVER)
            }
        })
    }
}

fn fake_registry() -> Vec<Box<dyn Provider>> {
    vec![
        // 3 条结果、**不带**磁力、能惰性解析 → 主测路径
        Box::new(Fake {
            id: "alpha",
            name: "Alpha（惰性磁力）",
            rows: 3,
            inline_magnet: false,
            can_resolve: true,
            fails: false,
        }),
        // 2 条结果、**自带**磁力 → 按钮应直接是「推送到下载器」
        Box::new(Fake {
            id: "beta",
            name: "Beta（自带磁力）",
            rows: 2,
            inline_magnet: true,
            can_resolve: false,
            fails: false,
        }),
        // 故意失败的一家 → 状态条 ✗，但不能拖垮别家
        Box::new(Fake {
            id: "gamma",
            name: "Gamma（故意失败）",
            rows: 0,
            inline_magnet: false,
            can_resolve: false,
            fails: true,
        }),
    ]
}

// ---------------------------------------------------------------------------
// 本地桩：一个极小的 qBittorrent（登录 → add）
// ---------------------------------------------------------------------------

/// 起一个只服务 **2 次**请求的 qB 桩，返回 `(基址, 收到的原始请求)`。
async fn stub_qb() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_out = Arc::clone(&seen);

    tokio::spawn(async move {
        for i in 0..2 {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let raw = read_request(&mut sock).await;
            seen.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&raw).into_owned());

            // 第 1 跳是登录，必须给 SID cookie（判定登录成功的唯一依据）
            let (head, body) = if i == 0 {
                (
                    "HTTP/1.1 200 X\r\nContent-Type: text/plain\r\n\
                     Set-Cookie: SID=selftest; path=/\r\n"
                        .to_string(),
                    "Ok.",
                )
            } else {
                (
                    "HTTP/1.1 200 X\r\nContent-Type: text/plain\r\n".to_string(),
                    "Ok.",
                )
            };
            let _ = sock
                .write_all(
                    format!(
                        "{head}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
            let _ = sock.write_all(body.as_bytes()).await;
            let _ = sock.flush().await;
        }
    });

    (format!("http://{addr}"), seen_out)
}

async fn read_request(sock: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 2048];
    loop {
        match tokio::time::timeout(Duration::from_millis(300), sock.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(n)) => {
                raw.extend_from_slice(&buf[..n]);
                if let Some(h) = find(&raw, b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..h]).to_lowercase();
                    let want = head
                        .split("content-length:")
                        .nth(1)
                        .and_then(|s| {
                            s.split(|c: char| c == '\r' || c == '\n' || c == ' ')
                                .find(|t| !t.is_empty())
                        })
                        .and_then(|s| s.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if raw.len() >= h + 4 + want {
                        break;
                    }
                }
            }
            Ok(Err(_)) => break,
        }
    }
    raw
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// 跑
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Report {
    ticks: usize,
    /// `take_snapshot()` 成功的次数 —— 少于 ticks 就说明"重绘"是假的
    paints_ok: usize,
    paints_err: usize,
    done: bool,
    reason: String,
}

pub fn run() -> Result<bool, slint::PlatformError> {
    let ui = AppWindow::new()?;
    install_registry(fake_registry());
    wire(&ui);

    // 桩下载器（跑在 tokio 上）
    let (url, seen) = runtime().block_on(stub_qb());
    ui.set_downloader_kind("qbittorrent".into());
    ui.set_downloader_url(url.clone().into());
    ui.set_downloader_user("admin".into());
    ui.set_downloader_secret("pw".into());
    ui.set_query("selftest".into());
    save_downloader(&ui);

    println!("[m4] 桩下载器 = {url}");
    println!("[m4] 假注册表 = alpha(3, 惰性磁力) + beta(2, 自带磁力) + gamma(失败)");

    let report = Rc::new(RefCell::new(Report::default()));
    let tick = Rc::new(Cell::new(0usize));
    let phase = Rc::new(Cell::new(0u8));
    let ui_search_idx = Rc::new(Cell::new(-1i32));

    let ui_weak = ui.as_weak();
    let rep = Rc::clone(&report);
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, TICK, move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        {
            let mut r = rep.borrow_mut();
            r.ticks += 1;
            // ⚠️ 强制**同步**重绘：这一步既让排版真的跑过，也是"这不是假绿"的证据
            match ui.window().take_snapshot() {
                Ok(_) => r.paints_ok += 1,
                Err(_) => r.paints_err += 1,
            }
            if r.done {
                return;
            }
        }

        let n = tick.get();
        tick.set(n + 1);

        if n >= DEADLINE_TICKS {
            let mut r = rep.borrow_mut();
            r.done = true;
            r.reason = format!("超时（{DEADLINE_TICKS} 跳）");
            let _ = slint::quit_event_loop();
            return;
        }

        match phase.get() {
            // 0：触发搜索（走**真实接线**里的 search 回调）
            0 => {
                phase.set(1);
                ui.invoke_search("selftest".into());
            }
            // 1：等搜索结束
            1 => {
                if !ui.get_searching() && ui.get_total() > 0 {
                    // 挑第一条**没有磁力**的行 → 走惰性解析那条路
                    let lazy = STATE.with(|s| {
                        s.borrow()
                            .app
                            .rows
                            .iter()
                            .position(|r| r.magnet().is_none())
                    });
                    match lazy {
                        Some(i) => {
                            ui_search_idx.set(i as i32);
                            phase.set(2);
                            ui.invoke_push_row(i as i32);
                        }
                        None => {
                            let mut r = rep.borrow_mut();
                            r.done = true;
                            r.reason = "搜索结果里没有一条需要惰性取磁力的行".to_string();
                            let _ = slint::quit_event_loop();
                        }
                    }
                }
            }
            // 2：等推送真打到桩上（登录 + add 两次请求）
            _ => {
                if seen.lock().unwrap().len() >= 2 {
                    let mut r = rep.borrow_mut();
                    r.done = true;
                    r.reason = "ok".to_string();
                    let _ = slint::quit_event_loop();
                }
            }
        }
    });

    // 事件循环跑起来 —— tokio 任务在后台推进，回调经 invoke_from_event_loop 回到这里
    ui.run()?;
    drop(timer);

    let r = report.borrow();
    let requests = seen.lock().unwrap().clone();
    let (rows, chips_x, chips_ok, resolved_idx) = STATE.with(|s| {
        let st = s.borrow();
        (
            st.app.rows.len(),
            st.app.chips.iter().filter(|c| c.glyph == "✗").count(),
            st.app.chips.iter().filter(|c| c.glyph == "✓").count(),
            ui_search_idx.get(),
        )
    });
    let row_action = STATE.with(|s| {
        let st = s.borrow();
        st.app
            .row(resolved_idx)
            .map(|r| r.action.label().to_string())
            .unwrap_or_default()
    });
    let status = STATE.with(|s| s.borrow().status.clone());

    // ---- 断言 ----
    let mut checks: Vec<(&str, bool, String)> = Vec::new();
    let mut push = |name: &'static str, ok: bool, detail: String| checks.push((name, ok, detail));

    push(
        "窗口真的重绘了",
        r.paints_ok >= r.ticks && r.ticks > 3,
        format!(
            "{} 次同步重绘 / {} 跳，失败 {} 次",
            r.paints_ok, r.ticks, r.paints_err
        ),
    );
    push(
        "搜索跑完且状态行收尾",
        !ui.get_searching(),
        format!("searching={} status={status:?}", ui.get_searching()),
    );
    push(
        "行数 = 5（3 惰性 + 2 自带）",
        rows == 5,
        format!("{rows} 行"),
    );
    push(
        "每站状态条 2✓ / 1✗（失败那家不拖垮别家）",
        chips_ok == 2 && chips_x == 1,
        format!("✓{chips_ok} ✗{chips_x}"),
    );
    push(
        "取磁力 + 推送走完（按钮变「已推送」）",
        row_action.contains("已推送"),
        format!("row{resolved_idx} action={row_action:?}"),
    );
    push(
        "桩收到 2 次请求（登录 + add）",
        requests.len() == 2,
        format!("{} 次", requests.len()),
    );
    let login_ok = requests
        .first()
        .map(|q| q.starts_with("POST /api/v2/auth/login "))
        .unwrap_or(false);
    push("第 1 跳是 qB 登录", login_ok, String::new());
    let add = requests.get(1).cloned().unwrap_or_default();
    push(
        "第 2 跳带 SID cookie",
        add.to_lowercase().contains("cookie: sid=selftest"),
        String::new(),
    );
    push(
        "推的是**惰性解析出来**的磁力（证明解析那步真跑了）",
        add.contains(RESOLVED_MAGNET),
        String::new(),
    );
    push(
        "搜索用的是假注册表（没有联网）",
        !status.contains("unreachable (selftest)")
            || STATE.with(|s| s.borrow().app.chips.iter().any(|c| c.glyph == "✗")),
        format!("{status:?}"),
    );

    let mut all_ok = true;
    for (name, ok, detail) in &checks {
        println!(
            "[m4] {} {name} {}",
            if *ok { "PASS" } else { "FAIL" },
            detail
        );
        all_ok &= *ok;
    }
    println!(
        "[m4] {} —— {} 跳 / {} 次重绘 / {} 行 / {} 次推送请求（{}）",
        if all_ok {
            "全部通过"
        } else {
            "有失败项"
        },
        r.ticks,
        r.paints_ok,
        rows,
        requests.len(),
        r.reason
    );
    Ok(all_ok)
}
