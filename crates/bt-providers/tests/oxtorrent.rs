//! `src/providers/oxtorrent.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实快照**
//! （`test/fixtures/oxtorrent-ubuntu.html` 结果页 3 条 /
//! `test/fixtures/oxtorrent-detail.html` 详情页，861 字符的磁力链）。
//!
//! 期望值来自 cheerio 1.2.0 的真值（`test/fixtures/html-probes.json` 的
//! `rows.count = 3`（**无表头行**）、`rows.first_3_rows`、`names.count_nonempty_texts = 3`、
//! `names.first_3_texts`、`names.attrs_first_3`、`cat_classes.attrs_first_3`（全是 `Logiciels`）、
//! `btn_magnet_links.count = 1`、`btn_magnet_links.first_href`）。
//!
//! ⚠️ 本站搜索结果**只有名字**：同行的 `700.4 MB` / `9` / `2` 和 `<i class="Logiciels">`
//! 都被 JS 丢掉了（见 `oxtorrent.rs` 文件头），所以那些断言写的是 `None` / `—`。

mod common;

use bt_core::http::HttpClient;
use bt_providers::oxtorrent;
use common::fixture;

/// `table > tbody > tr` 共 3 行，**这一站没有表头**，所以 3 行就是 3 条。
const EXPECTED_RESULTS: usize = 3;

const HASH_UPPER: &str = "574034E0EC52BA3BF08B5C58AB4A5DEE0627FA89";

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("oxtorrent-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = oxtorrent::search_at(&HttpClient::new(), &base, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(
        out.results.len(),
        EXPECTED_RESULTS,
        "没有表头，3 行就是 3 条"
    );
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("oxtorrent-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = oxtorrent::search_at(&HttpClient::new(), &base, "ubuntu").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "oxtorrent");
    assert_eq!(r.name, "Ubuntu 10.04 Desktop (32 bits)");
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}/torrent/48951/ubuntu-10-04-desktop-32-bits").as_str())
    );
    assert_eq!(r.id, "oxtorrent:Ubuntu 10.04 Desktop (32 bits)");

    assert!(r.needs_magnet, "列表页没有磁力");
    assert_eq!(r.magnet, None);
    assert_eq!(r.info_hash, None);

    // ⚠️ 同一行的 700.4 MB / 9 / 2 与分类 `Logiciels` 都被 JS 丢掉
    assert_eq!(r.size, None);
    assert_eq!(r.size_text, "—");
    assert_eq!(r.seeders, None);
    assert_eq!(r.leechers, None);
    assert_eq!(r.date, None);
    assert_eq!(r.date_text, "—");
    assert_eq!(r.category, None, "站上其实标了 Logiciels，JS 没用");
    assert_eq!(r.files, None);
}

#[tokio::test]
async fn all_rows_match_the_cheerio_oracle() {
    let url = common::oneshot(200, &fixture("oxtorrent-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = oxtorrent::search_at(&HttpClient::new(), &base, "ubuntu").await;

    let names: Vec<&str> = out.results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Ubuntu 10.04 Desktop (32 bits)",
            "Super Ubuntu 2008.09",
            "Ubuntu Ultimate Edition 1.4 DVD",
        ]
    );

    for r in &out.results {
        let d = r.detail_url.as_deref().expect("每条都该有详情链接");
        assert!(d.starts_with(&format!("{base}/torrent/")), "{d}");
        assert!(r.needs_magnet, "{}", r.name);
    }
}

/// 钉住请求 URL：`{base}/recherche/<encodeURIComponent(query)>`（**没有页码**）。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("oxtorrent-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = oxtorrent::search_at(&HttpClient::new(), &base, "ubuntu 22").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("/recherche/ubuntu%2022 "), "{raw}");
}

/// 没有行 → ⚠️ 这一站的错误串是 **`no_results`**（不带 `_parsed`）。
#[tokio::test]
async fn page_without_rows_reports_the_sites_own_error_string() {
    let url = common::oneshot(
        200,
        "<html><body><table><tbody></tbody></table></body></html>",
    )
    .await;

    let out = oxtorrent::search_at(&HttpClient::new(), &url, "nonexistent-xyz").await;

    assert_eq!(out.error.as_deref(), Some("no_results"));
    assert!(out.results.is_empty());
}

