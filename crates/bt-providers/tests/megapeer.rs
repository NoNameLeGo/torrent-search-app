//! `src/providers/megapeer.js` 的对照测试。
//!
//! 全部离线：fixture 是**真实快照**，且是**原始 bytes** ——
//! `test/fixtures/megapeer-1080p.html`（结果页 50 条，windows-1251）
//! 与 `test/fixtures/megapeer-detail.html`（详情页，磁力 200 字符）。
//!
//! 期望值由 Node 端 `src/lib/normalize.js` 生成（不是手抄的）：
//! row0 `size=10329396347` / `"9.6 GB"`，row2 `size=37172941947` / `"34.6 GB"`。
//!
//! ## 本文件盯住的两处**有意 divergence**（见 `megapeer.rs` 文件头）
//!
//! 1. **编码**：JS 的 axios 把 win1251 字节硬按 utf8 解码 → 西里尔全变 U+FFFD；
//!    Rust 侧 reqwest 读 `charset=windows-1251` 正确解码（测试里把同样的
//!    响应头发给本地服务，走的就是生产同一条解码路径）。
//! 2. **列语义**：JS 固定列索引在 5-td 行上把评论数当 size；
//!    Rust 用「倒数第 2 格 = size、最后一格 = peers」修正。

mod common;

use bt_core::http::HttpClient;
use bt_providers::megapeer;

/// 真实站点响应头带 `charset=windows-1251` —— 本地服务也带，让 reqwest 真解码。
const HTML_1251: &str = "text/html; charset=windows-1251";

const FIRST_HASH: &str = "50b225ff7c72835068d71720ad789fa589f4179b";
const FIRST_DETAIL_HREF: &str =
    "/torrent/214934/30-nochej-s-bivshim_30-notti-con-il-mio-ex-2025-bdrip-1080p";
const FIRST_NAME: &str =
    "30 ночей с бывшим / 30 notti con il mio ex (2025) BDRip 1080p | Р | Кирилица";

#[tokio::test]
async fn parses_all_50_rows_from_the_real_fixture() {
    let url = common::oneshot_bytes(
        200,
        HTML_1251,
        &common::fixture_bytes("megapeer-1080p.html"),
    )
    .await;
    let base = url.trim_end_matches('/').to_string();

    let out = megapeer::search_at(&HttpClient::new(), &base, "1080p").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 50, "没有表头行，50 行就是 50 条");
}

#[tokio::test]
async fn first_row_fields_match_the_normalized_js_pipeline() {
    let url = common::oneshot_bytes(
        200,
        HTML_1251,
        &common::fixture_bytes("megapeer-1080p.html"),
    )
    .await;
    let base = url.trim_end_matches('/').to_string();

    let out = megapeer::search_at(&HttpClient::new(), &base, "1080p").await;
    let r = &out.results[0];

    assert_eq!(r.provider, "megapeer");
    assert_eq!(r.name, FIRST_NAME);
    assert_eq!(
        r.detail_url.as_deref(),
        Some(format!("{base}{FIRST_DETAIL_HREF}").as_str()),
        "detail_url 由 base + href 拼出（别把线上域名写死在断言里）"
    );
    assert_eq!(r.id, format!("megapeer:{FIRST_NAME}"));

    // 期望值来自 Node normalize.js（本地时区不同会让 dateText 差一天，见下）
    assert_eq!(r.size, Some(10_329_396_347));
    assert_eq!(r.size_text, "9.6 GB");
    assert_eq!(r.seeders, Some(1));
    assert_eq!(r.leechers, Some(17));
    assert!(r.date.is_some());
    assert!(
        r.date_text == "2026-10-11" || r.date_text == "2026-10-10",
        "parseDate 用本地零点、formatDate 按 UTC：CI(UTC)=2026-10-11，东八区=2026-10-10，意外: {}",
        r.date_text
    );

    assert!(r.needs_magnet, "列表页没有磁力");
    assert_eq!(r.magnet, None);
    assert_eq!(r.info_hash, None);
    assert_eq!(r.category, None);
    assert_eq!(r.files, None);
}

