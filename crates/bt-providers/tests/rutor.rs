//! `src/providers/rutor.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实结果页快照**（`test/fixtures/rutor-ubuntu.html`，100 条）。
//!
//! 期望值不是手抄的 —— 数据行先与 cheerio 侧真值逐条核对
//! （`test/fixtures/html-probes.json` 的 `rows.count = 101`（含表头）、
//! `name.first_3_texts`、`name.attrs_first_3`、`magnet.first_href`、
//! `row_spans.first_3_rows`、`row_tds.first_3_rows`），
//! 再把每行喂给 `src/lib/normalize.js` 得到逐字段结果。
//!
//! ⚠️ 日期断言是**时区无关**的：`ruDate` 只把俄语月份换成英文（`30 Июн 26` → `30 Jun 26`），
//! 真正造时间戳的是 `normalize` 里的日期解析，而 JS 用的是**本地时区零点**。
//! 所以这里也用 `chrono::Local` 现算期望值（东八区是前一天 16:00Z，UTC 机器上就是当天零点）。
//! 实测：站上写 `30 Июн 26`，东八区解析出 `2026-06-29T16:00Z` → `dateText` 显示 `2026-06-29`。

mod common;

use bt_core::http::HttpClient;
use bt_providers::rutor;
use chrono::{Local, TimeZone};

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 fixture {}: {e}", p.display()))
}

/// `div#index > table > tbody > tr` 共 101 行，第一行是表头 → **100 条数据行**。
/// 101 与 100 两个数字都由 cheerio 侧交叉验证。
const EXPECTED_RESULTS: usize = 100;

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("rutor-ubuntu.html")).await;

    let out = rutor::search_at(&HttpClient::new(), &url, "ubuntu", 0).await;

    assert_eq!(out.error, None);
    assert_eq!(
        out.results.len(),
        EXPECTED_RESULTS,
        "101 行减去表头；数据行全部有片名 + 磁力"
    );
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("rutor-ubuntu.html")).await;

    let out = rutor::search_at(&HttpClient::new(), &url, "ubuntu", 0).await;
    let r = &out.results[0];

    assert_eq!(r.provider, "rutor");
    assert_eq!(r.name, "Rufus 4.15 (Build 2396) (2026) PC | Portable");
    assert_eq!(
        r.info_hash.as_deref(),
        Some("5c1d6707dade6bb1150ea1c7020a67cfa2b908f5")
    );
    assert_eq!(
        r.magnet.as_deref(),
        Some(
            "magnet:?xt=urn:btih:5c1d6707dade6bb1150ea1c7020a67cfa2b908f5\
             &dn=rutor.info&tr=udp://opentor.net:6969&tr=http://retracker.local/announce"
        )
    );
    assert_eq!(r.size, Some(11_408_507));
    assert_eq!(r.size_text, "10.9 MB");
    assert_eq!(r.seeders, Some(44));
    assert_eq!(r.leechers, Some(2));
    assert_eq!(r.category, None, "搜索列表没有逐条分类");
    assert!(!r.needs_magnet, "列表页自带磁力");
    assert_eq!(r.files, None);

    // 日期：站上是「30 Июн 26」→ 本地时区当日零点（见文件头说明）
    let expected_date = Local
        .with_ymd_and_hms(2026, 6, 30, 0, 0, 0)
        .single()
        .expect("合法日期")
        .timestamp_millis();
    assert_eq!(r.date, Some(expected_date));
}

#[tokio::test]
async fn rows_with_four_and_five_cells_are_both_parsed() {
    let url = common::oneshot(200, &fixture("rutor-ubuntu.html")).await;

    let out = rutor::search_at(&HttpClient::new(), &url, "ubuntu", 0).await;

    // [1] 是 5 个 td 的行（带评论数），[2] 是 4 个 td 的行
    let second = &out.results[1];
    assert_eq!(
        second.name,
        "WinPE 11-10-8 Sergei Strelec (x86/x64/Native x86) 2025.09.07 (2025) PC"
    );
    assert_eq!(
        second.info_hash.as_deref(),
        Some("98a9b7b93589cbca441455a7fcb2319c2ff59375")
    );
    assert_eq!(second.size_text, "4.6 GB");
    assert_eq!(second.seeders, Some(9));
    assert_eq!(second.leechers, Some(0));

    let third = &out.results[2];
    assert_eq!(third.name, "VA - Beach Deep House (2025) MP3");
    assert_eq!(
        third.info_hash.as_deref(),
        Some("66261ed980257b572c11d17e8b62c629b74cbc0e")
    );
    // ⚠️ 这一行只有 4 个 td —— size 必须按内容找才拿得到
    assert_eq!(third.size_text, "1.6 GB");
    assert_eq!(third.seeders, Some(9));
    assert_eq!(third.leechers, Some(0));
}

