//! `src/providers/limetorrents.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实快照**（`test/fixtures/limetorrents-ubuntu.html`，41 行含表头）。
//!
//! 期望值不是手抄的 —— 行数/各格内容来自 cheerio 1.2.0 的真值
//! （`test/fixtures/html-probes.json` 的 `rows.count = 41`、`rows.first_3_rows`、
//! `names.count_nonempty_texts = 40`、`filelinks.count = 40`、`names.first_3_texts`、
//! `names.attrs_first_3`、`datecell.texts`），字节数与日期再喂给 `src/lib/normalize.js`
//! 拿 `sizeText` / `dateText`。
//!
//! ⚠️ 页面别处也有 `td:nth-child(2)` / `td.tdseed`（旁边速度榜的 `"6572 KB/Sec"`），
//! 所以**只有 `rows.first_3_rows`（按行取整行）才是这些格子的真值**。

mod common;

use bt_core::http::HttpClient;
use bt_providers::limetorrents;
use chrono::Utc;
use common::fixture;

/// 41 行减表头 → 40 条（每行都能抠到 itorrents 里的 info hash）。
const EXPECTED_RESULTS: usize = 40;

const FIRST_NAME: &str = "Clinton D., Negus C. Ubuntu Linux Bible 11ed 2025";
const FIRST_HASH: &str = "232cd67eb3ffbd7c37bf9ec3ee887417e5ae1ee6";
const FIRST_DETAIL: &str =
    "/Clinton-D --Negus-C -Ubuntu-Linux-Bible-11ed-2025-torrent-19390961.html";

/// 与 `bt_core::normalize` 里的 `MS_MONTH` 同一个值（"9 months ago" 就是减这个）。
const MS_MONTH: i64 = 2_629_800_000;

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("limetorrents-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = limetorrents::search_at(&HttpClient::new(), &base, "ubuntu", 1).await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS, "41 行含表头");
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("limetorrents-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = limetorrents::search_at(&HttpClient::new(), &base, "ubuntu", 1).await;
    let r = &out.results[0];

    assert_eq!(r.provider, "limetorrents");
    assert_eq!(r.name, FIRST_NAME);
    assert_eq!(r.info_hash.as_deref(), Some(FIRST_HASH));
    assert_eq!(
        r.id,
        format!("limetorrents:{FIRST_HASH}"),
        "有 hash 就用 hash"
    );
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}{FIRST_DETAIL}").as_str())
    );

    // 磁力是 normalize 用 info hash + 名称拼出来的（本站列表页没有磁力链）
    let magnet = r.magnet.as_deref().expect("有 hash 就该有磁力");
    assert!(
        magnet.starts_with(&format!("magnet:?xt=urn:btih:{FIRST_HASH}")),
        "{magnet}"
    );
    assert!(
        magnet.ends_with("&dn=Clinton%20D.%2C%20Negus%20C.%20Ubuntu%20Linux%20Bible%2011ed%202025"),
        "{magnet}"
    );
    assert!(!r.needs_magnet, "不需要惰性解析");

    assert_eq!(r.size, Some(8_556_380), "8.16 MB");
    assert_eq!(r.size_text, "8.2 MB");
    assert_eq!(r.seeders, Some(35));
    assert_eq!(r.leechers, Some(0));
    assert_eq!(r.category.as_deref(), Some("Other"));
    assert_eq!(r.files, None);

    // ⚠️ 站上写的是相对日期，这条是**相对今天**算的 → 只能带容差断言
    let expected = Utc::now().timestamp_millis() - 9 * MS_MONTH;
    let got = r.date.expect("\"9 months ago\" 该能解析");
    assert!(
        (got - expected).abs() < 120_000,
        "9 months ago 应约等于 now - 9×MS_MONTH：got={got} want≈{expected}"
    );
    assert_eq!(r.date_text.len(), 10, "解析出来就不该是 —：{}", r.date_text);
}

