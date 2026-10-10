//! `src/providers/therarbg.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实快照**
//! （`test/fixtures/therarbg-ubuntu.html` 结果页 39 条 /
//! `test/fixtures/therarbg-detail.html` 详情页，磁力链只有 1 条）。
//!
//! 期望值不是手抄的 —— 结构数字与前三行的各字段来自 cheerio 1.2.0 的真值
//! （`test/fixtures/html-probes.json` 的 `rows.count = 39`、`rows.first_3_rows`、
//! `name.count_nonempty_texts = 40`、`name.first_3_texts`、`name.attrs_first_3`、
//! `size.attrs_first_3`、`date.attrs_first_3`、`cat.first_3_texts`、
//! `seeders.first_3_texts`、`leechers.first_3_texts`），字节数/时间戳再喂给
//! `src/lib/normalize.js` 拿到 `sizeText` / `dateText`。
//!
//! ⚠️ **39 行、40 个名称链接**：第 28 行有两个 `<a>`（post-detail + IMDb 徽章），
//! JS 的 `.first()` 取第一个，所以结果仍然是 39 条。

mod common;

use bt_core::http::HttpClient;
use bt_providers::therarbg;
use common::fixture;

/// `table > tbody > tr.list-entry` 共 39 行，每行都能解析出名称 + 详情链接。
const EXPECTED_RESULTS: usize = 39;

const FIRST_NAME: &str = "Ubuntu Linux Bible 11E by David Clinton epub Nonfiction";
const FIRST_HREF: &str =
    "/post-detail/810de9/ubuntu-linux-bible-11e-by-david-clinton-epub-nonfiction/";

/// 详情页里那条磁力链的 info hash（探针 `magnet.first_href` 的 `btih:` 部分）。
const HASH: &str = "45BB7B3AC77116EF4E7EC743E2A8319CADDF87B1";

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("therarbg-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = therarbg::search_at(&HttpClient::new(), &base, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS, "39 行，全部可解析");
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("therarbg-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = therarbg::search_at(&HttpClient::new(), &base, "ubuntu").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "therarbg");
    assert_eq!(r.name, FIRST_NAME);
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}{FIRST_HREF}").as_str())
    );
    assert_eq!(r.size, Some(8_493_465), "字节数来自 data-order");
    assert_eq!(r.size_text, "8.1 MB");
    assert_eq!(r.seeders, Some(149));
    assert_eq!(r.leechers, Some(3));
    assert_eq!(
        r.date,
        Some(1_764_263_429_000),
        "data-order 是 unix 秒 ×1000"
    );
    assert_eq!(r.date_text, "2025-11-27");
    assert_eq!(r.category.as_deref(), Some("Other"));
    assert_eq!(
        r.id,
        format!("therarbg:{FIRST_NAME}"),
        "没有 hash 时 id 用名字"
    );

    // 列表页一条磁力都没有（探针 magnet.count = 0）→ 等点击时去详情页捞
    assert_eq!(r.magnet, None);
    assert_eq!(r.info_hash, None);
    assert!(r.needs_magnet);
    assert_eq!(r.files, None);
}

