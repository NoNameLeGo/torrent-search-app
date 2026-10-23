//! `src/providers/nyaa.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实快照**，而且它是**第一个由 CI 抓回来的**
//! （`test/fixtures/nyaa-ubuntu.html`，本机对 `nyaa.si` 不通；
//! 走 `.github/workflows/fetch-fixture.yml`）。
//!
//! 期望值来自 cheerio 1.2.0 的真值（`test/fixtures/html-probes.json` 的
//! `rows.count = 1`、`rows.first_3_rows`（**8 个 td**，名字格带 `colspan="2"`）、
//! `view_links.*`、`cat_titles.attrs_first_3`（`Software - Applications`））。
//!
//! ## 2026-10-23：三处上游 bug 已修（JS + Rust 两版一起改）
//!
//! 之前这里钉的是「seeders=2009」「magnet=null」这类**荒诞值**（照抄上游）。
//! 但它们是数据级 bug：结果里没有磁力、size/date 全空、seeders 是年份。
//! 修法：磁力在**整行**里找、列索引改 3/4/5/6。因为两侧同轮改，
//! M6 差分门（同一份 fixture 下两边逐字段比）仍然一致。
//!
//! 所以现在断言的是**页面上真实的数据**：`624.0 MiB` / `2009-11-03` / `0` / 那条磁力。

mod common;

use bt_core::http::HttpClient;
use bt_providers::nyaa;
use common::fixture;

/// `table.torrent-list tbody tr` 只有 1 行（探活说的「2 行」是数 `<tr`、含表头）。
const EXPECTED_RESULTS: usize = 1;

const NAME: &str = "Koha Live CD Release 3 (3.0.4 Ubuntu 9.10 Desktop x86)";

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("nyaa-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = nyaa::search_at(&HttpClient::new(), &base, "ubuntu", 1).await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

#[tokio::test]
async fn the_single_row_now_carries_the_real_data() {
    let url = common::oneshot(200, &fixture("nyaa-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = nyaa::search_at(&HttpClient::new(), &base, "ubuntu", 1).await;
    let r = &out.results[0];

    assert_eq!(r.provider, "nyaa");
    assert_eq!(r.name, NAME);
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}/view/96659").as_str())
    );
    assert_eq!(r.category.as_deref(), Some("Software - Applications"));

    // 列索引修正后（3=Size / 4=Date / 5=Seeders / 6=Leechers）
    assert_eq!(r.size, Some(654_311_424), "624.0 MiB");
    assert_eq!(r.size_text, "624 MB");
    assert_eq!(r.date, Some(1_257_231_780_000), "2009-11-03 07:03 UTC");
    assert_eq!(r.date_text, "2009-11-03");
    assert_eq!(r.seeders, Some(0), "这一行真的是 0（不是 2009 了）");
    assert_eq!(r.leechers, Some(0));

    // 磁力现在拿得到（在 Link 格里找）
    let magnet = r.magnet.as_deref().unwrap_or_default();
    assert!(
        magnet.starts_with("magnet:?xt=urn:btih:45008e48c8800b7d7643337b2e70a634e4c69f6a"),
        "实际：{magnet}"
    );
    assert_eq!(
        r.info_hash.as_deref(),
        Some("45008e48c8800b7d7643337b2e70a634e4c69f6a")
    );
    assert!(!r.needs_magnet, "有磁力就不该再标 needs_magnet");
    // id 从 hash 派生（有 hash 时不再用名字）
    assert_eq!(r.id, "nyaa:45008e48c8800b7d7643337b2e70a634e4c69f6a");
    assert_eq!(r.files, None, "第 8 格是 downloads，JS 不读");
}

/// 钉住请求 URL：`{base}/?f=0&c=0_0&q=<query>&page=<page>`。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("nyaa-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = nyaa::search_at(&HttpClient::new(), &base, "ubuntu 22", 2).await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("/?f=0&c=0_0&q=ubuntu%2022&page=2 "), "{raw}");
}

/// JS 没过 `coercePage` —— 传 0 就发 `page=0`（别的 provider 会收敛成 1）。
#[tokio::test]
async fn page_zero_is_sent_as_is() {
    let (url, seen) = common::oneshot_capture(200, &fixture("nyaa-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = nyaa::search_at(&HttpClient::new(), &base, "ubuntu", 0).await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.contains("&page=0 "), "不该被收敛成 1: {raw}");
}

/// 没有结果行 → **空结果、`error: None`**（JS 是 `return { results: [] }`）。
#[tokio::test]
async fn no_rows_is_not_an_error() {
    let url = common::oneshot(
        200,
        "<html><body><table class=\"torrent-list\"><tbody></tbody></table></body></html>",
    )
    .await;

    let out = nyaa::search_at(&HttpClient::new(), &url, "nonexistent-xyz", 1).await;

    assert_eq!(out.error, None);
    assert!(out.results.is_empty());
}

/// HTTP 出错 → `NYAA unreachable (<错误串>)`（JS 自己拼的，不走 `runMirrors`）。
#[tokio::test]
async fn http_error_is_wrapped_with_the_sites_own_prefix() {
    let url = common::oneshot(503, "nope").await;

    let out = nyaa::search_at(&HttpClient::new(), &url, "ubuntu", 1).await;

    assert!(out.results.is_empty());
    let e = out.error.as_deref().expect("该有错误");
    assert!(e.starts_with("NYAA unreachable ("), "{e}");
    assert!(e.ends_with(')'), "{e}");
}

/// ⚠️ 200 但是空正文 → JS 的模板把 `undefined` 原样写进错误串。
#[tokio::test]
async fn an_empty_body_reports_the_literal_undefined() {
    let url = common::oneshot(200, "").await;

    let out = nyaa::search_at(&HttpClient::new(), &url, "ubuntu", 1).await;

    assert_eq!(out.error.as_deref(), Some("NYAA unreachable (undefined)"));
}

/// `search()` 用的是默认域名 `https://nyaa.si`（没有镜像、没有 runMirrors）。
#[test]
fn the_default_domain_is_nyaa_si() {
    assert_eq!(nyaa::BASE, "https://nyaa.si");
}