#[tokio::test]
async fn last_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("rutor-ubuntu.html")).await;

    let out = rutor::search_at(&HttpClient::new(), &url, "ubuntu", 0).await;
    let r = out.results.last().expect("应有 100 条");

    assert_eq!(
        r.name,
        "DVD приложение к журналу Хакер №03 (182) (март) (2014) PC"
    );
    assert_eq!(
        r.info_hash.as_deref(),
        Some("05439db6d96e892636378e5391040eca6ac95032")
    );
    assert_eq!(r.size_text, "7.8 GB");
    assert_eq!(r.seeders, Some(1));
    assert_eq!(r.leechers, Some(0));
}

/// 钉住请求 URL：`{base}/search/{page0}/0/010/2/{query}`。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("rutor-ubuntu.html")).await;

    let _ = rutor::search_at(&HttpClient::new(), &url, "ubuntu 22", 0).await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("/search/0/0/010/2/ubuntu%2022 "), "{raw}");
}

/// JS 的 `search()` 会 `coercePage(page) - 1` —— 所以「第 1 页」发出去的其实是 `search/0/`。
#[tokio::test]
async fn page_one_maps_to_zero_based_url() {
    let (url, seen) = common::oneshot_capture(200, &fixture("rutor-ubuntu.html")).await;
    let base = url.trim_end_matches('/');

    let _ = rutor::search_with(&HttpClient::new(), &[base], "ubuntu", 0).await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.contains("/search/0/0/010/2/"), "{raw}");
}

/// 只有表头（`rows.length <= 1`）→ `no_results_parsed`。
#[tokio::test]
async fn page_with_only_a_header_reports_no_results_parsed() {
    let url = common::oneshot(
        200,
        r#"<div id="index"><table><tbody>
             <tr><td>Добавлен</td><td>Название</td><td>Размер</td><td>Пиры</td></tr>
           </tbody></table></div>"#,
    )
    .await;

    let out = rutor::search_at(&HttpClient::new(), &url, "nonexistent-xyz", 0).await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

/// 完全没有 `div#index` 也算"解析不出结果"。
#[tokio::test]
async fn page_without_the_index_div_reports_no_results_parsed() {
    let url = common::oneshot(200, "<html><body><p>nothing</p></body></html>").await;

    let out = rutor::search_at(&HttpClient::new(), &url, "nonexistent-xyz", 0).await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(503, "nope").await;

    let out = rutor::search_at(&HttpClient::new(), &url, "ubuntu", 0).await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some(), "HTTP 错误要落进 error");
}

/// `search_with` 复刻 `runMirrors` —— 注意 JS 传的 name 是 **`'Rutor'`（首字母大写）**，
/// 错误串必须逐字一致。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><p>nothing</p></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = rutor::search_with(&HttpClient::new(), &[base], "nonexistent-xyz", 0).await;

    assert_eq!(
        out.error.as_deref(),
        Some("Rutor unreachable (no_results_parsed)"),
        "JS 的 runMirrors 会把各镜像错误拼接并加 provider 前缀"
    );
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn search_returns_ok_when_results_are_non_empty() {
    let url = common::oneshot(200, &fixture("rutor-ubuntu.html")).await;
    let base = url.trim_end_matches('/');

    let out = rutor::search_with(&HttpClient::new(), &[base], "ubuntu", 0).await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

/// 详情链接必须是**可点的** URL（对照 linuxtracker 那个少一个斜杠的死链 bug）。
#[tokio::test]
async fn detail_url_is_clickable() {
    let url = common::oneshot(200, &fixture("rutor-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = rutor::search_at(&HttpClient::new(), &url, "ubuntu", 0).await;

    for r in &out.results {
        let d = r.detail_url.as_deref().expect("每条都该有详情链接");
        assert!(
            d.starts_with(&format!("{base}/")),
            "不该拼出半截域名或双斜杠: {d}"
        );
    }
    assert!(out.results[0]
        .detail_url
        .as_deref()
        .unwrap()
        .ends_with("/torrent/915382/rufus-4.15-build-2396-2026-pc-portable"));
}

/// 本站列表页直接带磁力，所以 `needs_magnet` 应该恒为 false。
#[tokio::test]
async fn every_result_carries_a_magnet() {
    let url = common::oneshot(200, &fixture("rutor-ubuntu.html")).await;

    let out = rutor::search_at(&HttpClient::new(), &url, "ubuntu", 0).await;

    for r in &out.results {
        assert!(
            r.magnet
                .as_deref()
                .is_some_and(|m| m.starts_with("magnet:?")),
            "{}",
            r.name
        );
        assert!(!r.needs_magnet, "{}", r.name);
        assert!(r.info_hash.is_some(), "{}", r.name);
    }
}