/// ⚠️ 有行但一条都解析不出来 → **`error: None`**（JS 的 `error: null`）。
#[tokio::test]
async fn rows_present_but_unparseable_is_not_an_error() {
    let html = r#"<html><body><table><tbody>
        <tr><td><a>no href</a></td></tr>
      </tbody></table></body></html>"#;
    let url = common::oneshot(200, html).await;

    let out = oxtorrent::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.error, None, "JS 这里返回的是 error: null");
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(503, "nope").await;

    let out = oxtorrent::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some(), "HTTP 错误要落进 error");
}

/// `search_with` 复刻 `runMirrors` —— JS 传的是 **`'OxTorrent'`**。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><p>no table</p></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = oxtorrent::search_with(&HttpClient::new(), &[base], "nonexistent-xyz").await;

    assert_eq!(
        out.error.as_deref(),
        Some("OxTorrent unreachable (no_results)")
    );
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn search_returns_ok_when_results_are_non_empty() {
    let url = common::oneshot(200, &fixture("oxtorrent-ubuntu.html")).await;
    let base = url.trim_end_matches('/');

    let out = oxtorrent::search_with(&HttpClient::new(), &[base], "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

// ---- resolveMagnet（惰性磁力解析）-------------------------------------------

/// 详情页 `div.btn-magnet > a` 里那条 861 字符的磁力 —— 源玛里是**裸 `&`**。
#[tokio::test]
async fn resolve_magnet_reads_the_magnet_from_the_detail_page() {
    let url = common::oneshot(200, &fixture("oxtorrent-detail.html")).await;

    let out = oxtorrent::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error, None);
    let magnet = out.magnet.as_deref().expect("该拿到磁力");
    assert_eq!(magnet.len(), 861, "与探针 btn_magnet_links.first_href 等长");
    assert!(
        magnet.starts_with(&format!("magnet:?xt=urn:btih:{HASH_UPPER}&tr=")),
        "{}",
        &magnet[..90]
    );
    assert!(!magnet.contains("&amp;"), "不该有双重转义");
    assert_eq!(magnet.matches("&tr=").count(), 19, "tracker 一个都不能少");

    // ⚠️ 照抄 JS：本站的 resolveMagnet **不返回** infoHash（尽管磁力里就有）
    assert_eq!(out.info_hash, None);
}

/// 详情页能打开但没有 `div.btn-magnet > a` → `no_magnet_on_page`。
#[tokio::test]
async fn resolve_magnet_reports_no_magnet_on_page() {
    let url = common::oneshot(200, "<html><body><p>nothing</p></body></html>").await;

    let out = oxtorrent::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet_on_page"));
    assert_eq!(out.magnet, None);
    assert_eq!(out.info_hash, None);
}

/// 同级那个 `btn-download` 不算数 —— JS 只认 `div.btn-magnet > a`。
#[tokio::test]
async fn resolve_magnet_ignores_the_sibling_btn_download_block() {
    let html = format!(
        "<html><body><div class=\"btn-download\"><a href=\"magnet:?xt=urn:btih:{HASH_UPPER}\">dl</a></div></body></html>"
    );
    let url = common::oneshot(200, &html).await;

    let out = oxtorrent::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet_on_page"));
    assert_eq!(out.magnet, None);
}

/// ⚠️ 与 `torrent9` 不同：**HTTP 错误原样透出**（JS 没有吞掉它）。
#[tokio::test]
async fn resolve_magnet_passes_through_the_http_error() {
    let url = common::oneshot(503, "nope").await;

    let out = oxtorrent::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.magnet, None);
    assert!(out.error.is_some());
    assert_ne!(out.error.as_deref(), Some("no_magnet_on_page"));
    assert_ne!(out.error.as_deref(), Some("no_html"));
}

/// 200 但是空 body → `no_html`。
#[tokio::test]
async fn resolve_magnet_treats_an_empty_body_as_no_html() {
    let url = common::oneshot(200, "").await;

    let out = oxtorrent::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_html"));
}
