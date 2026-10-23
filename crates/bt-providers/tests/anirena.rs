//! `src/providers/anirena.js` 的对照测试。
//!
//! fixture 是 **2026-10-23 由 CI 抓的真实结果页**
//! （`test/fixtures/anirena-one-piece.html`，580KB / 100 行）。本机对 anirena.com 不通。
//!
//! ## 这个站的两个特点
//!
//! 1. **磁力是惰性的**：行里的磁力锚点是个 **302 到 `magnet:` 的地址**，
//!    JS 在 `resolveMagnet()` 里 `maxRedirects: 0` 手动读 `Location`。搜索阶段不请求它。
//!    ⇒ 每条结果 `magnet: null` + `needsMagnet: true`（与 JS 一致）。
//! 2. **日期来自行属性** `data-created-ts`（unix **秒**、属性不带引号），不是格子文本。
//!
//! ## 测试强度的说明
//!
//! 本机没有 cheerio ⇒ 算不出"应几行" ⇒ 只断言**非空 + 每行结构完整**；
//! 逐字段一致性交给 M6 差分门（否则选择器写错会「两边都 0 行」假通过）。

mod common;

use bt_core::http::HttpClient;
use bt_providers::anirena;
use common::fixture;

/// 允许出现的标准分类（JS 的 `CATEGORY_MAP` 值域）。
const CATEGORIES: [&str; 7] = ["Anime", "Books", "Music", "Series", "Apps", "Porn", "Other"];

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("anirena-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = anirena::search_at(&HttpClient::new(), &base, "one piece").await;

    assert_eq!(out.error, None);
    assert!(!out.results.is_empty(), "0 条 ⇒ 选择器没命中");
    assert!(
        out.results.len() >= 50,
        "只解析出 {} 条，明显偏少（fixture 里有 100 行）",
        out.results.len()
    );
}

#[tokio::test]
async fn every_row_has_name_date_category_and_no_magnet() {
    let url = common::oneshot(200, &fixture("anirena-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = anirena::search_at(&HttpClient::new(), &base, "one piece").await;

    for r in &out.results {
        assert_eq!(r.provider, "anirena");
        assert!(!r.name.trim().is_empty(), "名字为空");
        assert!(r.date.is_some(), "{} 没有日期（data-created-ts）", r.name);
        let cat = r.category.as_deref().unwrap_or("");
        assert!(CATEGORIES.contains(&cat), "{} 的分类没映射：{cat}", r.name);
        // 磁力惰性：搜索阶段一律没有
        assert_eq!(r.magnet, None, "{} 不该在搜索阶段带磁力", r.name);
        assert!(r.needs_magnet, "{} 应该标 needs_magnet", r.name);
        // 详情页由名字链接拼出
        let d = r.detail_url.as_deref().unwrap_or_default();
        assert!(
            d.starts_with("http") || d.starts_with(&base),
            "detail_url 形状不对：{d}"
        );
    }
}

#[tokio::test]
async fn request_url_contract() {
    let (url, req) = common::oneshot_capture(200, &fixture("anirena-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = anirena::search_at(&HttpClient::new(), &base, "one piece").await;

    let raw = req.lock().unwrap().clone();
    assert!(raw.starts_with("GET /?"), "请求行：{raw}");
    assert!(raw.contains("q=one%20piece"), "关键词要编码：{raw}");
    assert!(raw.contains("page=1"), "page 写死在 URL 里：{raw}");
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = anirena::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 没有结果表 ⇒ 0 条 + 不算错误（JS 返回 `error: null`）。
#[tokio::test]
async fn page_without_rows_is_not_an_error() {
    let url = common::oneshot(200, "<html><body>nothing</body></html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = anirena::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}

// ---- resolve_magnet（惰性磁力） -------------------------------------------

const HASH: &str = "45008e48c8800b7d7643337b2e70a634e4c69f6a";

/// 详情页里的锚点会 302 到 `magnet:` —— 必须**不跟随**、自己读 `Location`。
#[tokio::test]
async fn resolve_magnet_follows_the_302_location() {
    let magnet = format!("magnet:?xt=urn:btih:{HASH}");
    let redirect = common::oneshot_redirect(&magnet).await;
    let detail = common::oneshot(
        200,
        &format!(
            r#"<html><body><div class="td-actions"><a href="{redirect}">m</a></div></body></html>"#
        ),
    )
    .await;

    let out = anirena::resolve_magnet(&HttpClient::new(), &detail).await;

    assert_eq!(out.error, None);
    assert_eq!(out.magnet.as_deref(), Some(magnet.as_str()));
    assert_eq!(out.info_hash.as_deref(), Some(HASH));
}

/// 详情页里没有那个锚点 ⇒ `no_magnet_link`（照抄 JS 的错误串）。
#[tokio::test]
async fn resolve_magnet_reports_no_magnet_link() {
    let detail = common::oneshot(200, "<html><body>nothing</body></html>").await;

    let out = anirena::resolve_magnet(&HttpClient::new(), &detail).await;

    assert_eq!(out.magnet, None);
    assert_eq!(out.error.as_deref(), Some("no_magnet_link"));
}

/// 302 的 `Location` 不是 `magnet:` ⇒ `no_magnet`。
#[tokio::test]
async fn resolve_magnet_reports_no_magnet_when_location_is_not_a_magnet() {
    let redirect = common::oneshot_redirect("https://example.com/not-a-magnet").await;
    let detail = common::oneshot(
        200,
        &format!(
            r#"<html><body><div class="td-actions"><a href="{redirect}">m</a></div></body></html>"#
        ),
    )
    .await;

    let out = anirena::resolve_magnet(&HttpClient::new(), &detail).await;

    assert_eq!(out.magnet, None);
    assert_eq!(out.error.as_deref(), Some("no_magnet"));
}
