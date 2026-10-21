//! `src/providers/subsplease.js` 的对照测试。
//!
//! fixture 是 **2026-10-21 抓的真实响应**（`test/fixtures/subsplease-one-piece.json`，
//! 83KB / 30 个 show / 90 个 download 条目 / 90 条带磁力）：
//! 用 `?f=search&tz=$&s=one piece` 取回。
//!
//! ## 一处**有意**的差异（顺序）
//!
//! JS 用 `Object.entries()`（服务端给的顺序），Rust 侧 `serde_json::Map` 是 `BTreeMap`
//! （**按 show 名排序**）。内容完全一致，只是同一屏里先后不同 —— 原因与代价写在
//! `subsplease.rs` 文件头。所以下面断言的是**排序后**的第一条
//! （`One Piece - 1152`），而不是 fixture 里的第一条（`One Piece - 1180`）。

mod common;

use bt_core::http::HttpClient;
use bt_providers::subsplease;
use common::fixture;

/// 30 个 show × 3 个画质（480/720/1080）= 90 条。
const EXPECTED_RESULTS: usize = 90;

/// 排序后的第一条（`BTreeMap` 顺序）。
const FIRST_SHOW: &str = "One Piece - 1152";

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("subsplease-one-piece.json")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = subsplease::search_at(&HttpClient::new(), &base, "one piece").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("subsplease-one-piece.json")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = subsplease::search_at(&HttpClient::new(), &base, "one piece").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "subsplease");
    // name = `${showName} [${res}p]`，res 取 downloads 里的 `res`
    assert_eq!(r.name, "One Piece - 1152 [480p]");

    // 体积**不在响应里**，从磁力的 `xl=` 抠出来
    assert_eq!(r.size, Some(381_904_004));
    assert!(r.size_text.ends_with("MB"), "size_text = {}", r.size_text);

    // RFC 1123 → 时间戳（UTC 日期部分）
    assert_eq!(r.date_text, "2025-12-07");
    assert!(
        r.date.is_some(),
        "Date.parse 认得 RFC 1123，Rust 也必须认得"
    );

    // detailUrl = `${base}/${page}?ep=${episode}&res=${res}`
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}/one-piece?ep=1152&res=480").as_str())
    );

    assert_eq!(r.category.as_deref(), Some("Anime"));

    // 磁力原样保留（含全部 tracker）
    assert!(r
        .magnet
        .as_deref()
        .unwrap_or_default()
        .starts_with("magnet:?xt=urn:btih:SYYPTDS3DXJFXE4HIKBMCZYZEDSXTJD7"));

    // ⚠️ 这个站给的是 **base32** hash（32 字符，含 M/Q/K 这类字母），
    // 而 `extractInfoHash` 只认 hex 32~40 位 ⇒ 两侧都是 `null`。
    // 不是 bug，是「与 JS 逐字一致」的结果（真要支持得两边一起改）。
    assert_eq!(r.info_hash, None);
    assert!(!r.needs_magnet, "有磁力就不该标 needs_magnet");
}

#[tokio::test]
async fn one_show_expands_to_one_row_per_quality() {
    let url = common::oneshot(200, &fixture("subsplease-one-piece.json")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = subsplease::search_at(&HttpClient::new(), &base, "one piece").await;
    let rows: Vec<_> = out
        .results
        .iter()
        .filter(|r| r.name.starts_with(&format!("{FIRST_SHOW} [")))
        .collect();

    assert_eq!(rows.len(), 3, "同一个 show 的 480/720/1080 各一行");
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "One Piece - 1152 [480p]",
            "One Piece - 1152 [720p]",
            "One Piece - 1152 [1080p]",
        ],
        "顺序 = downloads 数组顺序（JS 也是这个顺序）"
    );
    // 每个画质的 detailUrl 只有 res 不同
    assert_eq!(
        rows[2].detail_url.as_deref(),
        Some(format!("{base}/one-piece?ep=1152&res=1080").as_str())
    );
}

/// 文档里写明的顺序差异，用测试钉住 —— 免得以后有人以为"顺序错了"。
#[tokio::test]
async fn divergence_result_order_is_sorted_by_show_name() {
    let url = common::oneshot(200, &fixture("subsplease-one-piece.json")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = subsplease::search_at(&HttpClient::new(), &base, "one piece").await;

    // 按 show 名（去掉 "[480p]" 这类后缀）取出出现顺序，必须是**升序**
    let mut shows: Vec<String> = Vec::new();
    for r in &out.results {
        let show = r.name.split(" [").next().unwrap_or("").to_string();
        if shows.last() != Some(&show) {
            shows.push(show);
        }
    }
    let mut sorted = shows.clone();
    sorted.sort();
    assert_eq!(
        shows, sorted,
        "Rust 侧按 show 名排序（BTreeMap），JS 侧是插入序"
    );
    // 并且第一条**不是** fixture 里的第一条（`One Piece - 1180`）
    assert_eq!(out.results[0].name, "One Piece - 1152 [480p]");
}

#[tokio::test]
async fn http_error_is_reported_not_swallowed() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = subsplease::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    let err = out.error.unwrap_or_default();
    assert!(
        err.starts_with("SubsPlease unreachable"),
        "错误串要对齐 JS 的形状，实际：{err}"
    );
}

/// 有意 divergence：JS 遇到「200 + 非 JSON 正文」会静默返回空数组，
/// Rust 侧显式报错（与 `yts.rs` / `knaben.rs` 同一条偏离）。
#[tokio::test]
async fn divergence_non_json_body_errors_instead_of_empty() {
    let url = common::oneshot(200, "<html>not json</html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = subsplease::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert!(
        out.error.is_some(),
        "非 JSON 正文必须报错，而不是假装没结果"
    );
}

#[tokio::test]
async fn empty_object_yields_no_results_and_no_error() {
    let url = common::oneshot(200, "{}").await;
    let base = url.trim_end_matches('/').to_string();

    let out = subsplease::search_at(&HttpClient::new(), &base, "nothing-here").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None, "空对象是「搜到了但没结果」，不是错误");
}
