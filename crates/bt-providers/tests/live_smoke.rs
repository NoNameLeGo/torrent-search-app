//! 联网冒烟测试 —— **默认跳过**，只有 `BT_LIVE_SMOKE=1` 时才真的打外网。
//!
//! 为什么要有它（Q5 决策的 B 半）：`test/fixtures/` 是历史快照，离线测试全绿
//! **只能证明"解析逻辑没退化"，证明不了"站点现在还能用"**。
//! 这条在 CI（干净 DNS）上跑，用来回答那个问题。
//!
//! 另外它还有一个作用：**抓真实响应形态**。比如 `internetarchive`，
//! 本机对 `archive.org` 有 DNS 污染抓不到，只能靠 CI 的日志看真数据长什么样。
//!
//! 用法：
//! ```bash
//! BT_LIVE_SMOKE=1 cargo test -p bt-providers --test live_smoke -- --nocapture --test-threads=1
//! ```
//! 对应 workflow：`.github/workflows/live-smoke.yml`（仅 `workflow_dispatch`，允许失败）。

use bt_core::http::{HttpClient, JsonResponse};
use bt_providers::{internetarchive, knaben, torrentscsv, tpb, yts, SearchOutcome};
use serde_json::Value;

fn live() -> bool {
    matches!(std::env::var("BT_LIVE_SMOKE").as_deref(), Ok("1"))
}

/// 五个 JSON 组 provider 各打一次真实接口。
#[tokio::test]
async fn smoke_the_json_group() {
    if !live() {
        eprintln!("[smoke] 跳过：未设置 BT_LIVE_SMOKE=1（离线测试不碰外网）");
        return;
    }

    let http = HttpClient::new();

    // 注意：这里用的是各家**默认端点**，不是本地服务
    let cases: Vec<(&str, SearchOutcome)> = vec![
        ("tpb", tpb::search(&http, "ubuntu", 1).await),
        ("knaben", knaben::search(&http, "ubuntu", 1).await),
        ("torrentscsv", torrentscsv::search(&http, "ubuntu", 1).await),
        // yts 只收电影，用 "ubuntu" 会是 0 条（不是错误）
        ("yts", yts::search(&http, "matrix", 1).await),
        (
            "internetarchive",
            internetarchive::search(&http, "ubuntu", 1).await,
        ),
    ];

    let mut broken = Vec::new();
    for (name, out) in &cases {
        println!(
            "[smoke] {name}: results={} error={:?}",
            out.results.len(),
            out.error
        );
        if let Some(first) = out.results.first() {
            println!("[smoke] {name} first: {first:?}");
        }
        if out.error.is_some() {
            broken.push(format!("{name}: {}", out.error.clone().unwrap_or_default()));
        }
    }

    assert!(
        broken.is_empty(),
        "这些 provider 现在报错了（可能是站点改版、也可能是被墙/限流，别直接当成回归）:\n{}",
        broken.join("\n")
    );
}

/// 把 InternetArchive 的**原始响应**打印出来，用来核对真实字段形态。
///
/// 本机 DNS 对 archive.org 有污染，只能靠 CI 跑这条看真数据。
#[tokio::test]
async fn dump_internetarchive_raw_shape() {
    if !live() {
        eprintln!("[smoke] 跳过：未设置 BT_LIVE_SMOKE=1");
        return;
    }

    let http = HttpClient::new();
    let url = format!(
        "{}/advancedsearch.php?q=title:ubuntu&fl[]=title,item_size,publicdate,\
         mediatype,identifier,btih&rows=3&page=1&output=json",
        internetarchive::BASE
    );

    let resp: JsonResponse<Value> = http.get_json(&url, None).await;
    println!(
        "[smoke] IA raw error={:?} status={:?}",
        resp.error, resp.status
    );

    let Some(root) = resp.data else {
        println!("[smoke] IA 没拿到 JSON 正文，无法核对字段");
        return;
    };

    match root
        .get("response")
        .and_then(|r| r.get("docs"))
        .and_then(Value::as_array)
    {
        Some(docs) => {
            println!("[smoke] IA docs.len()={}", docs.len());
            for (i, d) in docs.iter().enumerate() {
                println!(
                    "[smoke] IA doc[{i}] = {}",
                    serde_json::to_string_pretty(d).unwrap_or_else(|_| "<打印失败>".to_string())
                );
            }
        }
        None => println!(
            "[smoke] IA 响应结构与预期不同，全文如下：\n{}",
            serde_json::to_string_pretty(&root).unwrap_or_else(|_| "<打印失败>".to_string())
        ),
    }
}
