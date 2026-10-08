//! `src/providers/internetarchive.js` 的对照测试。
//!
//! ⚠️ **这份 fixture 不是逐字节快照**（`internetarchive-ubuntu.synthetic.json`）：
//! - `docs[0..2]` 是 2026-10-08 从 CI 联网冒烟日志里取回的**真实 doc**（值全真）
//! - `docs[3..]` 是手工构造的**边界样本**（真实样本凑不齐"缺 btih / 缺 title /
//!   item_size=0 / 未知 mediatype / 缺 identifier"这些情况）
//!
//! 为什么不用完整真实响应：本机对 `archive.org` 有 DNS 污染（连续解析得到不同的假 IP，
//! IPv6 落在 Facebook 黑洞前缀 `2a03:2880:face:b00c`），直连 / 代理 / DoH / 中转全部失败。
//! 但真实响应已经**在 CI 上验证过**：`live-smoke` 取回 97 条、`error=None`，
//! 字段名与类型与本文件一致（`docs[0..2]` 就是证据）。
//!
//! 期望值仍然是**把这份 fixture 喂给 `src/lib/normalize.js` 跑出来的输出**。

mod common;

use bt_core::http::HttpClient;
use bt_providers::internetarchive;
use serde_json::Value;

const FIXTURE: &str = "internetarchive-ubuntu.synthetic.json";

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 fixture {}: {e}", p.display()))
}

/// 把"这份 fixture 不是逐字节快照"钉进测试 —— 谁想换成完整真响应，
/// 必须先删掉 `_synthetic` 标记并把文件名去掉 `.synthetic`，于是不可能"忘了这件事"。
#[test]
fn fixture_is_still_marked_as_synthetic() {
    let v: Value = serde_json::from_str(&fixture(FIXTURE)).expect("fixture 该是合法 JSON");
    let note = v
        .get("_synthetic")
        .and_then(Value::as_str)
        .expect("`_synthetic` 标记不能丢：这份 fixture 不是逐字节快照");
    assert!(
        note.contains("archive.org"),
        "标记里该写明为什么不是完整快照: {note}"
    );
}

// ---- 解析（含跳过规则） ---------------------------------------------------

#[tokio::test]
async fn parses_the_fixture_and_skips_rows_without_title_or_btih() {
    let url = common::oneshot(200, &fixture(FIXTURE)).await;

    let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(
        out.results.len(),
        8,
        "10 条 docs 里：缺 btih 的真实条目 1 条 + 缺 title 的边界样本 1 条，都要跳过"
    );

    // 第 0 条是**真实** doc（mediatype=software → Apps）
    let first = &out.results[0];
    assert_eq!(first.provider, "internetarchive");
    assert_eq!(first.name, "Ubuntu 7.10 Alpha 3 (Tribe 3) (Desktop, i386)");
    assert_eq!(
        first.id,
        "internetarchive:d2e790e7c1585e3c0a0271103b145837243eabe7"
    );
    assert_eq!(
        first.info_hash.as_deref(),
        Some("d2e790e7c1585e3c0a0271103b145837243eabe7")
    );
    assert_eq!(first.size, Some(726_611_530));
    assert_eq!(first.size_text, "693 MB");
    assert_eq!(
        first.date,
        Some(1_636_037_836_000),
        "publicdate 是 ISO 字符串 \"2021-11-04T14:57:16Z\""
    );
    assert_eq!(first.date_text, "2021-11-04");
    assert_eq!(
        first.category.as_deref(),
        Some("Apps"),
        "mediatype=software"
    );
    assert_eq!(
        first.detail_url.as_deref(),
        Some("https://archive.org/details/ubuntu-7.10-alpha3-desktop-i386")
    );
    assert!(!first.needs_magnet);
}

