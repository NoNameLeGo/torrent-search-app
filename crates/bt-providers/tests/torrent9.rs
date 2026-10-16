//! `src/providers/torrent9.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实快照**
//! （`test/fixtures/torrent9-ubuntu.html` 结果页 3 条 /
//! `test/fixtures/torrent9-detail.html` 详情页，861 字符的磁力链）。
//!
//! 期望值来自 cheerio 1.2.0 的真值（`test/fixtures/html-probes.json` 的
//! `rows.count = 3`、`rows.first_3_rows`、`names.count_nonempty_texts = 3`、
//! `names.first_3_texts`、`names.attrs_first_3`、`h1.first_text`、
//! `magnet.count = 2`、`magnet.first_href`）。
//!
//! ⚠️ 本站的搜索结果**只有名字**：站上第 2~5 格的日期/大小/seed/leech 被 JS 丢掉了
//! （见 `torrent9.rs` 文件头），所以断言里那些字段是 `None` / `—`，不是"懒得测"。

mod common;

use bt_core::http::HttpClient;
use bt_providers::torrent9;
use common::fixture;

/// `table > tbody > tr` 共 3 行数据。
const EXPECTED_RESULTS: usize = 3;

/// 磁力串里的 hash 是**大写**，而 `extractInfoHash` 出来的字段是小写。
const HASH_UPPER: &str = "FEBD9A2CB755EC82E6E7A015A8DC497FDE9DD507";
const HASH_LOWER: &str = "febd9a2cb755ec82e6e7a015a8dc497fde9dd507";

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("torrent9-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = torrent9::search_at(&HttpClient::new(), &base, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("torrent9-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = torrent9::search_at(&HttpClient::new(), &base, "ubuntu").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "torrent9");
    assert_eq!(
        r.name, "Ubuntu Ultimate Edition 1.4 DVD",
        "名字跨 <h3>/<span> 拼出来"
    );
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}/torrent/49542/ubuntu-ultimate-edition-1-4-dvd").as_str())
    );
    assert_eq!(
        r.id, "torrent9:Ubuntu Ultimate Edition 1.4 DVD",
        "没有 hash 时 id 用名字"
    );

    // 列表页什么元数据都不解析 → 等点击时去详情页捞磁力
    assert!(r.needs_magnet);
    assert_eq!(r.magnet, None);
    assert_eq!(r.info_hash, None);

    // ⚠️ 站上第 2~5 格有 30/08/2018、1.9Go、3、3 —— JS 全丢，卡片上只剩名字
    assert_eq!(r.size, None);
    assert_eq!(r.size_text, "—");
    assert_eq!(r.seeders, None);
    assert_eq!(r.leechers, None);
    assert_eq!(r.date, None);
    assert_eq!(r.date_text, "—");
    assert_eq!(r.category, None);
    assert_eq!(r.files, None);
}

#[tokio::test]
async fn all_rows_match_the_cheerio_oracle() {
    let url = common::oneshot(200, &fixture("torrent9-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = torrent9::search_at(&HttpClient::new(), &base, "ubuntu").await;

    let names: Vec<&str> = out.results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Ubuntu Ultimate Edition 1.4 DVD",
            "Super Ubuntu 2008.09",
            "Ubuntu 10.04 Desktop (32 bits)",
        ]
    );

    // 每条都该有可点的详情链接（对照 linuxtracker 那个少一个斜杠的死链 bug）
    for r in &out.results {
        let d = r.detail_url.as_deref().expect("每条都该有详情链接");
        assert!(d.starts_with(&format!("{base}/torrent/")), "{d}");
        assert!(r.needs_magnet, "{}", r.name);
    }
}

/// 钉住请求 URL：`{base}/search_torrent/<encodeURIComponent(query)>.html`。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("torrent9-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = torrent9::search_at(&HttpClient::new(), &base, "ubuntu 22").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("/search_torrent/ubuntu%2022.html "), "{raw}");
}

