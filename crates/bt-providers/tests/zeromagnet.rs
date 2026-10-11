//! `src/providers/zeromagnet.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实快照**
//! （`test/fixtures/zeromagnet-ubuntu.html` 结果页 29 条 /
//! `test/fixtures/zeromagnet-detail.html` 详情页，195 字符磁力）。
//!
//! 期望值来自 Node 端 `normalize.js`（`category: "Porn"` 是 JS 硬编码的）。
//!
//! ⚠️ 两条本站特有的注意点：
//! 1. 名称被 `<mark>` 拆开 → 必须拼接（测试用真实 fixture 的前 3 条钉住）
//! 2. meta 列（size/日期）JS 全丢 → 断言写 `None` / `—`（照抄，不是 bug）

mod common;

use bt_core::http::HttpClient;
use bt_providers::zeromagnet;
use common::fixture;

const EXPECTED_RESULTS: usize = 29;

const FIRST_HASH: &str = "01c137287d6f0ed05a56742dae794f632c79ff3d";

#[tokio::test]
async fn parses_all_29_rows_from_the_real_fixture() {
    let url = common::oneshot(200, &fixture("zeromagnet-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = zeromagnet::search_at(&HttpClient::new(), &base, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS, "没有表头行");
}

#[tokio::test]
async fn first_row_fields_match_the_normalized_js_pipeline() {
    let url = common::oneshot(200, &fixture("zeromagnet-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = zeromagnet::search_at(&HttpClient::new(), &base, "ubuntu").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "zeromagnet");
    // ⚠️ 页面里是 `<mark>ubuntu</mark>-24.04.4-…` —— 名字必须是拼接后的完整文本
    assert_eq!(r.name, "ubuntu-24.04.4-desktop-amd64.iso");
    assert_eq!(r.id, "zeromagnet:ubuntu-24.04.4-desktop-amd64.iso");
    let expected_detail = format!("{base}/!lFvU");
    assert_eq!(
        r.detail_url.as_deref(),
        Some(expected_detail.as_str()),
        "detail_url 由 base + 短 ID 拼出（别写死线上域名）"
    );
    assert_eq!(r.category.as_deref(), Some("Porn"), "JS 硬编码的分类");

    assert!(r.needs_magnet, "列表页没有磁力");
    assert_eq!(r.magnet, None);
    assert_eq!(r.info_hash, None);

    // ⚠️ meta 列里其实有 6.2GB / 2026-08-07 —— JS 全丢，照抄
    assert_eq!(r.size, None);
    assert_eq!(r.size_text, "—");
    assert_eq!(r.date, None);
    assert_eq!(r.date_text, "—");
    assert_eq!(r.seeders, None);
    assert_eq!(r.leechers, None);
}

#[tokio::test]
async fn all_row_names_reassemble_the_mark_highlight() {
    let url = common::oneshot(200, &fixture("zeromagnet-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = zeromagnet::search_at(&HttpClient::new(), &base, "ubuntu").await;

    let names: Vec<&str> = out.results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        &names[..3],
        &[
            "ubuntu-24.04.4-desktop-amd64.iso",
            "ubuntu-22.04.5-desktop-amd64.iso",
            "ubuntu-23.04-desktop-amd64.iso",
        ]
    );
    assert!(
        names.iter().all(|n| n.starts_with("ubuntu")),
        "每个名字都该以关键词开头（拼接正确）"
    );
    assert!(
        names.iter().all(|n| !n.contains('\u{FFFD}')),
        "不该有替换字符"
    );
}

/// 钉住请求 URL：`{base}/search?q=<encodeURIComponent(query)>`（**没有页码**）。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("zeromagnet-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = zeromagnet::search_at(&HttpClient::new(), &base, "ubuntu 22").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("/search?q=ubuntu%2022 "), "{raw}");
}

/// 没有行 → 这一站的错误串是**裸 `no_results`**（与 oxtorrent 同款）。
#[tokio::test]
async fn page_without_rows_reports_no_results() {
    let url = common::oneshot(
        200,
        "<html><body><table class=\"file-list\"><tbody></tbody></table></body></html>",
    )
    .await;

    let out = zeromagnet::search_at(&HttpClient::new(), &url, "nonexistent-xyz").await;

    assert_eq!(out.error.as_deref(), Some("no_results"));
    assert!(out.results.is_empty());
}

/// 有行但一条都解析不出来 → **`error: None`**（JS 的 `error: null`）。
#[tokio::test]
async fn rows_present_but_unparseable_is_not_an_error() {
    let html = "<html><body><table class=\"file-list\"><tbody>\
                <tr><td class=\"result-title\"><a>no href</a></td></tr>\
                </tbody></table></body></html>";
    let url = common::oneshot(200, html).await;

    let out = zeromagnet::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.error, None, "JS 这里返回的是 error: null");
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(503, "nope").await;

    let out = zeromagnet::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some(), "HTTP 错误要落进 error");
}

/// `search_with` 复刻 `runMirrors` —— JS 传的 name 是 **`'ZeroMagnet'`**（注意大小写）。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><p>no table</p></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = zeromagnet::search_with(&HttpClient::new(), &[base], "nonexistent-xyz").await;

    assert_eq!(
        out.error.as_deref(),
        Some("ZeroMagnet unreachable (no_results)")
    );
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn search_returns_ok_when_results_are_non_empty() {
    let url = common::oneshot(200, &fixture("zeromagnet-ubuntu.html")).await;
    let base = url.trim_end_matches('/');

    let out = zeromagnet::search_with(&HttpClient::new(), &[base], "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

// ---- resolveMagnet（惰性磁力解析）-----------------------------------------

/// 详情页 `input#input-magnet` 的 **value 属性**里那条 195 字符的磁力 —— 源码里是 `&amp;`。
#[tokio::test]
async fn resolve_magnet_reads_the_magnet_from_the_input_value() {
    let url = common::oneshot(200, &fixture("zeromagnet-detail.html")).await;

    let out = zeromagnet::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error, None);
    let magnet = out.magnet.as_deref().expect("该拿到磁力");
    assert_eq!(magnet.len(), 195, "解码 &amp; → & 之后 195 字符");
    assert!(
        magnet.starts_with(&format!(
            "magnet:?xt=urn:btih:{FIRST_HASH}&dn=ubuntu-24.04.4-desktop-amd64.iso&tr="
        )),
        "{}",
        &magnet[..80]
    );
    assert!(!magnet.contains("&amp;"), "不该有双重转义");
    assert_eq!(magnet.matches("&tr=").count(), 2, "tracker 一个都不能少");

    // ⚠️ 照抄 JS：本站的 resolveMagnet **不返回** infoHash（尽管磁力里就有）
    assert_eq!(out.info_hash, None);
}

/// ⚠️ 与 megapeer 不同：**HTTP 错误原样透出**（JS 没有吞掉它）。
#[tokio::test]
async fn resolve_magnet_passes_through_the_http_error() {
    let url = common::oneshot(503, "nope").await;

    let out = zeromagnet::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.magnet, None);
    assert!(out.error.is_some());
    assert_ne!(out.error.as_deref(), Some("no_magnet_on_page"));
    assert_ne!(out.error.as_deref(), Some("no_html"));
}

/// 200 但是空 body → `no_html`。
#[tokio::test]
async fn resolve_magnet_treats_an_empty_body_as_no_html() {
    let url = common::oneshot(200, "").await;

    let out = zeromagnet::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_html"));
}

/// 详情页能打开但没有 `input#input-magnet` → `no_magnet_on_page`。
#[tokio::test]
async fn resolve_magnet_reports_no_magnet_on_page() {
    let url = common::oneshot(200, "<html><body><p>nothing</p></body></html>").await;

    let out = zeromagnet::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet_on_page"));
    assert_eq!(out.magnet, None);
    assert_eq!(out.info_hash, None);
}

/// 同级那条 `a[href^="magnet:"]`（复制按钮）不算数 —— JS 只认 `input#input-magnet`。
#[tokio::test]
async fn resolve_magnet_ignores_the_sibling_copy_link() {
    let html =
        format!("<html><body><a href=\"magnet:?xt=urn:btih:{FIRST_HASH}\">copy</a></body></html>");
    let url = common::oneshot(200, &html).await;

    let out = zeromagnet::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet_on_page"));
    assert_eq!(out.magnet, None);
}
