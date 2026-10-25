//! `video` 模块（通用 MacCMS 适配器）的离线测试。
//!
//! fixture 是 **2026-10-25 本机抓的真实搜索页**：
//!
//! - `appmovie-search.html`（APP影院，stui 模板）
//! - `dbku-search.html`（独播库，myui 模板）
//! - `didahd-search.html`（嘀嗒影视，myui 模板）
//!
//! 断言只钉「结构完整 + 非空 + URL 去重」；逐字段一致性由真实站点复测，
//! 因为列表页的年份/分类本来就不在同一个位置（当前适配器也不猜）。

mod common;

use bt_core::http::HttpClient;
use bt_providers::video::{self, DIDAHd, APPMOVIE, DBKU};
use common::fixture;

/// 每条结果的公共断言：provider 对、标题非空、URL 是绝对地址、无重复。
fn assert_well_formed(out: &video::VideoSearchOutcome, provider: &str, base: &str) {
    assert_eq!(out.error, None);
    assert!(!out.results.is_empty(), "0 条 ⇒ 选择器没命中");

    let mut seen = std::collections::HashSet::new();
    for r in &out.results {
        assert_eq!(r.provider.as_str(), provider);
        assert!(!r.title.trim().is_empty(), "标题为空");
        assert!(
            r.detail_url.starts_with("http"),
            "detail_url 不是绝对地址：{}",
            r.detail_url
        );
        assert!(
            r.detail_url.starts_with(base) || r.detail_url.starts_with("https://"),
            "detail_url 跑到了别的站点：{}",
            r.detail_url
        );
        assert!(
            seen.insert(r.detail_url.clone()),
            "同一个 detail_url 出现了两次：{}",
            r.detail_url
        );
    }
}

#[tokio::test]
async fn parses_appmovie_fixture() {
    let url = common::oneshot(200, &fixture("appmovie-search.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = video::search_at(&HttpClient::new(), &base, &APPMOVIE, "流浪地球", 1).await;

    assert_well_formed(&out, "appmovie", &base);
    assert!(
        out.results.len() >= 2,
        "只解析出 {} 条，明显偏少",
        out.results.len()
    );
    assert!(
        out.results.iter().any(|r| r.title.contains("流浪地球")),
        "结果里没有命中词：{:?}",
        out.results.iter().map(|r| &r.title).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn parses_dbku_fixture() {
    let url = common::oneshot(200, &fixture("dbku-search.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = video::search_at(&HttpClient::new(), &base, &DBKU, "流浪地球", 1).await;

    assert_well_formed(&out, "dbku", &base);
    // 这份 fixture 里「流浪地球」只命中 1 条（独播库的搜索结果本来就少）。
    assert!(!out.results.is_empty());
}

#[tokio::test]
async fn parses_didahd_fixture() {
    let url = common::oneshot(200, &fixture("didahd-search.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = video::search_at(&HttpClient::new(), &base, &DIDAHd, "流浪地球", 1).await;

    assert_well_formed(&out, "didahd", &base);
    assert!(!out.results.is_empty());
}

/// 搜索 URL 契约：路径 + `wd=` 编码 + 第 2 页的 `&page=`。
#[tokio::test]
async fn request_url_contract() {
    let (url, req) = common::oneshot_capture(200, &fixture("appmovie-search.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = video::search_at(&HttpClient::new(), &base, &APPMOVIE, "流浪地球", 2).await;

    let raw = req.lock().unwrap().clone();
    assert!(
        raw.starts_with("GET /index.php/vod/search.html?"),
        "请求行：{raw}"
    );
    assert!(
        raw.contains("wd=%E6%B5%81%E6%B5%AA%E5%9C%B0%E7%90%83"),
        "关键词要 URL 编码：{raw}"
    );
    assert!(raw.contains("page=2"), "第 2 页要带 page=2：{raw}");
}

/// HTTP 错误 ⇒ `error` 有值、结果为空。
#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = video::search_at(&HttpClient::new(), &base, &APPMOVIE, "x", 1).await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 没有结果列表 ⇒ 0 条 + 不算错误。
#[tokio::test]
async fn page_without_rows_is_not_an_error() {
    let url = common::oneshot(200, "<html><body>nothing</body></html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = video::search_at(&HttpClient::new(), &base, &DBKU, "x", 1).await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}

/// 注册表顺序 = UI 顺序，且 id 不重复。
#[test]
fn registry_is_stable() {
    let ids: Vec<_> = video::registry().iter().map(|p| p.id()).collect();
    assert_eq!(ids, vec!["appmovie", "dbku", "didahd"]);
}
