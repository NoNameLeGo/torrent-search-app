//! `src/providers/knaben.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实的 Knaben 响应快照**（`test/fixtures/knaben-ubuntu.json`，5 条），
//! 其余路径由本地一次性 HTTP 服务构造。
//!
//! 期望值不是手抄的 —— 是把同一个 fixture 喂给 `src/lib/normalize.js` 跑出来的输出，
//! 逐字段抄进断言。这样"Rust 版结果与 Node 版一致"才有实据。

mod common;

use bt_core::http::HttpClient;
use bt_providers::knaben;
use serde_json::{json, Value};

use common::fixture;

// ---- 真实 fixture ---------------------------------------------------------

#[tokio::test]
async fn parses_the_real_knaben_fixture() {
    let body = fixture("knaben-ubuntu.json");
    let url = common::oneshot(200, &body).await;

    let out = knaben::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 5, "fixture 里是 5 条");

    let first = &out.results[0];
    assert_eq!(first.provider, "knaben");
    assert_eq!(first.name, "ubuntu-26.04-desktop-amd64.iso");
    assert_eq!(first.id, "knaben:DAFC8C076CA2F3ED376EEAE7C76A0D6BE2415C45");
    assert_eq!(
        first.info_hash.as_deref(),
        Some("DAFC8C076CA2F3ED376EEAE7C76A0D6BE2415C45"),
        "knaben 的 hash 字段直接给，不必从磁力链里抠"
    );
    assert_eq!(first.size, Some(6_517_612_871), "bytes 是 JSON 数字");
    assert_eq!(first.size_text, "6.1 GB");
    assert_eq!(first.seeders, Some(181));
    assert_eq!(first.leechers, Some(10), "knaben 的 peers 就是 leechers");
    assert_eq!(
        first.date,
        Some(1_777_127_700_000),
        "date 是 ISO 8601 字符串 \"2026-04-25T14:35:00+00:00\""
    );
    assert_eq!(first.date_text, "2026-04-25");
    assert_eq!(
        first.category.as_deref(),
        Some("Apps"),
        "categoryId 4000000"
    );
    assert_eq!(
        first.detail_url.as_deref(),
        Some("https://knaben.xyz/thepiratebay/description.php?id=82917197")
    );
    assert_eq!(first.files, None, "knaben 不提供文件数");
    assert!(!first.needs_magnet);
    assert!(
        first
            .magnet
            .as_deref()
            .unwrap()
            .starts_with("magnet:?xt=urn:btih:DAFC8C076CA2F3ED376EEAE7C76A0D6BE2415C45"),
        "magnetUrl 原样透传"
    );
}

#[tokio::test]
async fn every_fixture_row_makes_it_through() {
    let body = fixture("knaben-ubuntu.json");
    let url = common::oneshot(200, &body).await;

    let out = knaben::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(
        out.results.iter().all(|r| !r.name.is_empty()),
        "不该有结果退化成 (untitled)"
    );
    assert!(
        out.results
            .iter()
            .all(|r| r.magnet.is_some() && r.info_hash.is_some()),
        "5 条都带 hash + magnetUrl"
    );
}

/// `categoryId` 是**数组**，JS 取 `Math.min(...)` 而不是首元素。
///
/// fixture 第 4 条是 `[10000000, 9001000]`：取首元素会得到 Other（错），取最小才是 Books。
#[tokio::test]
async fn category_uses_the_minimum_id_not_the_first() {
    let body = fixture("knaben-ubuntu.json");
    let url = common::oneshot(200, &body).await;

    let out = knaben::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let book = &out.results[3];
    assert_eq!(
        book.name,
        "Clinton D., Negus C. Ubuntu Linux Bible 11ed 2025"
    );
    assert_eq!(
        book.category.as_deref(),
        Some("Books"),
        "categoryId=[10000000, 9001000] → min=9001000 → Books"
    );
}

// ---- 分类映射 -------------------------------------------------------------

