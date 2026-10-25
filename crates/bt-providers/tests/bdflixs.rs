//! `bdflixs`（BD电影首发）的离线测试。
//!
//! fixture 是 2026-10-25 本机抓的真实页面：
//! - `bdflixs-search.html`（`/?s=流浪地球`，2 个影片条目）
//! - `bdflixs-detail.html`（`/594.html`，102 条磁力）
//!
//! 搜索只回影片条目（`needs_magnet`），磁力由 `resolve_magnet` 惰性取第一条 ——
//! 与项目已有的 6 家惰性 provider 同一条链路。

mod common;

use bt_core::http::HttpClient;
use bt_providers::bdflixs;
use common::fixture;

#[tokio::test]
async fn parses_the_real_search_fixture() {
    let url = common::oneshot(200, &fixture("bdflixs-search.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = bdflixs::search_at(&HttpClient::new(), &base, "流浪地球").await;

    assert_eq!(out.error, None);
    assert!(
        out.results.len() >= 2,
        "只解析出 {} 条，明显偏少（fixture 里有 2 个影片条目）",
        out.results.len()
    );

    for r in &out.results {
        assert_eq!(r.provider, "bdflixs");
        assert!(!r.name.trim().is_empty(), "名字为空");
        assert!(r.name.contains("流浪地球"), "标题里没有命中词：{}", r.name);
        // 搜索阶段不碰详情页：没有磁力、但有 detail_url
        assert_eq!(r.magnet, None, "{} 不该在搜索阶段带磁力", r.name);
        assert!(r.needs_magnet, "{} 应该标 needs_magnet", r.name);
        let d = r.detail_url.as_deref().unwrap_or_default();
        assert!(
            d.starts_with(&base),
            "detail_url 应该被本地化到测试 base：{d}"
        );
    }
}

#[tokio::test]
async fn request_url_contract() {
    let (url, req) = common::oneshot_capture(200, &fixture("bdflixs-search.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = bdflixs::search_at(&HttpClient::new(), &base, "流浪地球").await;

    let raw = req.lock().unwrap().clone();
    assert!(raw.starts_with("GET /?s="), "请求行：{raw}");
    assert!(
        raw.contains("%E6%B5%81%E6%B5%AA%E5%9C%B0%E7%90%83"),
        "关键词要 URL 编码：{raw}"
    );
}

#[tokio::test]
async fn resolve_magnet_takes_the_first_magnet_on_the_detail_page() {
    let url = common::oneshot(200, &fixture("bdflixs-detail.html")).await;

    let out = bdflixs::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.error, None);
    let magnet = out.magnet.as_deref().unwrap_or_default();
    assert!(
        magnet.starts_with("magnet:?xt=urn:btih:"),
        "磁力形状不对：{magnet}"
    );
    let hash = out.info_hash.as_deref().unwrap_or_default();
    assert_eq!(hash.len(), 40, "info_hash 应该是 40 位：{hash}");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "info_hash 不是十六进制：{hash}"
    );
}

#[tokio::test]
async fn resolve_magnet_reports_no_magnet() {
    let url = common::oneshot(200, "<html><body>nothing</body></html>").await;

    let out = bdflixs::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.magnet, None);
    assert_eq!(out.error.as_deref(), Some("no_magnet"));
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = bdflixs::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 没有结果条目 ⇒ 0 条 + 不算错误。
#[tokio::test]
async fn page_without_posts_is_not_an_error() {
    let url = common::oneshot(200, "<html><body>nothing</body></html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = bdflixs::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}