/// 没有行 → `no_results_parsed`。
#[tokio::test]
async fn page_without_rows_reports_no_results_parsed() {
    let url = common::oneshot(
        200,
        "<html><body><table><tbody></tbody></table></body></html>",
    )
    .await;

    let out = torrent9::search_at(&HttpClient::new(), &url, "nonexistent-xyz").await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

/// ⚠️ 与 therarbg / limetorrents 不同：**有行但一条都解析不出来时也报错**
/// （JS 写的是 `error: results.length === 0 ? 'no_results_parsed' : null`）。
#[tokio::test]
async fn rows_without_href_also_report_no_results_parsed() {
    let html = r#"<html><body><table><tbody>
        <tr><td><a>no href at all</a></td></tr>
      </tbody></table></body></html>"#;
    let url = common::oneshot(200, html).await;

    let out = torrent9::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(503, "nope").await;

    let out = torrent9::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some(), "HTTP 错误要落进 error");
}

/// `search_with` 复刻 `runMirrors` —— JS 传的是 **`'Torrent9'`**。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><p>no table</p></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = torrent9::search_with(&HttpClient::new(), &[base], "nonexistent-xyz").await;

    assert_eq!(
        out.error.as_deref(),
        Some("Torrent9 unreachable (no_results_parsed)")
    );
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn search_returns_ok_when_results_are_non_empty() {
    let url = common::oneshot(200, &fixture("torrent9-ubuntu.html")).await;
    let base = url.trim_end_matches('/');

    let out = torrent9::search_with(&HttpClient::new(), &[base], "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

// ---- resolveMagnet（惰性磁力解析）-------------------------------------------

/// 详情页里那条 861 字符的磁力链 —— 源玛里是**裸 `&`**（不是 `&amp;`）。
#[tokio::test]
async fn resolve_magnet_reads_the_magnet_from_the_detail_page() {
    let url = common::oneshot(200, &fixture("torrent9-detail.html")).await;

    let out = torrent9::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error, None);
    let magnet = out.magnet.as_deref().expect("该拿到磁力");
    assert_eq!(magnet.len(), 861, "与探针 magnet.first_href 等长");
    assert!(
        magnet.starts_with(&format!("magnet:?xt=urn:btih:{HASH_UPPER}&tr=")),
        "{}",
        &magnet[..90]
    );
    assert!(!magnet.contains("&amp;"), "不该有双重转义: {magnet}");
    assert_eq!(magnet.matches("&tr=").count(), 19, "tracker 一个都不能少");
    assert!(
        magnet.ends_with("&tr=http://inferno.demonoid.ph:3389/announce"),
        "尾部要和探针真值一致"
    );

    // infoHash 是**小写化**过的（磁力串里是大写）
    assert_eq!(out.info_hash.as_deref(), Some(HASH_LOWER));
}

/// ⚠️ 详情页缺 h1（名称）时，即使有磁力也报 `no_magnet` —— JS 的 `parseDetail` 守卫。
#[tokio::test]
async fn resolve_magnet_needs_the_h1_name_too() {
    let html =
        format!("<html><body><a href=\"magnet:?xt=urn:btih:{HASH_UPPER}\">dl</a></body></html>");
    let url = common::oneshot(200, &html).await;

    let out = torrent9::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet"));
    assert_eq!(out.magnet, None);
}

/// 有名称但没磁力 → `no_magnet`。
#[tokio::test]
async fn resolve_magnet_reports_no_magnet_when_the_link_is_missing() {
    let url = common::oneshot(
        200,
        "<html><body><div class=\"movie-section\"><h1>Some Movie</h1></div></body></html>",
    )
    .await;

    let out = torrent9::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet"));
    assert_eq!(out.magnet, None);
}

/// ⚠️ 与 `therarbg` 不同：**HTTP 错误也归到 `no_magnet`**（JS 把 fetch 的 error 吞了）。
#[tokio::test]
async fn resolve_magnet_swallows_http_errors_into_no_magnet() {
    let url = common::oneshot(503, "nope").await;

    let out = torrent9::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet"));
    assert_eq!(out.magnet, None);
    assert_eq!(out.info_hash, None);
}

/// 磁力里没有 btih 时：magnet 照给，`info_hash` 是 None，而且**不算错**。
#[tokio::test]
async fn resolve_magnet_returns_a_magnet_without_a_btih() {
    let html = "<html><body><div class=\"movie-section\"><h1>Some Movie</h1></div>\
                <a href=\"magnet:?dn=no-hash-here\">dl</a></body></html>";
    let url = common::oneshot(200, html).await;

    let out = torrent9::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error, None);
    assert_eq!(out.magnet.as_deref(), Some("magnet:?dn=no-hash-here"));
    assert_eq!(out.info_hash, None);
}
