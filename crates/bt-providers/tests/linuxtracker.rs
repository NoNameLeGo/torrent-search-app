//! `src/providers/linuxtracker.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实结果页快照**（`test/fixtures/linuxtracker-linux.html`）。
//! 期望值不是手抄的 —— 关键数字先由 cheerio 侧算出（`test/fixtures/html-probes.json`
//! 里的 `main_only.nonempty_texts_count = 18`、前三行的单元格与 href），
//! 再喂给 `src/lib/normalize.js` 得到逐字段结果。
//!
//! ⚠️ 日期断言是**时区无关**的：JS 用本地时区构造 `DD/MM/YYYY` 的零点，
//! 所以这里也用 `chrono::Local` 算期望值（在东八区是前一天 16:00Z，在 UTC 机器上就是当天零点）。
//! 顺带说：这个行为意味着**站上 28/04/2026 在东八区会显示成 2026-04-27** —— JS 版既有 bug，本移植照抄。

mod common;

use bt_core::http::HttpClient;
use bt_core::normalize::format_date;
use bt_providers::linuxtracker;
use chrono::{Local, TimeZone};

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 fixture {}: {e}", p.display()))
}

/// fixture 里 `td.lista a[href*="torrent-details"]` 共 43 条，
/// 其中主表行 33 条（`href^="index.php"`），名字非空的 18 条 —— 都由 cheerio 侧交叉验证过。
const EXPECTED_RESULTS: usize = 18;

#[tokio::test]
async fn parses_the_real_fixture() {
    let body = fixture("linuxtracker-linux.html");
    let url = common::oneshot(200, &body).await;

    let out = linuxtracker::search_at(&HttpClient::new(), &url, "linux").await;

    assert_eq!(out.error, None);
    assert_eq!(
        out.results.len(),
        EXPECTED_RESULTS,
        "43 个候选 → 挡掉侧栏 2-td 行 → 再挡掉名字为空的行（展开的描述行）"
    );
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let body = fixture("linuxtracker-linux.html");
    let url = common::oneshot(200, &body).await;

    let out = linuxtracker::search_at(&HttpClient::new(), &url, "linux").await;
    let first = &out.results[0];

    assert_eq!(first.provider, "linuxtracker");
    assert_eq!(first.name, "Fedora-KDE-Desktop-Live-x86_64-44");
    assert_eq!(
        first.id,
        "linuxtracker:5a2759c487c21a692df3e521cbcb3df8731bfb5f"
    );
    assert_eq!(
        first.info_hash.as_deref(),
        Some("5a2759c487c21a692df3e521cbcb3df8731bfb5f"),
        "infoHash 来自详情链接的 id 参数"
    );
    assert_eq!(first.size, Some(3_371_549_327));
    assert_eq!(first.size_text, "3.1 GB");
    assert_eq!(first.seeders, Some(15));
    assert_eq!(first.leechers, Some(0));
    assert_eq!(first.category.as_deref(), Some("Apps"), "该站分类写死 Apps");
    // ⚠️ 这里只用 ends_with：base 是本地测试服务器的地址，相对链接会被拼到它后面
    // （真实运行时 base 是 https://linuxtracker.org）
    assert!(
        first.detail_url.as_deref().is_some_and(|u| u.ends_with(
            "/index.php?page=torrent-details&id=5a2759c487c21a692df3e521cbcb3df8731bfb5f"
        )),
        "详情链接是把页面里的相对 href 拼到 base 上: {:?}",
        first.detail_url
    );
    assert_eq!(first.files, None);

    // 日期：fixture 里这行是 "28/04/2026"
    let expected_ms = Local
        .with_ymd_and_hms(2026, 4, 28, 0, 0, 0)
        .single()
        .expect("合法日期")
        .timestamp_millis();
    assert_eq!(
        first.date,
        Some(expected_ms),
        "与 JS 的 new Date(y, m-1, d) 一致"
    );
    assert_eq!(first.date_text, format_date(Some(expected_ms)));
}

#[tokio::test]
async fn second_and_third_rows_too() {
    let body = fixture("linuxtracker-linux.html");
    let url = common::oneshot(200, &body).await;

    let out = linuxtracker::search_at(&HttpClient::new(), &url, "linux").await;

    let second = &out.results[1];
    assert_eq!(second.name, "kali-linux-2026.2-installer-amd64.iso");
    assert_eq!(
        second.info_hash.as_deref(),
        Some("83f92aecfa3d92d3df79a5661ad8efb57282b48b")
    );
    assert_eq!(second.size, Some(4_799_625_953));
    assert_eq!(second.size_text, "4.5 GB");
    assert_eq!(second.seeders, Some(14));

    let third = &out.results[2];
    assert_eq!(third.name, "debian-13.6.0-amd64-DVD-1.iso");
    assert_eq!(
        third.info_hash.as_deref(),
        Some("204e02b76378fc6e76f8b09d2ede2e3332136b3c")
    );
    assert_eq!(third.size, Some(3_994_319_585));
    assert_eq!(third.seeders, Some(12));
}