/// ⚠️ divergence：真实 fixture 里第 3 行（index 2）是 **5-td 形状**（带评论格）。
/// JS 的 `td:nth-child(3)` 会取到评论数 "1"；修正版必须拿到真正的大小。
#[tokio::test]
async fn divergence_the_5td_row_takes_the_real_size_not_the_comment_count() {
    let url = common::oneshot_bytes(
        200,
        HTML_1251,
        &common::fixture_bytes("megapeer-1080p.html"),
    )
    .await;
    let base = url.trim_end_matches('/').to_string();

    let out = megapeer::search_at(&HttpClient::new(), &base, "1080p").await;
    let r = &out.results[2];

    assert_eq!(r.name, "Тёмная материя / Dark Matter [2 сезон: 1-7 серия из 10] (2026) WEB-DL 1080p | LostFilm, HDRezka Studio, NewComers, ViruseProject, ColdFilm");
    assert_eq!(r.size, Some(37_172_941_947), "不能是评论数");
    assert_eq!(r.size_text, "34.6 GB", "JS 这里会显示 1.0 B");
    assert_eq!(r.seeders, Some(26), "JS 在 5-td 行上 peers 整体落空");
    assert_eq!(r.leechers, Some(101));
}

/// 末行（index 49）：win1251 里 `0x92` 是 **U+2019（’）**，
/// 名字里正好有一个 —— 三种解码器（Python / TextDecoder / encoding_rs）
/// 在这一字节上必须都给 `’`，否则探针真值会对不上。
#[tokio::test]
async fn last_row_keeps_the_cyrillic_name_and_the_u2019_apostrophe() {
    let url = common::oneshot_bytes(
        200,
        HTML_1251,
        &common::fixture_bytes("megapeer-1080p.html"),
    )
    .await;
    let base = url.trim_end_matches('/').to_string();

    let out = megapeer::search_at(&HttpClient::new(), &base, "1080p").await;
    let r = &out.results[49];

    assert!(r.name.contains("Месть Чарли Шоу"), "{}", r.name);
    assert!(r.name.contains('’'), "0x92 应解成 U+2019: {}", r.name);
    assert_eq!(r.size_text, "1.5 GB");
}

/// ⚠️ divergence 的另一半：**证明 JS 那条路（utf8 硬解码）拿到的就是 � 版文本**。
#[tokio::test]
async fn divergence_js_style_utf8_decoding_would_mangle_the_cyrillic() {
    // JS（axios 1.7.x）对文本响应一律 `buffer.toString('utf8')` —— 等效于：
    let lossy = String::from_utf8_lossy(&common::fixture_bytes("megapeer-1080p.html"));
    assert!(
        lossy.contains('\u{FFFD}'),
        "win1251 字节按 utf8 解必然产生替换字符 —— 这就是 JS 版标题的样子"
    );

    // Rust 侧走 charset 头 → reqwest → encoding_rs：正确文本
    let url = common::oneshot_bytes(
        200,
        HTML_1251,
        &common::fixture_bytes("megapeer-1080p.html"),
    )
    .await;
    let base = url.trim_end_matches('/').to_string();
    let out = megapeer::search_at(&HttpClient::new(), &base, "1080p").await;

    let name = &out.results[0].name;
    assert!(name.contains("ночей"), "{name}");
    assert!(!name.contains('\u{FFFD}'), "{name}");
}

/// 钉住请求 URL 契约（page 不进 URL —— JS 收了但没用）。
#[tokio::test]
async fn request_url_matches_the_js_contract() {
    let (url, seen) = common::oneshot_capture_bytes(
        200,
        HTML_1251,
        &common::fixture_bytes("megapeer-1080p.html"),
    )
    .await;
    let base = url.trim_end_matches('/').to_string();

    let _ = megapeer::search_at(&HttpClient::new(), &base, "ubuntu 22").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(
        raw.contains("/browse.php?search=ubuntu%2022&age=&cat=0&stype=0&sort=0&ascdesc=0"),
        "{raw}"
    );
}

