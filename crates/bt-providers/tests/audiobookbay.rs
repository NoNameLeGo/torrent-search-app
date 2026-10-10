//! `src/providers/audiobookbay.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实快照**
//! （`test/fixtures/audiobookbay-ubuntu.html` 结果页 9 条 /
//! `test/fixtures/audiobookbay-detail.html` 详情页 179KB）。
//!
//! 期望值不是手抄的 —— 结构数字与前三条的文本来自 cheerio 1.2.0 的真值
//! （`test/fixtures/html-probes.json` 的 `posts.count = 9`、`titles.count = 9`、
//! `titles.first_3_texts`、`titles.attrs_first_3`、`info.count = 9`、
//! `info.first_text`），字段结果再由 `src/lib/normalize.js` 实测输出对齐。
//!
//! ⚠️ **本站的 size / date 恒为空**（上游 bug，见 `audiobookbay.rs` 文件头），
//! 所以断言写的是 `size: None` / `size_text: "—"`，不是"懒得测"。
//!
//! 详情页那边只用了 `select("td")` / `text_trim` / `closest_tag` / `find` / `nth`
//! —— 全都被 `crates/bt-core/src/dom.rs` 的单测或探针钉住；
//! 期望的 hash 由探针 `cover.first_src` 独立给出（站点把 hash 写进封面图文件名）。

mod common;

use bt_core::http::HttpClient;
use bt_providers::audiobookbay;

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 fixture {}: {e}", p.display()))
}

/// `div.post` 共 9 条 —— 与 `posts.count` 一致。
const EXPECTED_RESULTS: usize = 9;

/// 探针 `cover.first_src` 里那个前缀，也是详情页 `<td>Info Hash:</td>` 后面那格的值。
const HASH: &str = "f759c8ef86a9ff389e7b965cf5037288aa5ec896";

const FIRST_NAME: &str =
    "Legion Nieśmiertelnych tomy 1-23 audiobook PL R.Siemianowski - Larson B. V.";
const FIRST_HREF: &str =
    "/abss/legion-niesmiertelnych-tomy-1-23-audiobook-pl-rsiemianowski-larson-b-v/";

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("audiobookbay-ubuntu.html")).await;

    let out = audiobookbay::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

#[tokio::test]
async fn titles_match_the_cheerio_oracle() {
    let url = common::oneshot(200, &fixture("audiobookbay-ubuntu.html")).await;

    let out = audiobookbay::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let names: Vec<&str> = out.results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names[0], FIRST_NAME);
    // ⚠️ 站上这里就是**两个**空格，cheerio 的 .text().trim() 也不折叠中间空白
    assert_eq!(
        names[1],
        "Testimony Therapy: Decolonizing Mental Health for Black Therapists and Clients  - Makungu M. Akinyela"
    );
    assert_eq!(
        names[2],
        "Good Karma Refuge for Elephants: Good Karma, Book 1 - David Michie"
    );
    assert_eq!(names.last(), Some(&"With A Little Help - Cory Doctorow"));
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("audiobookbay-ubuntu.html")).await;
    // 站点基址本来就不带尾斜杠（JS 的 DOMAINS 也是这样），测试里也去掉，
    // 否则拼出来是 `http://host//abss/...` 这种双斜杠
    let base = url.trim_end_matches('/').to_string();

    let out = audiobookbay::search_at(&HttpClient::new(), &base, "ubuntu").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "audiobookbay");
    assert_eq!(r.name, FIRST_NAME);
    let expected_url = format!("{base}{FIRST_HREF}");
    assert_eq!(r.detail_url.as_deref(), Some(expected_url.as_str()));
    assert_eq!(r.category.as_deref(), Some("Books"));

    // 列表页没有磁力 → 等用户点击时再解析
    assert_eq!(r.magnet, None);
    assert_eq!(r.info_hash, None);
    assert!(r.needs_magnet);
    assert_eq!(
        r.id,
        format!("audiobookbay:{FIRST_NAME}"),
        "没有 hash 时 id 用名字"
    );

    // ⚠️ 上游 bug：这段文本没有换行，size/date 两个字段都拿不到（见 provider 文件头）
    assert_eq!(r.size, None);
    assert_eq!(r.size_text, "—");
    assert_eq!(r.date, None);
    assert_eq!(r.date_text, "—");

    assert_eq!(r.seeders, None, "站点列表页不显示 peers");
    assert_eq!(r.leechers, None);
    assert_eq!(r.files, None);
}

