//! `cilixiong`（磁力熊）的离线测试。
//!
//! fixture 是 2026-10-25 本机抓的真实页面：
//! - `cilixiong-search.html`（POST `keyboard=流浪地球&show=title&tempid=1&classid=1,2`，3 条）
//! - `cilixiong-detail.html`（`/movie/4763.html`，4 条磁力）
//!
//! ⚠️ 搜索**必须**是表单 POST：GET 同一组参数会返回「信息提示 / 没有搜索到」。
//! 测试用 `oneshot_capture` 钉住请求行与 form body。

mod common;

use bt_core::http::HttpClient;
use bt_providers::cilixiong;
use common::fixture;

#[tokio::test]
async fn parses_the_real_search_fixture() {
    let url = common::oneshot(200, &fixture("cilixiong-search.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = cilixiong::search_at(&HttpClient::new(), &base, "流浪地球").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 3, "fixture 里是 3 条");

    for r in &out.results {
        assert_eq!(r.provider, "cilixiong");
        assert!(!r.name.trim().is_empty(), "名字为空");
        assert!(r.name.contains("流浪地球"), "标题里没有命中词：{}", r.name);
        assert_eq!(r.magnet, None, "{} 不该在搜索阶段带磁力", r.name);
        assert!(r.needs_magnet, "{} 应该标 needs_magnet", r.name);
        let d = r.detail_url.as_deref().unwrap_or_default();
        assert!(
            d.starts_with(&base),
            "detail_url 应该被本地化到测试 base：{d}"
        );
    }
}

/// POST 契约：请求行 + form body 的四个字段。
#[tokio::test]
async fn request_contract_is_form_post() {
    let (url, req) = common::oneshot_capture(200, &fixture("cilixiong-search.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = cilixiong::search_at(&HttpClient::new(), &base, "流浪地球").await;

    let raw = req.lock().unwrap().clone();
    assert!(raw.starts_with("POST /e/search/index.php"), "请求行：{raw}");
    assert!(
        raw.to_lowercase()
            .contains("content-type: application/x-www-form-urlencoded"),
        "必须是表单 POST：{raw}"
    );
    assert!(
        raw.contains("keyboard=%E6%B5%81%E6%B5%AA%E5%9C%B0%E7%90%83"),
        "关键词要 URL 编码：{raw}"
    );
    assert!(raw.contains("show=title"), "show 字段：{raw}");
    assert!(raw.contains("tempid=1"), "tempid 字段：{raw}");
    assert!(raw.contains("classid=1%2C2"), "classid 必须是 1,2：{raw}");
}

#[tokio::test]
async fn resolve_magnet_takes_the_first_magnet_on_the_detail_page() {
    let url = common::oneshot(200, &fixture("cilixiong-detail.html")).await;

    let out = cilixiong::resolve_magnet(&HttpClient::new(), &url).await;

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

    let out = cilixiong::resolve_magnet(&HttpClient::new(), &url).await;

    assert_eq!(out.magnet, None);
    assert_eq!(out.error.as_deref(), Some("no_magnet"));
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = cilixiong::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 没有结果卡片 ⇒ 0 条 + 不算错误。
#[tokio::test]
async fn page_without_cards_is_not_an_error() {
    let url = common::oneshot(200, "<html><body>nothing</body></html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = cilixiong::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}