#[test]
fn category_ids_map_to_the_standard_buckets() {
    // 每个区间的左端点
    assert_eq!(knaben::knaben_category(1_000_000).as_deref(), Some("Music"));
    assert_eq!(
        knaben::knaben_category(2_000_000).as_deref(),
        Some("Series")
    );
    assert_eq!(
        knaben::knaben_category(3_000_000).as_deref(),
        Some("Movies")
    );
    assert_eq!(knaben::knaben_category(4_000_000).as_deref(), Some("Apps"));
    assert_eq!(knaben::knaben_category(5_000_000).as_deref(), Some("Porn"));
    assert_eq!(knaben::knaben_category(6_000_000).as_deref(), Some("Anime"));
    assert_eq!(knaben::knaben_category(7_000_000).as_deref(), Some("Games"));
    assert_eq!(knaben::knaben_category(9_000_000).as_deref(), Some("Books"));
    assert_eq!(
        knaben::knaben_category(10_000_000).as_deref(),
        Some("Other")
    );

    // 区间内
    assert_eq!(knaben::knaben_category(4_004_000).as_deref(), Some("Apps"));
    assert_eq!(knaben::knaben_category(9_001_000).as_deref(), Some("Books"));

    // ⚠️ 8 开头**没有映射**（JS 里也是这样，不是漏写）
    assert_eq!(knaben::knaben_category(8_000_000), None);
    // 右端点闭合、越界
    assert_eq!(
        knaben::knaben_category(2_000_000 - 1).as_deref(),
        Some("Music")
    );
    assert_eq!(knaben::knaben_category(11_000_000), None);
    assert_eq!(knaben::knaben_category(0), None);
    assert_eq!(knaben::knaben_category(-1), None);
}

// ---- 请求契约 -------------------------------------------------------------

/// POST body 的字段名是服务端约定的 snake_case，写错了只在真实环境才炸 —— 钉住它。
#[tokio::test]
async fn request_body_matches_the_js_payload() {
    let body = fixture("knaben-ubuntu.json");
    let (url, seen) = common::oneshot_capture(200, &body).await;

    let _ = knaben::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("POST "), "必须是 POST: {raw}");
    assert!(raw
        .to_lowercase()
        .contains("content-type: application/json"));

    let payload: Value = serde_json::from_str(
        raw.split_once("\r\n\r\n")
            .expect("请求里该有头/体分隔")
            .1
            .trim_end_matches('\0'),
    )
    .expect("body 该是 JSON");

    assert_eq!(payload["query"], json!("ubuntu"));
    assert_eq!(payload["size"], json!(300));
    assert_eq!(payload["order_by"], json!("seeders"));
    assert_eq!(payload["order_direction"], json!("desc"));
    assert_eq!(payload["hide_unsafe"], json!(true));
    assert_eq!(payload["hide_xxx"], json!(false));
}

// ---- 空结果与失败路径 -----------------------------------------------------

#[tokio::test]
async fn empty_hits_is_no_results_not_an_error() {
    let url = common::oneshot(200, r#"{"hits":[],"total":{"value":0}}"#).await;
    let out = knaben::search_at(&HttpClient::new(), &url, "nonexistent-xyz-12345").await;

    assert_eq!(out.error, None, "无结果不是错误");
    assert!(out.results.is_empty());
}

/// JS 用 `Array.isArray(data.hits)` 兜底，所以 `hits` 缺失/不是数组时一律当空结果。
#[tokio::test]
async fn hits_missing_or_not_an_array_is_no_results() {
    for body in [
        r#"{"total":{"value":0}}"#,
        r#"{"hits":"nope"}"#,
        r#"[1,2,3]"#,
    ] {
        let url = common::oneshot(200, body).await;
        let out = knaben::search_at(&HttpClient::new(), &url, "ubuntu").await;
        assert_eq!(out.error, None, "body={body}");
        assert!(out.results.is_empty(), "body={body}");
    }
}

#[tokio::test]
async fn http_error_becomes_knaben_unreachable() {
    let url = common::oneshot(503, "nope").await;
    let out = knaben::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("应当有 error");
    assert!(e.contains("Knaben unreachable"), "{e}");
}

/// ⚠️ **有意偏离 JS**（divergence，别当 bug 修）。
///
/// JS 拿到「200 + 非 JSON 正文」时：axios 把正文当字符串塞进 `data`，
/// `data.hits` 是 `undefined` → 不是数组 → 静默返回 `{ results: [] }`。
/// 结果是被 Cloudflare 拦了、还是站点改版了、还是真的没结果，**完全分不出来**。
///
/// Rust 版走 `bt-core::http` 的 `is_parse_error` 分支，显式报错并附上正文开头 ——
/// 对抓取型项目来说，"静默为空"比"报错"危险得多。
#[tokio::test]
async fn divergence_non_json_body_becomes_an_error_instead_of_silent_empty() {
    let url = common::oneshot(200, "<html>Just a moment...</html>").await;
    let out = knaben::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("Rust 版应当报错，JS 版会静默返回空");
    assert!(e.contains("Knaben unreachable"), "{e}");
    assert!(
        e.contains("Just a moment"),
        "错误里必须带原始正文开头，否则拦页和改版分不清: {e}"
    );
}
