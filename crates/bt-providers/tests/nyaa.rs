//! `src/providers/nyaa.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实快照**，而且它是**第一个由 CI 抓回来的**
//! （`test/fixtures/nyaa-ubuntu.html`，本机对 `nyaa.si` 不通；
//! 走 `.github/workflows/fetch-fixture.yml`）。
//!
//! 期望值来自 cheerio 1.2.0 的真值（`test/fixtures/html-probes.json` 的
//! `rows.count = 1`、`rows.first_3_rows`（**8 个 td**，名字格带 `colspan="2"`）、
//! `view_links.*`、`cat_titles.attrs_first_3`（`Software - Applications`）、
//! `namecell_magnets.count = 0`、`td3..td6.texts`），再由 `src/lib/normalize.js`
//! 实测出 `sizeText` / `dateText` / `seeders`。
//!
//! ⚠️ **本站的点：JS 的列索引全是错的**（`colspan` 把评论列合掉了，一行只有 8 个 td，
//! 而 JS 按 9 列读）→ `sizeText`/`dateText` 空、`seeders` 变成 2009。
//! 测试把这些荒诞值也钉住，免得以后有人以为是 Rust 侧写错（详见 `nyaa.rs` 文件头）。

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
async fn the_single_row_matches_the_js_pipeline_bug_for_bug() {
    let url = common::oneshot(200, &fixture("nyaa-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = nyaa::search_at(&HttpClient::new(), &base, "ubuntu", 1).await;
    let r = &out.results[0];

    assert_eq!(r.provider, "nyaa");
    assert_eq!(r.name, NAME);
    assert_eq!(r.id, format!("nyaa:{NAME}"));
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}/view/96659").as_str())
    );
    assert_eq!(r.category.as_deref(), Some("Software - Applications"));

    // ⚠️ 列错位：size 读到空的 Link 格、date 读到 "624.0 MiB"
    assert_eq!(r.size, None);
    assert_eq!(r.size_text, "—");
    assert_eq!(r.date, None);
    assert_eq!(r.date_text, "—");

    // ⚠️ seeders 读到日期串 "2009-11-03 07:03" → parseInt = 2009（JS 实测就是这个数）
    assert_eq!(r.seeders, Some(2009));
    assert_eq!(r.leechers, Some(0), "读到的是真 seeders，这一行恰好也是 0");

    // ⚠️ 磁力在 Link 格，而 JS 在名字格里找 → 恒空
    assert_eq!(r.magnet, None);
    assert_eq!(r.info_hash, None);
    assert!(r.needs_magnet);
    assert_eq!(r.files, None);

    // 真正的数据其实都在页面上（探针 rows.first_3_rows 钉着）：
    //   td[2] 里有 magnet:?xt=urn:btih:45008e48c8800b7d7643337b2e70a634e4c69f6a
    //   td[3] = "624.0 MiB"、td[4] = "2009-11-03 07:03"、td[5] = 真 seeders
    let html = fixture("nyaa-ubuntu.html");
    assert!(
        html.contains("45008e48c8800b7d7643337b2e70a634e4c69f6a"),
        "磁力确实在页面上（只是在 Link 格，JS 找错格）"
    );
    assert!(html.contains("624.0 MiB"), "真 size 在页面上");
    assert!(html.contains("2009-11-03 07:03"), "真日期在页面上");
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