#[tokio::test]
async fn second_row_is_the_iso_and_maps_apps() {
    let url = common::oneshot(200, &fixture("therarbg-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = therarbg::search_at(&HttpClient::new(), &base, "ubuntu").await;
    let r = &out.results[1];

    assert_eq!(r.name, "ubuntu-24.04.1-desktop-amd64.iso");
    assert_eq!(r.size, Some(6_203_355_136));
    assert_eq!(r.size_text, "5.8 GB");
    assert_eq!(r.seeders, Some(119));
    assert_eq!(r.leechers, Some(11));
    assert_eq!(r.date_text, "2026-04-23");
    assert_eq!(r.category.as_deref(), Some("Apps"));
}

/// ⚠️ 站上的分类是 `E-books`，而 JS 的 `switch` 只认 `Books` → 落到 `Other`。
/// 这条把「分类映射是有损的」钉住（不是 bug，是照抄）。
#[tokio::test]
async fn unknown_site_categories_fall_back_to_other() {
    let url = common::oneshot(200, &fixture("therarbg-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = therarbg::search_at(&HttpClient::new(), &base, "ubuntu").await;
    let r = &out.results[2];

    assert_eq!(
        r.name,
        "Ubuntu Facile Collezione Anno 2022 PDF Ita by Ciliegia85"
    );
    assert_eq!(r.category.as_deref(), Some("Other"), "站上写的是 E-books");
    assert_eq!(r.size_text, "133 MB");
    assert_eq!(r.date_text, "2025-03-09");
}

/// 第 28 行（下标 27）在两个 `<a>` 里必须取第一个 —— 否则名称会变成 IMDb 徽章。
#[tokio::test]
async fn the_row_with_two_links_uses_the_first_one() {
    let url = common::oneshot(200, &fixture("therarbg-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = therarbg::search_at(&HttpClient::new(), &base, "ubuntu").await;
    let r = &out.results[27];

    assert_eq!(
        r.name,
        "Celtics City S01E08 Chapter VIII Ubuntu 720p AMZN WEB DL DDP5 1 H 264 RAWR EZTV"
    );
    assert!(
        r.detail_url
            .as_deref()
            .is_some_and(|d| d.ends_with("/post-detail/7879f1/")),
        "{:?}",
        r.detail_url
    );
    assert!(
        !r.detail_url.as_deref().unwrap().contains("imdb-detail"),
        "不能取到 IMDb 徽章那条"
    );
}

#[tokio::test]
async fn last_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("therarbg-ubuntu.html")).await;

    let out = therarbg::search_at(&HttpClient::new(), &url, "ubuntu").await;
    let r = out.results.last().expect("应有 39 条");

    assert_eq!(r.name, "UwUntu-22.10-desktop-amd64");
    assert!(r
        .detail_url
        .as_deref()
        .is_some_and(|d| d.ends_with("/post-detail/8a6cff/uwuntu-22-10-desktop-amd64/")));
    assert_eq!(r.size, Some(5_045_443_558));
    assert_eq!(r.size_text, "4.7 GB");
    assert_eq!(r.date_text, "2026-04-23");
    assert_eq!(r.category.as_deref(), Some("Apps"));
}

#[tokio::test]
async fn every_result_has_peers_and_a_clickable_detail_url() {
    let url = common::oneshot(200, &fixture("therarbg-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = therarbg::search_at(&HttpClient::new(), &base, "ubuntu").await;

    for r in &out.results {
        assert!(r.needs_magnet, "{}", r.name);
        assert_eq!(r.magnet, None);
        assert!(r.seeders.is_some(), "{} 缺 seeders", r.name);
        assert!(r.leechers.is_some(), "{} 缺 leechers", r.name);
        assert!(r.size.is_some(), "{} 缺 size", r.name);
        assert!(r.date.is_some(), "{} 缺 date", r.name);
        let d = r.detail_url.as_deref().expect("每条都该有详情链接");
        assert!(
            d.starts_with(&format!("{base}/post-detail/")),
            "不该拼出半截域名或双斜杠: {d}"
        );
    }
}

/// 钉住请求 URL：`{base}/get-posts/keywords:<encodeURIComponent(query)>`。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("therarbg-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = therarbg::search_at(&HttpClient::new(), &base, "ubuntu 22").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("/get-posts/keywords:ubuntu%2022 "), "{raw}");
}

#[tokio::test]
async fn page_without_list_rows_reports_no_results_parsed() {
    let url = common::oneshot(200, "<html><body><table></table></body></html>").await;

    let out = therarbg::search_at(&HttpClient::new(), &url, "nonexistent-xyz").await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

/// ⚠️ 与 `audiobookbay` 的关键差异：**有行、但一条都解析不出来时 error 是 `None`**。
///
/// 于是 `runMirrors` 会拼出一个空括号的错误串 —— 这条把那个怪癖钉住。
#[tokio::test]
async fn rows_present_but_unparseable_is_not_an_error() {
    let html = r#"<html><body><table><tbody>
        <tr class="list-entry"><td class="cellName"><div><a>no href</a></div></td></tr>
      </tbody></table></body></html>"#;
    let url = common::oneshot(200, html).await;

    let out = therarbg::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.error, None, "JS 这里返回的是 error: null");
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(503, "nope").await;

    let out = therarbg::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some(), "HTTP 错误要落进 error");
}

/// `search_with` 复刻 `runMirrors` —— JS 传的是 **`'TheRarBg'`（驼峰大写）**。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><p>no table</p></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = therarbg::search_with(&HttpClient::new(), &[base], "nonexistent-xyz").await;

    assert_eq!(
        out.error.as_deref(),
        Some("TheRarBg unreachable (no_results_parsed)")
    );
    assert!(out.results.is_empty());
}

/// 上一个测试的另一半：全镜像都「正常但没结果」时，错误串是空括号。
#[tokio::test]
async fn all_mirrors_empty_without_error_gives_empty_parens() {
    let html = r#"<html><body><table><tbody>
        <tr class="list-entry"><td class="cellName"><div><a>no href</a></div></td></tr>
      </tbody></table></body></html>"#;
    let url = common::oneshot(200, html).await;
    let base = url.trim_end_matches('/');

    let out = therarbg::search_with(&HttpClient::new(), &[base], "x").await;

    assert_eq!(out.error.as_deref(), Some("TheRarBg unreachable ()"));
}

#[tokio::test]
async fn search_returns_ok_when_results_are_non_empty() {
    let url = common::oneshot(200, &fixture("therarbg-ubuntu.html")).await;
    let base = url.trim_end_matches('/');

    let out = therarbg::search_with(&HttpClient::new(), &[base], "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

// ---- resolveMagnet（惰性磁力解析）-------------------------------------------

/// 详情页里那条 `a[href^="magnet:?"]`，源玛里是 `&amp;`，取出来必须是解码后的单个 `&`。
#[tokio::test]
async fn resolve_magnet_reads_the_magnet_from_the_detail_page() {
    let url = common::oneshot(200, &fixture("therarbg-detail.html")).await;

    let out = therarbg::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error, None);
    let magnet = out.magnet.as_deref().expect("该拿到磁力");
    assert!(
        magnet.starts_with(&format!("magnet:?xt=urn:btih:{HASH}&dn=")),
        "{magnet}"
    );
    assert!(magnet.contains("&dn=Ubuntu%20Linux%20Bible"), "{magnet}");
    assert!(!magnet.contains("&amp;"), "实体必须已解码: {magnet}");
    assert_eq!(
        magnet.matches("&tr=").count(),
        14,
        "tracker 一个不能少（探针 magnet.first_href 长度 848）"
    );
    assert!(
        magnet.ends_with("&tr=udp%3A%2F%2Ftracker.qu.ax%3A6969%2Fannounce"),
        "尾部要和探针真值一致: {magnet}"
    );

    // ⚠️ 照抄 JS：本站的 resolveMagnet **不返回** infoHash
    assert_eq!(out.info_hash, None);
}

/// 详情页能打开但没有磁力链 → `no_magnet_on_page`。
#[tokio::test]
async fn resolve_magnet_reports_no_magnet_on_page() {
    let url = common::oneshot(200, "<html><body><p>nothing here</p></body></html>").await;

    let out = therarbg::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet_on_page"));
    assert_eq!(out.magnet, None);
    assert_eq!(out.info_hash, None);
}

/// HTTP 出错 → 把错误串原样交出去（**不是** `no_magnet_on_page`）。
#[tokio::test]
async fn resolve_magnet_passes_through_the_http_error() {
    let url = common::oneshot(503, "nope").await;

    let out = therarbg::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.magnet, None);
    assert!(out.error.is_some());
    assert_ne!(
        out.error.as_deref(),
        Some("no_magnet_on_page"),
        "这一站跟 audiobookbay 不一样，HTTP 错误会原样透出"
    );
}

/// 200 但是空 body → JS 的 `!html` 分支，错误串是 `no_html`。
#[tokio::test]
async fn resolve_magnet_treats_an_empty_body_as_no_html() {
    let url = common::oneshot(200, "").await;

    let out = therarbg::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_html"));
}
