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
use bt_providers::{internetarchive, knaben, megapeer, torrentscsv, tpb, yts, SearchOutcome};
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

    let mut problems = Vec::new();
    for (name, out) in &cases {
        println!(
            "[smoke] {name}: results={} error={:?}",
            out.results.len(),
            out.error
        );
        if let Some(first) = out.results.first() {
            println!("[smoke] {name} first: {first:?}");
        }

        match &out.error {
            // apibay 对机房 IP 限流/封禁（2026-10-08 实测：CI 上稳定 403）。
            // 这不是解析回归，所以单独记一条 NOTE 而不是让冒烟变红。
            Some(e) if *name == "tpb" && e.contains("403") => println!(
                "[smoke] NOTE: tpb 返回 403 —— apibay 对机房 IP 限流，属已知情况，不计入失败"
            ),
            Some(e) => problems.push(format!("{name}: {e}")),
            // 有错误要报，**没有结果但也没错误**更要报：那通常是选择器/字段失效
            None if out.results.is_empty() => problems.push(format!(
                "{name}: 0 条结果且无错误 —— 选择器或字段名可能已失效"
            )),
            None => {}
        }
    }

    println!(
        "[smoke] 汇总：{} 个 provider，{} 个问题",
        cases.len(),
        problems.len()
    );
    assert!(
        problems.is_empty(),
        "这些 provider 现在有问题（站点改版？字段改名？别直接当成代码回归）:\n{}",
        problems.join("\n")
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

/// MegaPeer —— HTML 组里第一个需要联网验证**编码**的站点。
///
/// 离线测试已经证明「响应头带 charset 时能解对 windows-1251」；
/// 这条回答的是那个前提在真实站点上成不成立：**响应头到底带不带
/// `charset=windows-1251`**。带了 → reqwest 解出正常的西里尔文本；
/// 没带 → reqwest 退 utf8，标题会全变 U+FFFD，这条就会红。
#[tokio::test]
async fn smoke_megapeer_checks_real_world_charset_decoding() {
    if !live() {
        eprintln!("[smoke] 跳过：未设置 BT_LIVE_SMOKE=1（离线测试不碰外网）");
        return;
    }

    let http = HttpClient::new();
    let out = megapeer::search(&http, "1080p", 1).await;

    println!(
        "[smoke] megapeer: results={} error={:?}",
        out.results.len(),
        out.error
    );

    let Some(first) = out.results.first() else {
        // 0 结果不等于解析回归（站点可能挡机房 IP）—— 打 NOTE 不炸，
        // 「0 条且无错误」的判定逻辑在 smoke_the_json_group 里是给 JSON 组的。
        println!(
            "[smoke] NOTE: megapeer 没返回结果（error={:?}）—— 站点状态问题，不算解析回归",
            out.error
        );
        return;
    };

    println!("[smoke] megapeer first.name = {}", first.name);
    println!("[smoke] megapeer first.size_text = {}", first.size_text);

    let has_cyrillic = first
        .name
        .chars()
        .any(|c| ('\u{0400}'..='\u{04FF}').contains(&c));
    assert!(
        has_cyrillic,
        "标题里没有西里尔字母 —— 站点可能不再发 charset=windows-1251，或改了编码: {}",
        first.name
    );
    assert!(
        !first.name.contains('\u{FFFD}'),
        "标题里有替换字符（utf8 硬解码的痕迹）: {}",
        first.name
    );
}
