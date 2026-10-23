//! `src/providers/nekobt.js` 的对照测试。
//!
//! fixture 是 **2026-10-23 由 CI 抓的真实结果页**
//! （`test/fixtures/nekobt-one-piece.html`，2.0MB，Svelte 渲染的 Tailwind 表格）。
//!
//! ## 为什么这里没有"精确条数"断言
//!
//! 本机没有 `cheerio` ⇒ HTML provider 的 Node 侧只能在 CI 跑 ⇒ **算不出应有多少行**。
//! 与其瞎猜一个数，不如：
//! ① 断言**非空** —— 这防的是"选择器写错 ⇒ 两边都 0 行 ⇒ 差分门 0==0 假通过"
//!    （`compare.cjs` 只比条数与字段，两边都空是它拦不住的）；
//! ② 断言每行结构完整（磁力 / 名字 / 40 位 hash）；
//! ③ 精确的逐字段一致性交给 **M6 差分门**（同一份 fixture，与 Node 版逐字段比）。

mod common;

use bt_core::http::HttpClient;
use bt_providers::nekobt;
use common::fixture;

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("nekobt-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = nekobt::search_at(&HttpClient::new(), &base, "one piece").await;

    assert_eq!(out.error, None);
    assert!(
        !out.results.is_empty(),
        "0 条 ⇒ 选择器没命中（`table.table > tbody > tr` 那条链要复核）"
    );
    // 一页 50 行是站上的常态；给个下限，别让"只解析出 1 行"也算通过
    assert!(
        out.results.len() >= 20,
        "只解析出 {} 条，明显偏少",
        out.results.len()
    );
}

#[tokio::test]
async fn every_row_has_a_magnet_name_and_hex_hash() {
    let url = common::oneshot(200, &fixture("nekobt-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = nekobt::search_at(&HttpClient::new(), &base, "one piece").await;

    for r in &out.results {
        assert_eq!(r.provider, "nekobt");
        assert_eq!(r.category.as_deref(), Some("Anime"));
        assert!(!r.name.trim().is_empty(), "名字为空");
        let magnet = r.magnet.as_deref().unwrap_or_default();
        assert!(magnet.starts_with("magnet:?xt=urn:btih:"), "磁力：{magnet}");
        assert_eq!(
            r.info_hash.as_deref().map(str::len),
            Some(40),
            "{} 的 hash 不是 40 位",
            r.name
        );
        // 名称链接拼出来的详情页要么是绝对地址，要么以 base 开头
        let d = r.detail_url.as_deref().unwrap_or_default();
        assert!(
            d.starts_with("http") || d.starts_with(&base),
            "detail_url 形状不对：{d}"
        );
    }
}

/// 名字取自 `span > span:nth-child(1)` —— 只该是标题，不该混进「seeders」这类旁注。
#[tokio::test]
async fn names_do_not_leak_neighbour_text() {
    let url = common::oneshot(200, &fixture("nekobt-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = nekobt::search_at(&HttpClient::new(), &base, "one piece").await;

    for r in &out.results {
        assert!(
            !r.name.contains("seeders") && !r.name.contains("leechers"),
            "名字里混进了旁注：{}",
            r.name
        );
        assert!(
            r.name.len() < 300,
            "名字过长，可能整行都拼进来了：{}",
            r.name
        );
    }
}

#[tokio::test]
async fn request_url_contract() {
    let (url, req) = common::oneshot_capture(200, &fixture("nekobt-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = nekobt::search_at(&HttpClient::new(), &base, "one piece").await;

    let raw = req.lock().unwrap().clone();
    assert!(raw.starts_with("GET /search?"), "请求行：{raw}");
    assert!(raw.contains("query=one%20piece"), "关键词要编码：{raw}");
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = nekobt::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 没有结果表 ⇒ 0 条 + 不算错误（JS 的 `parseRows` 返回 `[]`）。
#[tokio::test]
async fn page_without_the_table_is_not_an_error() {
    let url = common::oneshot(200, "<html><body>nothing</body></html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = nekobt::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}
