//! `src/providers/tpb.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实的 apibay 响应快照**（`test/fixtures/tpb-ubuntu.json`，
//! 100 条），其余路径由本地一次性 HTTP 服务构造。
//!
//! ⚠️ 记住这条：离线 fixture 只证明"解析逻辑没退化"，
//! **证明不了"现在还能搜出东西"** —— 快照是 2026-07 的，站点早已改版。

mod common;

use bt_core::http::HttpClient;
use bt_providers::tpb;

/// 读 `test/fixtures/<name>`。fixture 永久保留在仓库里，不随「边搬边删」删掉。
fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 fixture {}: {e}", p.display()))
}

// ---- 真实 fixture ---------------------------------------------------------

#[tokio::test]
async fn parses_the_real_apibay_fixture() {
    let body = fixture("tpb-ubuntu.json");
    let url = common::oneshot(200, &body).await;

    let out = tpb::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 100, "fixture 里是 100 条");

    let first = &out.results[0];
    assert_eq!(first.provider, "tpb");
    assert_eq!(first.name, "Ubuntu 22.04 LTS");
    assert_eq!(first.id, "tpb:2C6B6858D61DA9543D4231A71DB4B1C9264B0685");
    assert_eq!(
        first.info_hash.as_deref(),
        Some("2C6B6858D61DA9543D4231A71DB4B1C9264B0685"),
        "infoHash 保持原样大小写，JS 版也不做小写化"
    );
    assert_eq!(first.size, Some(3_654_957_056), "size 是字节数字符串");
    assert_eq!(first.size_text, "3.4 GB");
    assert_eq!(first.seeders, Some(39));
    assert_eq!(first.leechers, Some(1));
    assert_eq!(
        first.date,
        Some(1_652_877_231_000),
        "added 是秒，要 ×1000 变毫秒"
    );
    assert_eq!(first.files, Some(1));
    assert_eq!(first.category.as_deref(), Some("Apps"), "303 → 3xx → Apps");
    assert_eq!(
        first.detail_url.as_deref(),
        Some("https://thepiratebay.org/description.php?id=59191690")
    );
    assert!(
        first
            .magnet
            .as_deref()
            .unwrap()
            .starts_with("magnet:?xt=urn:btih:2C6B6858"),
        "有 infoHash 就该拼出磁力链"
    );
    assert!(!first.needs_magnet, "有 infoHash 就不需要惰性解析");
}

#[tokio::test]
async fn every_fixture_row_makes_it_through() {
    let body = fixture("tpb-ubuntu.json");
    let url = common::oneshot(200, &body).await;

    let out = tpb::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(
        out.results.iter().all(|r| !r.name.is_empty()),
        "任何一条都不该退化成 (untitled)"
    );
    assert!(
        out.results.iter().all(|r| r.magnet.is_some()),
        "apibay 每条都带 info_hash，应当全部拼出磁力链"
    );
}

// ---- 分类映射 -------------------------------------------------------------

#[test]
fn category_codes_map_to_the_standard_buckets() {
    assert_eq!(tpb::tpb_category("101").as_deref(), Some("Music"));
    assert_eq!(tpb::tpb_category("102").as_deref(), Some("Books"), "有声书");
    assert_eq!(tpb::tpb_category("201").as_deref(), Some("Movies"));
    assert_eq!(tpb::tpb_category("205").as_deref(), Some("Series"));
    assert_eq!(tpb::tpb_category("208").as_deref(), Some("Series"));
    assert_eq!(tpb::tpb_category("303").as_deref(), Some("Apps"));
    assert_eq!(tpb::tpb_category("401").as_deref(), Some("Games"));
    assert_eq!(tpb::tpb_category("501").as_deref(), Some("Porn"));
    assert_eq!(tpb::tpb_category("601").as_deref(), Some("Books"));
    assert_eq!(tpb::tpb_category("699").as_deref(), Some("Other"));
    assert_eq!(tpb::tpb_category(""), None);
    assert_eq!(tpb::tpb_category("999"), None, "9 开头没有映射");
}

// ---- 三条失败路径 ---------------------------------------------------------

#[tokio::test]
async fn single_error_element_means_no_results_not_an_error() {
    // apibay 的「无结果」约定
    let url = common::oneshot(200, r#"[{"error":"no results"}]"#).await;
    let out = tpb::search_at(&HttpClient::new(), &url, "nonexistent-xyz-12345").await;

    assert_eq!(out.error, None, "无结果不是错误");
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_becomes_tpb_unreachable() {
    let url = common::oneshot(404, "nope").await;
    let out = tpb::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("应当有 error");
    assert!(e.contains("TPB unreachable"), "{e}");
}

#[tokio::test]
async fn non_json_body_becomes_unexpected_response() {
    // tpb-empty.json 的真实内容就是限流时返回的纯文本 "429 Too Many Requests"
    let body = fixture("tpb-empty.json");
    assert!(
        !body.trim_start().starts_with('['),
        "这个 fixture 本来就不是 JSON，是限流正文"
    );
    let url = common::oneshot(200, &body).await;

    let out = tpb::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let e = out.error.expect("应当有 error");
    assert!(e.starts_with("unexpected response"), "{e}");
    assert!(
        e.contains("429"),
        "错误里必须带上原始正文，否则限流和改版分不清: {e}"
    );
}

#[tokio::test]
async fn json_that_is_not_an_array_is_unexpected_response() {
    let url = common::oneshot(200, r#"{"status":"ok"}"#).await;
    let out = tpb::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert_eq!(out.error.as_deref(), Some("unexpected response"));
    assert!(out.results.is_empty());
}
