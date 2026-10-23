//! `src/providers/bangumimoe.js` 的对照测试。
//!
//! fixture 是 **2026-10-22 由 CI 用 POST 抓的真实响应**
//! （`test/fixtures/bangumimoe-one-piece.json`，43KB / 30 条，全部带磁力）。
//! 本机对 bangumi.moe 不通，且这是 **POST** 接口 —— 为此给 `fetch-fixture.yml`
//! 加了可选的 `method` / `body` 输入（2026-10-22）。
//!
//! ## 期望值的来源
//!
//! 全部来自 **Node 侧导出器**（`node scripts/diff/dump-node.cjs bangumimoe`），
//! 不是手算 —— 这条路径不需要 `node_modules`，JSON 类 provider 本机就能当 oracle。
//!
//! ## 一个必须照抄的"错"字段名
//!
//! 站上返回 `infoHash`（大写 H），而 JS 读的是 `obj.infohash`（小写）⇒ 永远 undefined
//! ⇒ **总是走 `extractInfoHash(magnet)` 回退**。这里同样先读小写键再回退。

mod common;

use bt_core::http::HttpClient;
use bt_providers::bangumimoe;
use common::fixture;

/// 30 条，全部带 title + magnet。
const EXPECTED_RESULTS: usize = 30;

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("bangumimoe-one-piece.json")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = bangumimoe::search_at(&HttpClient::new(), &base, "one piece").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("bangumimoe-one-piece.json")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = bangumimoe::search_at(&HttpClient::new(), &base, "one piece").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "bangumimoe");
    assert_eq!(
        r.name, "[黒ネズミたち] 海贼王 / One Piece - 1180 (B-Global 3840x2160 HEVC AAC MKV)",
        "日文/中文混排要原样保留"
    );

    // `size` 是字符串 "1.04 GB" ⇒ 解析成字节再格式化 ⇒ "1.0 GB"（<100 取一位小数）
    assert_eq!(r.size, Some(1_116_691_497));
    assert_eq!(r.size_text, "1.0 GB");

    // ⚠️ 0 是**真值**，不是"没有"（这条站给的就是 0/0）
    assert_eq!(r.seeders, Some(0));
    assert_eq!(r.leechers, Some(0));

    // publish_time 是 RFC 3339（`2026-09-28T02:01:37.607Z`）
    assert_eq!(r.date, Some(1_790_560_897_607));
    assert_eq!(r.date_text, "2026-09-28");

    assert_eq!(
        r.info_hash.as_deref(),
        Some("5f647b282e8f644ca72ba425179a64e40eae8d02"),
        "小写 infohash 取不到 ⇒ 从 magnet 里抠（与 JS 同一条回退）"
    );
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}/torrent/6ab9ca819605d5c6906d7fc5").as_str())
    );
    assert_eq!(r.category.as_deref(), Some("Anime"));
    assert!(!r.needs_magnet);
}

/// 请求契约：POST + `Content-Type: application/json` + body 是 `{"query":"…"}`。
#[tokio::test]
async fn request_is_a_json_post() {
    let (url, req) = common::oneshot_capture(200, &fixture("bangumimoe-one-piece.json")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = bangumimoe::search_at(&HttpClient::new(), &base, "one piece").await;

    let raw = req.lock().unwrap().clone();
    assert!(
        raw.starts_with("POST /api/v2/torrent/search"),
        "请求行：{raw}"
    );
    assert!(
        raw.to_ascii_lowercase()
            .contains("content-type: application/json"),
        "必须是 JSON 请求：{raw}"
    );
    // body 逐字对齐 JS 的 `JSON.stringify({ query })`
    assert!(raw.contains(r#"{"query":"one piece"}"#), "body：{raw}");
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = bangumimoe::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 200 + 但没有 `torrents` 数组 ⇒ JS 的 `bad_response`（不是"没结果"）。
#[tokio::test]
async fn missing_torrents_array_is_bad_response() {
    let url = common::oneshot(200, r#"{"success":true}"#).await;
    let base = url.trim_end_matches('/').to_string();

    let out = bangumimoe::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error.as_deref(), Some("bad_response"));
}

#[tokio::test]
async fn empty_torrents_array_is_not_an_error() {
    let url = common::oneshot(200, r#"{"torrents":[]}"#).await;
    let base = url.trim_end_matches('/').to_string();

    let out = bangumimoe::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}
