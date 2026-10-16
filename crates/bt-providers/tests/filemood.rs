//! `src/providers/filemood.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实结果页快照**（`test/fixtures/filemood-ubuntu.html`，20 条）。
//!
//! 期望值不是手抄的 —— 数据行先由 cheerio 侧交叉验证
//! （`test/fixtures/html-probes.json` 里的 `title.first_3_texts` / `size.first_3_texts` /
//! `status.first_3_texts` / `detail_link.first_3_hrefs` 与 `rows.count = 65` / `btn_success.count = 20`），
//! 再把整行喂给 `src/lib/normalize.js` 得到逐字段结果。

mod common;

use bt_core::http::HttpClient;
use bt_providers::filemood;

use common::fixture;

/// `table > tbody > tr` 共 65 行，其中含 `a.btn-success` 的 20 行是数据行，
/// 且 20 行全部抠得出 infoHash —— 两个数字都由 cheerio 侧交叉验证。
const EXPECTED_RESULTS: usize = 20;

/// fixture 首条的详情路径（= cheerio 真值里的 `detail_link.first_href`）。
const FIRST_DETAIL_PATH: &str =
    "/ubuntu-26.04-desktop-amd64.iso-dafc8c076ca2f3ed376eeae7c76a0d6be2415c45.html";

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("filemood-ubuntu.html")).await;

    let out = filemood::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(
        out.results.len(),
        EXPECTED_RESULTS,
        "65 行里靠 a.btn-success 挑出 20 条数据行，全部能抠到 infoHash"
    );
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("filemood-ubuntu.html")).await;

    let out = filemood::search_at(&HttpClient::new(), &url, "ubuntu").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "filemood");
    assert_eq!(r.name, "ubuntu-26.04-desktop-amd64.iso");
    assert_eq!(r.id, "filemood:dafc8c076ca2f3ed376eeae7c76a0d6be2415c45");
    assert_eq!(
        r.info_hash.as_deref(),
        Some("dafc8c076ca2f3ed376eeae7c76a0d6be2415c45")
    );
    assert_eq!(r.size, Some(6_979_321_856));
    assert_eq!(r.size_text, "6.5 GB");
    assert_eq!(r.seeders, Some(2518));
    assert_eq!(r.leechers, Some(65));
    assert_eq!(r.date, None, "本 provider 不产出日期");
    assert_eq!(r.date_text, "—", "formatDate(null) 的产物，不是空串");
    assert_eq!(r.category.as_deref(), Some("Other"));

    // 详情链接 = 本次请求的 base + fixture 里的相对路径。
    // （离线测试的 base 是本地一次性服务，所以不能拿线上域名去断言。）
    let base = url.trim_end_matches('/');
    let expected = format!("{base}{FIRST_DETAIL_PATH}");
    assert_eq!(r.detail_url.as_deref(), Some(expected.as_str()));

    assert_eq!(
        r.magnet.as_deref(),
        Some(
            "magnet:?xt=urn:btih:dafc8c076ca2f3ed376eeae7c76a0d6be2415c45\
             &dn=ubuntu-26.04-desktop-amd64.iso"
        )
    );
    assert!(!r.needs_magnet);
    assert_eq!(r.files, None);
}

#[tokio::test]
async fn second_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("filemood-ubuntu.html")).await;

    let out = filemood::search_at(&HttpClient::new(), &url, "ubuntu").await;
    let r = &out.results[1];

    assert_eq!(r.name, "ubuntu-26.04-live-server-amd64.iso");
    assert_eq!(
        r.info_hash.as_deref(),
        Some("e1fc140a6391357fa1cf08ddb70274f9c05eb88b")
    );
    assert_eq!(r.size, Some(3_113_851_290));
    assert_eq!(r.size_text, "2.9 GB");
    assert_eq!(r.seeders, Some(1906));
    assert_eq!(r.leechers, Some(25));
}

