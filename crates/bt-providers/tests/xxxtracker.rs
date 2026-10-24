//! `src/providers/xxxtracker.js` 的对照测试。
//!
//! fixture 是 **2026-10-24 由 CI 抓的真实结果页**
//! （`test/fixtures/xxxtracker-one-piece.html`，92KB / 101 行 = 1 行表头 + 100 条）。
//! 本机对 `xxxtor.com` 不通（`curl` 直接 000）。
//!
//! ## 这个站的两个特点（详见 provider 文件头）
//!
//! 1. 一行**只有 4 个 td**（第 2 格 `colspan = "2"`），磁力就在行里 ⇒ 不需要详情页。
//! 2. `leechers` **恒为 None** —— JS 的 `span:nth-child(2)` 在真实 DOM 上匹配不到
//!    （第 4 格的元素子节点是 span / img / span）。这是**上游 bug，照抄**。
//!
//! ## 日期为什么不断言具体值
//!
//! 站点日期是 `18 Окт 26`，JS 的 `Date.parse` 按**本地时区**当天零点算 ⇒ 时间戳
//! 跟跑测试的机器时区绑定（CI 是 UTC、本机是 UTC+8，差 8 小时）。所以这里只断言
//! 「有值 + 能被格式化」，具体一致性交给 M6 差分（两侧都在同一台 CI 机器上跑）。

mod common;

use bt_core::http::HttpClient;
use bt_providers::xxxtracker;
use common::fixture;

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("xxxtracker-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = xxxtracker::search_at(&HttpClient::new(), &base, "one piece").await;

    assert_eq!(out.error, None);
    // 101 行 - 1 行表头 = 100；一条都不该被过滤掉（每行都有 magnet + 名字）
    assert_eq!(out.results.len(), 100, "行数不对 ⇒ 表头跳过或列映射有问题");
}

#[tokio::test]
async fn first_row_fields_match_the_js_truth() {
    let url = common::oneshot(200, &fixture("xxxtracker-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = xxxtracker::search_at(&HttpClient::new(), &base, "one piece").await;
    let r = out.results.first().expect("至少一条");

    // 期望值由真 JS 跑出来（normalize.js + parseSize + Date.parse），不是手抄：
    //   size 原文是 "1.03\u{a0}GB"（NBSP），1.03 * 1024^3 = 1105954078.72 → 1105954079
    assert_eq!(
        r.name,
        "Honey April - Honey's First Creampie and BBC / BBC Surprise (2026) SiteRip"
    );
    assert_eq!(r.id, "xxxtracker:ec3ba0e839485f06fdff4d694230fa198bb28b16");
    assert_eq!(r.provider, "xxxtracker");
    assert_eq!(r.category.as_deref(), Some("Porn"));
    assert_eq!(r.size, Some(1_105_954_079));
    assert_eq!(r.size_text, "1.0 GB");
    assert_eq!(r.seeders, Some(7));
    // ⚠️ 上游 bug：`span:nth-child(2)` 匹配不到 ⇒ leechers 恒 None（照抄）
    assert_eq!(r.leechers, None);
    assert!(r.magnet.as_deref().unwrap_or("").starts_with("magnet:?xt="));
    assert_eq!(
        r.info_hash.as_deref(),
        Some("ec3ba0e839485f06fdff4d694230fa198bb28b16")
    );
    assert!(
        r.detail_url
            .as_deref()
            .unwrap_or("")
            .ends_with("/torrent/119076/"),
        "detail_url 形状不对：{:?}",
        r.detail_url
    );
    assert!(!r.needs_magnet, "搜索阶段就带磁力");
    assert!(r.date.is_some(), "俄语月份没被翻译成可解析的日期");
    assert_ne!(r.date_text, "—");
}

/// 100 条里每一条都该有名字 / 磁力 / 分类。
#[tokio::test]
async fn every_row_is_complete() {
    let url = common::oneshot(200, &fixture("xxxtracker-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = xxxtracker::search_at(&HttpClient::new(), &base, "one piece").await;

    for r in &out.results {
        assert!(!r.name.trim().is_empty(), "名字为空");
        assert!(r.magnet.is_some(), "{} 没有磁力", r.name);
        assert!(r.info_hash.is_some(), "{} 没抠出 infoHash", r.name);
        assert_eq!(r.category.as_deref(), Some("Porn"));
        assert!(r.size.is_some(), "{} 没有体积", r.name);
        assert!(r.seeders.is_some(), "{} 没有做种数", r.name);
        // 详情链接必须是绝对地址（相对路径该被拼上 base）
        assert!(
            r.detail_url.as_deref().unwrap_or("").starts_with("http"),
            "{} 的 detail_url 不是绝对地址：{:?}",
            r.name,
            r.detail_url
        );
    }
}

#[tokio::test]
async fn request_url_contract() {
    let (url, req) = common::oneshot_capture(200, &fixture("xxxtracker-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = xxxtracker::search_at(&HttpClient::new(), &base, "one piece").await;

    let raw = req.lock().unwrap().clone();
    assert!(raw.starts_with("GET /b.php?"), "请求行：{raw}");
    assert!(
        raw.contains("search=one%20piece"),
        "关键词要按 encodeURIComponent 编码：{raw}"
    );
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = xxxtracker::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 没有结果表 ⇒ **算错误**（JS: `rows.length <= 1` → `no_results_parsed`），
/// 不是「0 条正常结果」。这是与 `zeromagnet` / `oxtorrent` 不同的地方，别抄错。
#[tokio::test]
async fn page_without_the_table_is_an_error() {
    let url = common::oneshot(200, "<html><body>nothing here</body></html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = xxxtracker::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
}

/// 只有表头的页面同样算错误 —— 「页面正常但没结果」在 JS 里是 error。
#[tokio::test]
async fn header_only_page_is_an_error_too() {
    let header_only = "<html><body><table width=\"100%\">\
        <tr class=\"backgr\"><td>Добавлен</td><td colspan=\"2\">Название</td>\
        <td>Размер</td><td>Пиры</td></tr></table></body></html>";
    let url = common::oneshot(200, header_only).await;
    let base = url.trim_end_matches('/').to_string();

    let out = xxxtracker::search_at(&HttpClient::new(), &base, "x").await;

    assert!(out.results.is_empty());
    assert_eq!(out.error.as_deref(), Some("no_results_parsed"));
}
