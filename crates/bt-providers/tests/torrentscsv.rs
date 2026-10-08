//! `src/providers/torrentscsv.js` 的对照测试。
//!
//! fixture `test/fixtures/torrentscsv-ubuntu.json` 是 **2026-10-08 抓的真实响应快照**
//! （`https://torrents-csv.com/service/search?q=ubuntu`，25 条）。
//! 期望值是把同一份 fixture 喂给 `src/lib/normalize.js` 得到的输出，逐字段抄来。

mod common;

use bt_core::http::HttpClient;
use bt_providers::torrentscsv;

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 fixture {}: {e}", p.display()))
}

// ---- 真实 fixture ---------------------------------------------------------

#[tokio::test]
async fn parses_the_real_torrentscsv_fixture() {
    let body = fixture("torrentscsv-ubuntu.json");
    let url = common::oneshot(200, &body).await;

    let out = torrentscsv::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 25, "fixture 里是 25 条");

    let first = &out.results[0];
    assert_eq!(first.provider, "torrentscsv");
    assert_eq!(
        first.name,
        "Clinton D., Negus C. Ubuntu Linux Bible 11ed 2025"
    );
    assert_eq!(
        first.id,
        "torrentscsv:232cd67eb3ffbd7c37bf9ec3ee887417e5ae1ee6"
    );
    assert_eq!(
        first.info_hash.as_deref(),
        Some("232cd67eb3ffbd7c37bf9ec3ee887417e5ae1ee6"),
        "这个站的 infohash 本来就是小写，原样透传"
    );
    assert_eq!(first.size, Some(8_554_738));
    assert_eq!(first.size_text, "8.2 MB");
    assert_eq!(first.seeders, Some(27));
    assert_eq!(first.leechers, Some(0), "0 是有效值，不该被当成缺失");
    assert_eq!(first.date, Some(1_784_214_068_000), "created_unix 是秒");
    assert_eq!(first.date_text, "2026-07-16");
    assert_eq!(first.category.as_deref(), Some("Other"), "该站不提供分类");
    assert_eq!(first.detail_url, None, "没有详情页");
    assert_eq!(first.files, None);
    assert!(!first.needs_magnet, "有 infoHash 就能直接拼出磁力链");
    assert_eq!(
        first.magnet.as_deref(),
        Some(
            "magnet:?xt=urn:btih:232cd67eb3ffbd7c37bf9ec3ee887417e5ae1ee6\
             &dn=Clinton%20D.%2C%20Negus%20C.%20Ubuntu%20Linux%20Bible%2011ed%202025"
        )
    );
}

/// 末条：2010 年的老种子，`sizeText` 落在 MB 量级 —— 顺带覆盖 size 格式化的另一档。
#[tokio::test]
async fn parses_the_oldest_row_too() {
    let body = fixture("torrentscsv-ubuntu.json");
    let url = common::oneshot(200, &body).await;

    let out = torrentscsv::search_at(&HttpClient::new(), &url, "ubuntu").await;

    let last = out.results.last().expect("该有结果");
    assert_eq!(
        last.id,
        "torrentscsv:a1425e0d6630336cdd9fb320f3fff1030098975a"
    );
    assert_eq!(last.name, "Ubuntu 10.04 LTS x64");
    assert_eq!(last.size, Some(731_453_440));
    assert_eq!(last.size_text, "698 MB");
    assert_eq!(last.seeders, Some(2));
    assert_eq!(last.date_text, "2010-04-29");
}

#[tokio::test]
async fn every_fixture_row_makes_it_through() {
    let body = fixture("torrentscsv-ubuntu.json");
    let url = common::oneshot(200, &body).await;

    let out = torrentscsv::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(
        out.results.iter().all(|r| !r.name.is_empty()),
        "不该有结果退化成 (untitled)"
    );
    assert!(
        out.results.iter().all(|r| r.magnet.is_some()),
        "全部 25 条都有 infohash"
    );
    assert!(
        out.results
            .iter()
            .all(|r| r.category.as_deref() == Some("Other")),
        "分类是写死的"
    );
}