/// 没有行 → `no_results_parsed`（这一站用 `_parsed` 后缀，与 oxtorrent 不同）。
#[tokio::test]
async fn page_without_rows_reports_no_results_parsed() {
    let url = common::oneshot(200, "<div id=\"index\"><table><tbody></tbody></table></div>").await;

    let out = megapeer::search_at(&HttpClient::new(), &url, "nonexistent-xyz").await;

    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
    assert!(out.results.is_empty());
}

/// 有行但一条都解析不出来 → **`error: None`**（JS 的 `error: null`）。
#[tokio::test]
async fn rows_present_but_unparseable_is_not_an_error() {
    let html = "<div id=\"index\"><table><tbody>\
                <tr class=\"table_fon\"><td>only one cell</td></tr>\
                </tbody></table></div>";
    let url = common::oneshot(200, html).await;

    let out = megapeer::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.error, None, "JS 这里返回的是 error: null");
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(503, "nope").await;

    let out = megapeer::search_at(&HttpClient::new(), &url, "1080p").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some(), "HTTP 错误要落进 error");
}

/// `search_with` 复刻 `runMirrors` —— JS 传的 name 是 **全小写 `'megapeer'`**。
#[tokio::test]
async fn search_wraps_the_error_like_run_mirrors() {
    let url = common::oneshot(200, "<html><body><p>no table</p></body></html>").await;
    let base = url.trim_end_matches('/');

    let out = megapeer::search_with(&HttpClient::new(), &[base], "nonexistent-xyz").await;

    assert_eq!(
        out.error.as_deref(),
        Some("megapeer unreachable (no_results_parsed)")
    );
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn search_with_returns_ok_when_results_are_non_empty() {
    let url = common::oneshot_bytes(
        200,
        HTML_1251,
        &common::fixture_bytes("megapeer-1080p.html"),
    )
    .await;
    let base = url.trim_end_matches('/');

    let out = megapeer::search_with(&HttpClient::new(), &[base], "1080p").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 50);
}

// ---- resolveMagnet（惰性磁力解析）-----------------------------------------

/// 详情页 `a[href^="magnet:?xt="]` 里那条 200 字符的磁力 —— 源玛里是 `&amp;`。
#[tokio::test]
async fn resolve_magnet_reads_the_magnet_from_the_detail_page() {
    let url = common::oneshot_bytes(
        200,
        HTML_1251,
        &common::fixture_bytes("megapeer-detail.html"),
    )
    .await;

    let out = megapeer::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error, None);
    let magnet = out.magnet.as_deref().expect("该拿到磁力");
    assert_eq!(magnet.len(), 200, "解码 &amp; → & 之后 200 字符");
    assert!(
        magnet.starts_with(&format!("magnet:?xt=urn:btih:{FIRST_HASH}&dn=")),
        "{}",
        &magnet[..80]
    );
    assert!(!magnet.contains("&amp;"), "不该有双重转义");
    assert_eq!(magnet.matches("&tr=").count(), 2, "tracker 一个都不能少");
    // 与 oxtorrent 不同：本站**带** infoHash（从磁力里抠）
    assert_eq!(out.info_hash.as_deref(), Some(FIRST_HASH));
}

/// ⚠️ 与 oxtorrent / torrent9 都不同：**HTTP 错误被统一吞成 `no_magnet`**。
#[tokio::test]
async fn resolve_magnet_swallows_http_errors_as_no_magnet() {
    let url = common::oneshot(503, "nope").await;

    let out = megapeer::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet"));
    assert_eq!(out.magnet, None);
    assert_eq!(out.info_hash, None);
}

#[tokio::test]
async fn resolve_magnet_treats_an_empty_body_as_no_magnet() {
    let url = common::oneshot(200, "").await;

    let out = megapeer::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet"));
}

#[tokio::test]
async fn resolve_magnet_reports_no_magnet_when_the_page_has_none() {
    let url = common::oneshot(200, "<html><body><p>nothing</p></body></html>").await;

    let out = megapeer::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error.as_deref(), Some("no_magnet"));
    assert_eq!(out.magnet, None);
}