/// **真实** doc 的 title 里带 HTML 实体 `&#124;`（IA 自己不转义）。
///
/// JS 版原样透传、不 decode —— Rust 版保持一致。这不是我们的 bug，是两边相同的行为；
/// 哪天要解码，两版必须一起改，别只改一边。
#[tokio::test]
async fn upstream_html_entities_in_titles_are_passed_through_verbatim() {
    let url = common::oneshot(200, &fixture(FIXTURE)).await;

    let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let r = &out.results[1];
    assert!(
        r.name.contains("GeckoLinux &#124; This Week in Linux 30"),
        "实体原样保留（与 JS 版一致）: {}",
        r.name
    );
    assert!(
        r.magnet.as_deref().unwrap().contains("%26%23124%3B"),
        "拼进磁力链时被 URI 编码，也是原样透传"
    );
    assert_eq!(r.category.as_deref(), Some("Other"), "mediatype=audio");
}

/// IA 没有 tracker 统计 —— 全部结果的 seeders / leechers 都该是 None。
#[tokio::test]
async fn no_seeder_or_leecher_data_at_all() {
    let url = common::oneshot(200, &fixture(FIXTURE)).await;

    let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(
        out.results
            .iter()
            .all(|r| r.seeders.is_none() && r.leechers.is_none()),
        "IA 不提供 seeders/leechers"
    );
    assert!(
        out.results.iter().all(|r| r.files.is_none()),
        "IA 也不提供文件数"
    );
}

/// btih 前后带空白时要 trim（JS 是 `String(x).toLowerCase().trim()`）；大写也要小写化。
#[tokio::test]
async fn btih_is_lowercased_and_trimmed() {
    let url = common::oneshot(200, &fixture(FIXTURE)).await;

    let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let movies = out
        .results
        .iter()
        .find(|r| r.name == "Edge: Ubuntu Documentary 2024")
        .expect("该有这条");
    assert_eq!(
        movies.info_hash.as_deref(),
        Some("1111222233334444555566667777888899990000"),
        "fixture 里的原值前后各带两个空格"
    );

    let books = out
        .results
        .iter()
        .find(|r| r.name == "Edge: Ubuntu Linux Bible 11ed")
        .expect("该有这条");
    assert_eq!(
        books.info_hash.as_deref(),
        Some("abcdef0123456789abcdef0123456789abcdef01"),
        "原值是大写"
    );
}

// ---- 分类映射 -------------------------------------------------------------

#[test]
fn media_types_map_to_the_standard_buckets() {
    assert_eq!(
        internetarchive::media_type_category(Some("software")),
        "Apps"
    );
    assert_eq!(internetarchive::media_type_category(Some("texts")), "Books");
    assert_eq!(
        internetarchive::media_type_category(Some("movies")),
        "Movies"
    );
    // JS 的 switch 没有 audio / etree…，一律 default
    assert_eq!(internetarchive::media_type_category(Some("audio")), "Other");
    assert_eq!(internetarchive::media_type_category(Some("etree")), "Other");
    // 大小写敏感（JS 的 switch 也是）
    assert_eq!(
        internetarchive::media_type_category(Some("Software")),
        "Other"
    );
    assert_eq!(internetarchive::media_type_category(None), "Other");
    assert_eq!(internetarchive::media_type_category(Some("")), "Other");
}

#[tokio::test]
async fn each_fixture_row_gets_the_expected_category() {
    let url = common::oneshot(200, &fixture(FIXTURE)).await;

    let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let got: Vec<&str> = out
        .results
        .iter()
        .map(|r| r.category.as_deref().unwrap_or("<none>"))
        .collect();
    assert_eq!(
        got,
        vec![
            "Apps",   // 真实 doc：software
            "Other",  // 真实 doc：audio
            "Books",  // texts
            "Movies", // movies
            "Apps",   // Zero Size
            "Apps",   // Missing Size
            "Other",  // 未知 mediatype (etree)
            "Apps",   // No Identifier
        ],
    );
}

// ---- item_size 的三态 -----------------------------------------------------

/// ⚠️ 与 torrentscsv / yts 相反：这里 `0` 是**有效值**（JS 用 `!= null` 而非 falsy）。
#[tokio::test]
async fn item_size_zero_is_valid_but_missing_is_not() {
    let url = common::oneshot(200, &fixture(FIXTURE)).await;

    let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let zero = out
        .results
        .iter()
        .find(|r| r.name == "Edge: Zero Size")
        .expect("该有这条");
    assert_eq!(zero.size, Some(0), "JS 用的是 `!= null`，0 不该被丢掉");
    assert_eq!(zero.size_text, "0 B");

    let missing = out
        .results
        .iter()
        .find(|r| r.name == "Edge: Missing Size")
        .expect("该有这条");
    assert_eq!(missing.size, None);
    assert_eq!(missing.size_text, "—");
}

