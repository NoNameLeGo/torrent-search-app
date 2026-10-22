//! `src/providers/animetosho.js` 的对照测试。
//!
//! fixture 是 **2026-10-22 由 CI 抓的真实结果页**
//! （`test/fixtures/animetosho-one-piece.html`，170KB / **75 个条目**，全部能解析）。
//! 本机对 animetosho.org 不通（`curl` 直接失败），快照只能让 CI 取。
//!
//! ## 日期这条最容易错
//!
//! 站上给的是 `Date/time submitted: 06/05/2026 11:02`，而 JS：
//! ① 去掉前缀 → ② **只取空格前那一段**（丢掉 `11:02`）→ ③ 按 `d/M/yyyy` 解析
//! → ④ `new Date(y, m-1, d)` = **本地时区零点**。
//! 所以 `06/05/2026` 是 **5 月 6 日**（不是 6 月 5 日），时间是 00:00 而不是 11:02。
//! 断言必须现算（本地时区），不能写死 `date_text` —— CI 是 UTC、本机是 +08:00。

mod common;

use bt_core::http::HttpClient;
use bt_providers::animetosho;
use chrono::{Local, TimeZone};
use common::fixture;

/// 75 个条目，全部同时具备名称/大小/日期/磁力。
const EXPECTED_RESULTS: usize = 75;

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("animetosho-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = animetosho::search_at(&HttpClient::new(), &base, "one piece").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("animetosho-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = animetosho::search_at(&HttpClient::new(), &base, "one piece").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "animetosho");
    // 名字里有 `<wbr/>`（断行提示），它没有文本 ⇒ 拼起来是连着的
    assert_eq!(
        r.name, "One.Piece.Vol002.DVDRip.480p.x265.10bit.AAC.Multi-uP",
        "`<wbr/>` 不该被当成空格"
    );

    // ⚠️ 站上原文是 `1.874 GB`，但 `size_text` 是**按字节重算再格式化**的结果
    // （JS 的 `formatSize(parseSize(...))` 也是这个口径）⇒ 1.874 GB → "1.9 GB"。
    // 断言原文 `1.874 GB` 会红 —— 那不是 bug，是显示口径。
    // 同理不写死 `size` 的字节数：它是从**文本** `1.874 GB` 反算出来的，
    // 与 `div.size` 那个 `title="… 2,012,372,487 bytes"` 不是同一个来源（JS 也没用它）。
    assert_eq!(r.size_text, "1.9 GB");
    assert!(r.size.is_some());

    // 06/05/2026 → 2026-05-06 **本地** 00:00
    let expected_date = Local
        .with_ymd_and_hms(2026, 5, 6, 0, 0, 0)
        .single()
        .expect("构造本地时间")
        .timestamp_millis();
    assert_eq!(
        r.date,
        Some(expected_date),
        "d/M/yyyy + 丢掉时间 + 本地零点"
    );

    // `[2↑/10↓]` → 2 / 10
    assert_eq!(r.seeders, Some(2));
    assert_eq!(r.leechers, Some(10));

    assert_eq!(r.category.as_deref(), Some("Anime"));
    assert!(r
        .magnet
        .as_deref()
        .unwrap_or_default()
        .starts_with("magnet:?xt=urn:btih:PDJTN7UDSTJTYMLDI7FWSWCVSAUEFDZA"));
    // base32 hash ⇒ 与 JS 一样取不到 hex infoHash
    assert_eq!(r.info_hash, None);
    assert_eq!(
        r.detail_url.as_deref(),
        Some("https://animetosho.org/view/one-piece-vol002-dvdrip-480p-x265-10bit-aac-multi-up.n2106671")
    );
}

#[tokio::test]
async fn every_row_has_a_magnet_size_and_date() {
    let url = common::oneshot(200, &fixture("animetosho-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = animetosho::search_at(&HttpClient::new(), &base, "one piece").await;

    for r in &out.results {
        assert!(r.magnet.is_some(), "{} 没有磁力", r.name);
        assert!(r.size.is_some(), "{} 没有体积", r.name);
        assert!(r.date.is_some(), "{} 没有日期", r.name);
        assert_eq!(r.category.as_deref(), Some("Anime"));
    }
}

/// 请求契约：`/search?q=<编码后的词>`。
#[tokio::test]
async fn request_url_contract() {
    let (url, req) = common::oneshot_capture(200, &fixture("animetosho-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = animetosho::search_at(&HttpClient::new(), &base, "one piece").await;

    let raw = req.lock().unwrap().clone();
    assert!(raw.starts_with("GET /search?"), "请求行：{raw}");
    assert!(raw.contains("q=one%20piece"), "关键词要编码：{raw}");
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = animetosho::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 没有条目容器 ⇒ 0 条 + 不算错误（JS 返回 `error: null`）。
#[tokio::test]
async fn page_without_entries_is_not_an_error() {
    let url = common::oneshot(200, "<html><body>nothing</body></html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = animetosho::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}