// ---- 请求契约 -------------------------------------------------------------

/// 关键词必须过 `encodeURIComponent` —— 查询里带空格/`&` 的写法很常见，
/// 拼错会让服务端把 `&` 当成参数分隔符。
#[tokio::test]
async fn query_is_uri_encoded_in_request_url() {
    let body = fixture("torrentscsv-ubuntu.json");
    let (url, seen) = common::oneshot_capture(200, &body).await;

    let _ = torrentscsv::search_at(&HttpClient::new(), &url, "ubuntu 22.04 &LTS").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(
        raw.contains("/?q=ubuntu%2022.04%20%26LTS "),
        "空格 → %20、& → %26: {raw}"
    );
}

// ---- 空结果与失败路径 -----------------------------------------------------

#[tokio::test]
async fn empty_torrents_is_no_results_not_an_error() {
    let url = common::oneshot(200, r#"{"torrents":[],"next":null}"#).await;
    let out = torrentscsv::search_at(&HttpClient::new(), &url, "nonexistent-xyz-12345").await;

    assert_eq!(out.error, None, "无结果不是错误");
    assert!(out.results.is_empty());
}

#[tokio::test]
async fn torrents_missing_or_not_an_array_is_no_results() {
    for body in [r#"{"next":null}"#, r#"{"torrents":"nope"}"#, r#"[1,2,3]"#] {
        let url = common::oneshot(200, body).await;
        let out = torrentscsv::search_at(&HttpClient::new(), &url, "ubuntu").await;
        assert_eq!(out.error, None, "body={body}");
        assert!(out.results.is_empty(), "body={body}");
    }
}

/// JS 写的是 `it.created_unix ? Number(it.created_unix) : null` —— 0 是 falsy，
/// 缺字段和 0 都该退化成"没有日期"，而不是 1970-01-01。
#[tokio::test]
async fn falsy_created_unix_means_no_date_but_still_a_result() {
    let body = r#"{"torrents":[
        {"infohash":"aaaabbbbccccddddeeeeffff0000111122223333","name":"no date","size_bytes":100,"seeders":1,"leechers":2},
        {"infohash":"bbbbccccddddeeeeffff00001111222233334444","name":"zero date","size_bytes":200,"seeders":3,"leechers":4,"created_unix":0}
    ]}"#;
    let url = common::oneshot(200, body).await;

    let out = torrentscsv::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 2, "缺日期不该让条目消失");
    for r in &out.results {
        assert_eq!(r.date, None, "{}", r.name);
        assert_eq!(r.date_text, "", "{}", r.name);
    }
}

#[tokio::test]
async fn http_error_becomes_torrentscsv_unreachable() {
    let url = common::oneshot(500, "nope").await;
    let out = torrentscsv::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("应当有 error");
    assert!(e.contains("TorrentsCSV unreachable"), "{e}");
}

/// ⚠️ **有意偏离 JS**（divergence，别当 bug 修；与 `knaben.rs` 同一条）。
///
/// JS 拿到「200 + 非 JSON 正文」时 `data.torrents` 是 `undefined` → 静默返回空。
/// Rust 版显式报错并附正文开头，因为对抓取型项目「静默为空」比「报错」危险得多
/// （Cloudflare 拦页会伪装成"没结果"）。
#[tokio::test]
async fn divergence_non_json_body_becomes_an_error_instead_of_silent_empty() {
    let url = common::oneshot(200, "<html>Attention Required!</html>").await;
    let out = torrentscsv::search_at(&HttpClient::new(), &url, "ubuntu").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("Rust 版应当报错，JS 版会静默返回空");
    assert!(e.contains("TorrentsCSV unreachable"), "{e}");
    assert!(
        e.contains("Attention Required"),
        "错误里必须带原始正文开头，否则拦页和改版分不清: {e}"
    );
}