// ---- detailUrl ------------------------------------------------------------

#[tokio::test]
async fn missing_identifier_means_no_detail_url() {
    let url = common::oneshot(200, &fixture(FIXTURE)).await;

    let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let r = out
        .results
        .iter()
        .find(|r| r.name == "Edge: No Identifier")
        .expect("该有这条");
    assert_eq!(r.detail_url, None);
}

// ---- 请求契约 -------------------------------------------------------------

#[tokio::test]
async fn request_url_carries_the_full_fl_list() {
    let (url, seen) = common::oneshot_capture(200, &fixture(FIXTURE)).await;

    let _ = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu linux").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("advancedsearch.php?q=title"), "{raw}");
    assert!(raw.contains("ubuntu%20linux"), "查询要 URI 编码: {raw}");
    for field in [
        "title",
        "item_size",
        "publicdate",
        "mediatype",
        "identifier",
        "btih",
    ] {
        assert!(raw.contains(field), "fl[] 里少了 {field}: {raw}");
    }
    assert!(raw.contains("rows=100"), "{raw}");
    assert!(raw.contains("output=json"), "{raw}");
}

// ---- 空结果与失败路径 -----------------------------------------------------

/// `docs` 不是数组时报 `no_docs` 而**不是**空结果 —— JS 就是这么写的（唯一这么做的 provider）。
#[tokio::test]
async fn docs_missing_or_not_an_array_is_an_error_not_empty_results() {
    for body in [
        r#"{"responseHeader":{"status":0}}"#,
        r#"{"response":{"numFound":0}}"#,
        r#"{"response":{"docs":"nope"}}"#,
        r#"{"response":"nope"}"#,
        r"[1,2,3]",
    ] {
        let url = common::oneshot(200, body).await;
        let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;
        assert_eq!(out.error.as_deref(), Some("no_docs"), "body={body}");
        assert!(out.results.is_empty(), "body={body}");
    }
}

/// 真的没有命中时是空数组 → 正常返回，不是错误。
#[tokio::test]
async fn empty_docs_array_is_no_results_not_an_error() {
    let url = common::oneshot(200, r#"{"response":{"numFound":0,"docs":[]}}"#).await;
    let out = internetarchive::search_at(&HttpClient::new(), &url, "nonexistent-xyz-12345").await;

    assert_eq!(out.error, None);
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_becomes_internetarchive_unreachable() {
    let url = common::oneshot(403, "nope").await;
    let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("应当有 error");
    assert!(e.contains("internetarchive unreachable"), "{e}");
    assert!(e.contains("403"), "要带上状态码: {e}");
}

/// ⚠️ **有意偏离 JS**（divergence，别当 bug 修）。
///
/// 「200 + 非 JSON 正文」时：JS 里 axios 把正文当字符串塞进 `data`（非空串是 truthy），
/// 于是走到 `docs` 不是数组 → 报 **`no_docs`**。
/// Rust 版在 http 层就报 `internetarchive unreachable (invalid json: … | body: …)`。
///
/// 两者都是错误（不像 knaben 那样静默为空），只是文案不同；Rust 版多带了正文开头，
/// 对"Cloudflare 拦页 vs 站点改版"的区分更有用。
#[tokio::test]
async fn divergence_non_json_body_reports_a_parse_error_instead_of_no_docs() {
    let url = common::oneshot(200, "<html>Access Denied</html>").await;
    let out = internetarchive::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("应当有 error");
    assert!(e.contains("internetarchive unreachable"), "{e}");
    assert!(e.contains("Access Denied"), "错误里必须带原始正文开头: {e}");
    assert!(
        !e.contains("no_docs"),
        "JS 会给 no_docs，Rust 版刻意改成带正文的解析错误: {e}"
    );
}