#[tokio::test]
async fn last_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("filemood-ubuntu.html")).await;

    let out = filemood::search_at(&HttpClient::new(), &url, "ubuntu").await;
    let r = out.results.last().expect("应有 20 条");

    assert_eq!(r.name, "kubuntu-24.04.4-desktop-amd64.iso");
    assert_eq!(
        r.info_hash.as_deref(),
        Some("299671d28121049a9265be9062d503c4d8402cfb")
    );
    assert_eq!(r.size, Some(5_153_960_755));
    assert_eq!(r.size_text, "4.8 GB");
    assert_eq!(r.seeders, Some(202));
    assert_eq!(r.leechers, Some(3));
}

/// 钉住请求 URL：`${base}/result?q=<encodeURIComponent(query)>+in%3Atitle`。
/// 那个 `+` 是字面量、`%3A` 是 `:` 的编码，两者都不能被通用编码函数吃掉。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("filemood-ubuntu.html")).await;

    let _ = filemood::search_at(&HttpClient::new(), &url, "ubuntu linux").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(
        raw.contains("/result?q=ubuntu%20linux+in%3Atitle "),
        "{raw}"
    );
}

/// 页面里一行数据行都没有 → `no_results_parsed`（与 JS 的 `if (rows.length === 0)` 位置一致）。
#[tokio::test]
async fn page_without_data_rows_reports_no_results_parsed() {
    let url = common::oneshot(
        200,
        "<html><body><table><tr><td>x</td></tr></table></body></html>",
    )
    .await;

    let out = filemood::search_at(&HttpClient::new(), &url, "nonexistent-xyz").await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(503, "nope").await;

    let out = filemood::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some(), "HTTP 错误要落进 error");
}

/// ⚠️ 边界行为（照抄 JS，别"修"）：正文为空**且**没有 HTTP 错误时，
/// JS 返回 `{results: [], error: undefined}`，于是 `runMirrors` 的 `filter(Boolean)`
/// 把它滤掉，最终错误串是 `filemood unreachable ()` —— 一对空括号。
#[tokio::test]
async fn empty_body_without_an_error_yields_the_empty_mirror_error() {
    let url = common::oneshot(200, "").await;
    let base = url.trim_end_matches('/');

    let out = filemood::search_with(&HttpClient::new(), &[base], "ubuntu").await;

    assert_eq!(out.error.as_deref(), Some("filemood unreachable ()"));
    assert!(out.results.is_empty());
}

/// `search()` 走的是 `runMirrors`：**结果非空才算成功**，
/// 否则错误会被包装成 `filemood unreachable (<原因>)` —— 与 JS 逐字一致。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><table></table></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = filemood::search_with(&HttpClient::new(), &[base], "nonexistent-xyz").await;

    assert_eq!(
        out.error.as_deref(),
        Some("filemood unreachable (no_results_parsed)"),
        "JS 的 runMirrors 会拼接各镜像错误并加 provider 前缀"
    );
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn search_returns_ok_when_results_are_non_empty() {
    let url = common::oneshot(200, &fixture("filemood-ubuntu.html")).await;
    let base = url.trim_end_matches('/');

    let out = filemood::search_with(&HttpClient::new(), &[base], "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

/// 详情链接的 base 拼接：本站 href 以 `/` 开头，拼出来必须是**可点的** URL
/// （对照 linuxtracker 那个少一个斜杠的死链 bug）。
#[tokio::test]
async fn detail_url_is_clickable() {
    let url = common::oneshot(200, &fixture("filemood-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = filemood::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let first = out.results[0]
        .detail_url
        .as_deref()
        .expect("首条应有详情链接");
    assert_eq!(first, format!("{base}{FIRST_DETAIL_PATH}"));

    for r in &out.results {
        let d = r.detail_url.as_deref().expect("每条都该有详情链接");
        assert!(
            d.starts_with(&format!("{base}/")),
            "不该拼出半截域名或双斜杠: {d}"
        );
    }
}