#[tokio::test]
async fn last_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("audiobookbay-ubuntu.html")).await;

    let out = audiobookbay::search_at(&HttpClient::new(), &url, "ubuntu").await;
    let r = out.results.last().expect("应有 9 条");

    assert_eq!(r.name, "With A Little Help - Cory Doctorow");
    assert!(
        r.detail_url
            .as_deref()
            .is_some_and(|d| d.ends_with("/abss/withq-a-little-help-cory-doctorow/")),
        "{:?}",
        r.detail_url
    );
    assert!(r.needs_magnet);
}

/// 每条都该有**可点**的详情链接（对照 linuxtracker 那个少一个斜杠的死链 bug）。
#[tokio::test]
async fn every_result_is_magnetless_and_points_at_a_clickable_detail_url() {
    let url = common::oneshot(200, &fixture("audiobookbay-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = audiobookbay::search_at(&HttpClient::new(), &base, "ubuntu").await;

    for r in &out.results {
        assert!(r.needs_magnet, "{}", r.name);
        assert_eq!(r.magnet, None);
        let d = r.detail_url.as_deref().expect("每条都该有详情链接");
        assert!(
            d.starts_with(&format!("{base}/abss/")),
            "不该拼出半截域名或双斜杠: {d}"
        );
    }
}

/// 钉住请求 URL：`{base}/?s=<encodeURIComponent(query)>`（JS 用的是查询串，不是路径）。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("audiobookbay-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = audiobookbay::search_at(&HttpClient::new(), &base, "ubuntu 22").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("/?s=ubuntu%2022 "), "{raw}");
}

/// 页面里连一个 `div.post` 都没有 → `no_results_parsed`。
#[tokio::test]
async fn page_without_posts_reports_no_results_parsed() {
    let url = common::oneshot(200, "<html><body><p>nothing</p></body></html>").await;

    let out = audiobookbay::search_at(&HttpClient::new(), &url, "nonexistent-xyz").await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

/// 有 `div.post` 但一条都解析不出来（缺 href）也算解析失败。
#[tokio::test]
async fn posts_that_all_fail_to_parse_report_no_results_parsed() {
    let html = r#"<html><body>
        <div class="post"><div class="postTitle"><h2><a>no href</a></h2></div></div>
      </body></html>"#;
    let url = common::oneshot(200, html).await;

    let out = audiobookbay::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(503, "nope").await;

    let out = audiobookbay::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some(), "HTTP 错误要落进 error");
}

/// `search_with` 复刻 `runMirrors` —— JS 传的 name 是 **`'audiobookbay'`（全小写）**。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><p>nothing</p></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = audiobookbay::search_with(&HttpClient::new(), &[base], "nonexistent-xyz").await;

    assert_eq!(
        out.error.as_deref(),
        Some("audiobookbay unreachable (no_results_parsed)")
    );
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn search_returns_ok_when_results_are_non_empty() {
    let url = common::oneshot(200, &fixture("audiobookbay-ubuntu.html")).await;
    let base = url.trim_end_matches('/');

    let out = audiobookbay::search_with(&HttpClient::new(), &[base], "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

// ---- resolveMagnet（惰性磁力解析）-------------------------------------------

#[tokio::test]
async fn resolve_magnet_reads_the_info_hash_from_the_detail_page() {
    let url = common::oneshot(200, &fixture("audiobookbay-detail.html")).await;

    let out = audiobookbay::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error, None);
    assert_eq!(out.info_hash.as_deref(), Some(HASH));
    let expected_magnet = format!("magnet:?xt=urn:btih:{HASH}");
    assert_eq!(out.magnet.as_deref(), Some(expected_magnet.as_str()));
}

/// 详情页没有 `Info Hash:` 那一行 → `no_info_hash`（不区分原因，照抄 JS）。
#[tokio::test]
async fn resolve_magnet_reports_no_info_hash_when_the_label_is_missing() {
    let url = common::oneshot(
        200,
        "<html><body><table><tr><td>Piece Size:</td><td>2 MBs</td></tr></table></body></html>",
    )
    .await;

    let out = audiobookbay::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_info_hash"));
    assert_eq!(out.magnet, None);
    assert_eq!(out.info_hash, None);
}

/// 标签在，但后面那格不是 40 位 hex → 同样 `no_info_hash`。
#[tokio::test]
async fn resolve_magnet_rejects_a_value_that_is_not_a_40_hex_hash() {
    let url = common::oneshot(
        200,
        "<html><body><table><tr><td>Info Hash:</td><td>not-a-hash</td></tr></table></body></html>",
    )
    .await;

    let out = audiobookbay::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_info_hash"));
}

/// HTTP 出错时 JS 也只给 `no_info_hash`（`getInfoHash` 把 error 吞了）。
#[tokio::test]
async fn resolve_magnet_turns_an_http_error_into_no_info_hash() {
    let url = common::oneshot(503, "nope").await;

    let out = audiobookbay::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_info_hash"));
    assert_eq!(out.magnet, None);
}
