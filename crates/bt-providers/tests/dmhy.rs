//! `src/providers/dmhy.js` 的对照测试。
//!
//! fixture 是 **2026-10-21 由 CI 抓的真实结果页**
//! （`test/fixtures/dmhy-one-piece.html`，950KB / 81 个 `<tr>` = 1 表头 + 80 条数据，
//! 80 条全带磁力）。本机网络对 dmhy 封锁，快照只能让 CI 取（`fetch-fixture.yml`）。
//!
//! ## 这个站最值得钉住的一条
//!
//! JS 用 `ownText`（只取**直接**文本节点）取标题，而站上会把命中的关键词包进
//! `<span class="keyword">`。于是**名字里没有关键词**，只剩「[Skymoon-Raws][ 海賊王]…」
//! 这种残缺形状 —— 看着像 bug，但那是 Node 版的实际行为，Rust 侧必须一致。

mod common;

use bt_core::http::HttpClient;
use bt_providers::dmhy;
use common::fixture;

/// 80 条数据行，全部带磁力。
const EXPECTED_RESULTS: usize = 80;

#[tokio::test]
async fn parses_the_real_fixture() {
    let url = common::oneshot(200, &fixture("dmhy-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = dmhy::search_at(&HttpClient::new(), &base, "one piece", 1).await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), EXPECTED_RESULTS);
}

#[tokio::test]
async fn first_row_fields_match_the_js_pipeline() {
    let url = common::oneshot(200, &fixture("dmhy-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = dmhy::search_at(&HttpClient::new(), &base, "one piece", 1).await;
    let r = &out.results[0];

    assert_eq!(r.provider, "dmhy");

    // ⚠️ ownText 语义：直接子文本节点拼起来 ⇒ **没有**被 <span class="keyword"> 包住的词
    assert!(
        r.name.starts_with("[Skymoon-Raws]["),
        "名称要以发布组开头，实际：{}",
        r.name
    );
    assert!(r.name.contains("海賊王"), "实际：{}", r.name);
    assert!(
        !r.name.contains("One") && !r.name.contains("Piece"),
        "ownText 会把高亮的 One/Piece 丢掉（与 JS 一致），实际：{}",
        r.name
    );
    assert!(
        !r.name.contains("天月動漫"),
        "不能取到 <span class=\"tag\"> 里的字幕组链接（那是后代元素），实际：{}",
        r.name
    );

    // 磁力原样保留（含全部 tracker），且是第 4 格里的**第一个** <a>
    assert!(r
        .magnet
        .as_deref()
        .unwrap_or_default()
        .starts_with("magnet:?xt=urn:btih:LJDJSTRGCAW5PUOER34MCTSCQLPWD4XG"));
    // base32 hash ⇒ 与 JS 一样取不到 hex infoHash
    assert_eq!(r.info_hash, None);
    assert!(!r.needs_magnet);

    assert!(r.size.is_some(), "size_text = {}", r.size_text);
    assert!(r.size_text.ends_with("MB"), "size_text = {}", r.size_text);
    assert_eq!(r.seeders, Some(11));
    assert_eq!(r.leechers, Some(0));

    // 日期取第 1 格里那个 display:none 的 <span>（2026/09/28 16:47）
    assert_eq!(r.date_text, "2026-09-28");

    // class="sort-2" → Anime
    assert_eq!(r.category.as_deref(), Some("Anime"));

    assert_eq!(
        r.detail_url.as_deref(),
        Some(
            format!(
                "{base}/topics/view/727909_Skymoon-Raws_One_Piece_1180_ViuTV_WEB-RIP_CHT_SRT_1080p_MKV.html"
            )
            .as_str()
        )
    );
}

#[tokio::test]
async fn every_row_has_a_magnet_and_a_name() {
    let url = common::oneshot(200, &fixture("dmhy-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = dmhy::search_at(&HttpClient::new(), &base, "one piece", 1).await;

    for r in &out.results {
        assert!(r.magnet.is_some(), "{} 没有磁力", r.name);
        assert!(!r.name.trim().is_empty());
        // 分类必须是映射过的标准桶之一
        let cat = r.category.as_deref().unwrap_or("");
        assert!(
            ["Anime", "Books", "Series", "Music", "Games", "Other"].contains(&cat),
            "未映射的分类：{cat}"
        );
    }
}

/// 请求契约：关键词要百分号编码，`page` 直接进 URL（JS 没做 coercePage）。
#[tokio::test]
async fn request_url_contract() {
    let (url, req) = common::oneshot_capture(200, &fixture("dmhy-one-piece.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = dmhy::search_at(&HttpClient::new(), &base, "one piece", 3).await;

    let raw = req.lock().unwrap().clone();
    assert!(raw.starts_with("GET /topics/list?"), "请求行：{raw}");
    assert!(raw.contains("keyword=one%20piece"), "关键词要编码：{raw}");
    assert!(raw.contains("page=3"), "page 要进 URL：{raw}");
    assert!(raw.contains("order=date-desc"), "排序参数照抄：{raw}");
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();

    let out = dmhy::search_at(&HttpClient::new(), &base, "x", 1).await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

/// 没有结果表 ⇒ 0 条 + **不算错误**（JS 返回 `error: null`）。
#[tokio::test]
async fn page_without_the_table_is_not_an_error() {
    let url = common::oneshot(200, "<html><body>nothing here</body></html>").await;
    let base = url.trim_end_matches('/').to_string();

    let out = dmhy::search_at(&HttpClient::new(), &base, "x", 1).await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}
