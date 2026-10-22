//! `src/providers/mikan.js` 的对照测试。
//!
//! fixture 是 **2026-10-22 由 CI 抓的真实结果页**
//! （`test/fixtures/mikan-one-piece.html`，4.0MB / **1000 行**，全部带名称与磁力）。
//! 本机对 mikanani.me 不通，快照只能让 CI 取（`fetch-fixture.yml`）。
//!
//! ## 这个站的两处不同
//!
//! 1. 磁力在 **`data-clipboard-text`** 属性上（站上的「复制磁力」按钮），不在 `href` 里；
//! 2. 名称/大小/日期都走 `ownText`（只取直接子文本节点）。
//!
//! ## 一处**丢字段**（有意，写在 `mikan.rs` 文件头）
//!
//! JS 会带 `fileDownloadLink`（第 5 格的 `.torrent` 直链），Rust 的 `TorrentResult`
//! 没有这个字段 ⇒ 丢弃。下面有一条测试把这件事**明确钉住**，免得以后被当成 bug 重查。

mod common;

use bt_core::http::HttpClient;
use bt_providers::mikan;
use common::fixture;

/// 1000 行数据，全部带名称 + 磁力。
const EXPECTED_RESULTS: usize = 1000;

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("mikan-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = mikan::search_at(&HttpClient::new(), &base, "one piece").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("mikan-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = mikan::search_at(&HttpClient::new(), &base, "one piece").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "mikan");
    // 标题里的 `&#x6D77;&#x8D3C;&#x738B;` 必须被解码成「海賊王」
    assert!(
        r.name.starts_with("[Skymoon-Raws][One Piece 海賊王][1180]"),
        "实体没解码？实际：{}",
        r.name
    );

    // 磁力来自 `data-clipboard-text`（属性值里的 `&amp;` 也要解码）
    let magnet = r.magnet.as_deref().unwrap_or_default();
    assert!(
        magnet.starts_with("magnet:?xt=urn:btih:5a46994e26102dd7d1c48ef8c14e4282df61f2e6"),
        "实际：{magnet}"
    );
    assert!(
        magnet.contains("&tr=") && !magnet.contains("&amp;"),
        "属性实体必须解码：{magnet}"
    );

    // 这个站给的是 **hex** hash（40 位）⇒ 能取到 infoHash（与 subsplease 的 base32 不同）
    assert_eq!(
        r.info_hash.as_deref(),
        Some("5a46994e26102dd7d1c48ef8c14e4282df61f2e6")
    );

    assert_eq!(r.size_text, "457.9 MB");
    assert_eq!(r.date_text, "2026-09-28");
    assert_eq!(r.category.as_deref(), Some("Anime"));
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}/Home/Episode/5a46994e26102dd7d1c48ef8c14e4282df61f2e6").as_str())
    );
    assert!(!r.needs_magnet, "有磁力就不该标 needs_magnet");
}

/// 有意丢字段：`fileDownloadLink` 在 Rust 侧**没有落点**。
/// 这条测试的作用是「把已知差异写进代码」，不是验证某个行为。
#[tokio::test]
async fn divergence_file_download_link_is_dropped() {
    let url = common::oneshot(200, &fixture("mikan-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = mikan::search_at(&HttpClient::new(), &base, "one piece").await;
    let r = &out.results[0];

    // `TorrentResult` 没有 file_download_link 字段 ⇒ 这里只能断言「其余都还在」：
    // 磁力在、详情页在，用户仍能拿到种子（点详情页或直接用磁力）。
    assert!(r.magnet.is_some());
    assert!(r.detail_url.is_some());
    // 详情页指向的是 `/Home/Episode/<hash>`，不是那个 `.torrent` 直链
    assert!(!r.detail_url.as_deref().unwrap_or("").ends_with(".torrent"));
}

#[tokio::test]
async fn every_row_has_a_hex_hash_and_an_anime_category() {
    let url = common::oneshot(200, &fixture("mikan-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = mikan::search_at(&HttpClient::new(), &base, "one piece").await;

    for r in &out.results {
        assert_eq!(r.category.as_deref(), Some("Anime"), "{}", r.name);
        assert!(r.magnet.is_some(), "{} 没有磁力", r.name);
        assert_eq!(
            r.info_hash.as_deref().map(str::len),
            Some(40),
            "{} 的 hash 不是 40 位 hex",
            r.name
        );
    }
}

/// 请求契约：参数名是 `searchstr`（不是 `q`），关键词要编码。
#[tokio::test]
async fn request_url_contract() {
    let (url, req) = common::oneshot_capture(200, &fixture("mikan-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = mikan::search_at(&HttpClient::new(), &base, "one piece").await;

    let raw = req.lock().unwrap().clone();
    assert!(raw.starts_with("GET /Home/Search?"), "请求行：{raw}");
    assert!(raw.contains("searchstr=one%20piece"), "参数名与编码：{raw}");
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = mikan::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 没有结果行 ⇒ 0 条 + **不算错误**（JS 返回 `error: null`）。
#[tokio::test]
async fn page_without_rows_is_not_an_error() {
    let url = common::oneshot(200, "<html><body>nothing</body></html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = mikan::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}