/// 站上的 `1 Year+` 解析不出日期 —— 钉住这个上游行为（大部分结果都是这样）。
#[tokio::test]
async fn a_truncated_date_leaves_the_field_empty() {
    let url = common::oneshot(200, &fixture("limetorrents-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = limetorrents::search_at(&HttpClient::new(), &base, "ubuntu", 1).await;
    let r = &out.results[1];

    assert_eq!(
        r.name,
        "VanDine K  The Ultimate Ubuntu Handbook  A complete guide to Ubuntu 24 04,  2025"
    );
    assert_eq!(r.size, Some(11_523_850), "10.99 MB");
    assert_eq!(r.size_text, "11.0 MB");
    assert_eq!(r.seeders, Some(15));
    assert_eq!(r.leechers, Some(1));
    assert_eq!(r.date, None, "\"1 Year+\" 没有 ago，parseDate 不认");
    assert_eq!(r.date_text, "—");
    // 站上写 `TV shows`/`E-books`，都对不上 JS 的 switch → Other
    assert_eq!(r.category.as_deref(), Some("Other"));
}

/// 第 3 行站上写的是 `TV shows` —— 同样落到 `Other`。
#[tokio::test]
async fn site_category_wording_never_matches_the_switch() {
    let url = common::oneshot(200, &fixture("limetorrents-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = limetorrents::search_at(&HttpClient::new(), &base, "ubuntu", 1).await;

    assert_eq!(
        out.results[2].name,
        "Celtics City S01E08 Chapter VIII Ubuntu 720p AMZN WEB-DL DDP5 1 H 264-RAWR EZTV"
    );
    assert_eq!(out.results[2].category.as_deref(), Some("Other"));
    // 整页扫一遍：一个非 Other 的分类都不该出现（switch 与站上措辞完全对不上）
    assert!(
        out.results
            .iter()
            .all(|r| r.category.as_deref() == Some("Other")),
        "实际出现过的分类: {:?}",
        out.results
            .iter()
            .map(|r| r.category.clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn every_result_carries_a_magnet_and_a_detail_link() {
    let url = common::oneshot(200, &fixture("limetorrents-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = limetorrents::search_at(&HttpClient::new(), &base, "ubuntu", 1).await;

    for r in &out.results {
        assert!(r.info_hash.is_some(), "{}", r.name);
        assert!(
            r.magnet
                .as_deref()
                .is_some_and(|m| m.starts_with("magnet:?")),
            "{}",
            r.name
        );
        assert!(!r.needs_magnet, "{}", r.name);
        let d = r.detail_url.as_deref().expect("每条都该有详情链接");
        assert!(
            d.starts_with(&format!("{base}/")),
            "不该拼出半截域名或双斜杠: {d}"
        );
    }
}

/// 钉住请求 URL：`{base}/search/all/<query>/date/<page>/`。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("limetorrents-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = limetorrents::search_at(&HttpClient::new(), &base, "ubuntu 22", 2).await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("/search/all/ubuntu%2022/date/2/ "), "{raw}");
}

#[tokio::test]
async fn page_without_rows_reports_no_results_parsed() {
    let url = common::oneshot(
        200,
        "<html><body><table class=\"table2\"></table></body></html>",
    )
    .await;

    let out = limetorrents::search_at(&HttpClient::new(), &url, "nonexistent-xyz", 1).await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(503, "nope").await;

    let out = limetorrents::search_at(&HttpClient::new(), &url, "ubuntu", 1).await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some(), "HTTP 错误要落进 error");
}

/// `search_with` 复刻 `runMirrors` —— JS 传的是 **`'LimeTorrents'`**。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><p>no table</p></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = limetorrents::search_with(&HttpClient::new(), &[base], "nonexistent-xyz", 1).await;

    assert_eq!(
        out.error.as_deref(),
        Some("LimeTorrents unreachable (no_results_parsed)")
    );
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn search_returns_ok_when_results_are_non_empty() {
    let url = common::oneshot(200, &fixture("limetorrents-ubuntu.html")).await;
    let base = url.trim_end_matches('/');

    let out = limetorrents::search_with(&HttpClient::new(), &[base], "ubuntu", 1).await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}