/// 全部结果都该有 infoHash 和磁力链（该站详情链接必带 id）。
#[tokio::test]
async fn every_result_has_a_magnet() {
    let body = fixture("linuxtracker-linux.html");
    let url = common::oneshot(200, &body).await;

    let out = linuxtracker::search_at(&HttpClient::new(), &url, "linux").await;

    for r in &out.results {
        assert!(r.info_hash.is_some(), "{} 缺 infoHash", r.name);
        assert!(
            r.magnet
                .as_deref()
                .is_some_and(|m| m.starts_with("magnet:?")),
            "{} 缺磁力链",
            r.name
        );
        assert!(!r.needs_magnet, "有 infoHash 就不需要惰性解析");
        assert_eq!(r.category.as_deref(), Some("Apps"));
    }
}

/// 「展开的描述行」被保留，但字段是退化的 —— **JS 也是这个结果**，别当 bug 修。
#[tokio::test]
async fn expanded_description_row_keeps_degraded_fields_like_js() {
    let body = fixture("linuxtracker-linux.html");
    let url = common::oneshot(200, &body).await;

    let out = linuxtracker::search_at(&HttpClient::new(), &url, "linux").await;

    let Some(r) = out
        .results
        .iter()
        .find(|r| r.name == "4MLinux 52 0 core iso")
    else {
        panic!("该有这条（19 个 td 的展开行，名字非空所以会保留）");
    };
    assert_eq!(
        r.size, None,
        "该行的 tds[4] 是 \"Size: 16.13 MB\"，解析不出数字"
    );
    assert_eq!(r.size_text, "—");
    assert_eq!(r.seeders, None);
    assert_eq!(r.leechers, None);
    assert_eq!(r.date, None);
    assert_eq!(r.date_text, "—");
}

// ---- 请求契约 -------------------------------------------------------------

#[tokio::test]
async fn request_url_encodes_the_query() {
    let body = fixture("linuxtracker-linux.html");
    let (url, seen) = common::oneshot_capture(200, &body).await;

    let _ = linuxtracker::search_at(&HttpClient::new(), &url, "ubuntu linux").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(
        raw.contains("/index.php?page=torrents&search=ubuntu%20linux&category=0&active=0 "),
        "{raw}"
    );
}

// ---- 失败路径 -------------------------------------------------------------

#[tokio::test]
async fn http_error_is_passed_through_unchanged() {
    let url = common::oneshot(500, "nope").await;
    let out = linuxtracker::search_at(&HttpClient::new(), &url, "linux").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("应当有 error");
    // JS 直接抛出 axios 的文案，不加自己的前缀
    assert!(e.contains("500"), "{e}");
    assert!(!e.contains("linuxtracker"), "JS 不改写错误文案: {e}");
}

/// ⚠️ **有意偏离 JS**（divergence，别当回退改掉）：
/// JS 把相对 href 拼到没有结尾斜杠的 base 上，得到
/// `https://linuxtracker.orgindex.php?page=torrent-details&id=…` —— 死链，点开 404。
/// Rust 版补斜杠，得到正常可点的详情页。
#[test]
fn divergence_detail_url_is_not_the_js_dead_link() {
    let body = fixture("linuxtracker-linux.html");
    let results = linuxtracker::parse("https://linuxtracker.org", &body);

    assert_eq!(
        results[0].detail_url.as_deref(),
        Some(
            "https://linuxtracker.org/index.php?page=torrent-details\
             &id=5a2759c487c21a692df3e521cbcb3df8731bfb5f"
        )
    );
    assert!(
        !results[0]
            .detail_url
            .as_deref()
            .unwrap()
            .contains("orgindex.php"),
        "JS 版就是 orgindex.php 这种死链"
    );
}

#[tokio::test]
async fn page_without_results_reports_no_results_parsed() {
    let url = common::oneshot(200, "<html><body><p>nothing</p></body></html>").await;
    let out = linuxtracker::search_at(&HttpClient::new(), &url, "nonexistent-xyz").await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

/// `search()` 走的是 `runMirrors`：**结果非空才算成功**，
/// 否则错误会被包装成 `linuxtracker unreachable (<原因>)` —— 与 JS 逐字一致。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><p>nothing</p></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = linuxtracker::search_with(&HttpClient::new(), &[base], "nonexistent-xyz").await;

    assert_eq!(
        out.error.as_deref(),
        Some("linuxtracker unreachable (no_results_parsed)"),
        "JS 的 runMirrors 会把各镜像错误拼接并加 provider 前缀"
    );
    assert!(out.results.is_empty());
}

/// 有结果时 `search()` 不该带 error。
#[tokio::test]
async fn search_returns_ok_when_results_are_non_empty() {
    let body = fixture("linuxtracker-linux.html");
    let url = common::oneshot(200, &body).await;
    let base = url.trim_end_matches('/');

    let out = linuxtracker::search_with(&HttpClient::new(), &[base], "linux").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}
