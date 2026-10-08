//! `src/providers/yts.js` 的对照测试。
//!
//! fixture `test/fixtures/yts-matrix.json` 是 **2026-10-08 抓的真实响应快照**
//! （`list_movies.json?query_term=matrix&limit=2`，2 部电影 / 7 个种子）。
//! 期望值是把同一份 fixture 喂给 `src/lib/normalize.js` 得到的输出，逐字段抄来。

mod common;

use bt_core::http::HttpClient;
use bt_providers::yts;

fn fixture(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 fixture {}: {e}", p.display()))
}

// ---- 真实 fixture ---------------------------------------------------------

#[tokio::test]
async fn parses_the_real_yts_fixture() {
    let body = fixture("yts-matrix.json");
    let url = common::oneshot(200, &body).await;

    let out = yts::search_at(&HttpClient::new(), &url, "matrix").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 7, "2 部电影展开成 7 条（2 + 5）");

    let first = &out.results[0];
    assert_eq!(first.provider, "yts");
    assert_eq!(
        first.name, "Matrix: Generation (2024) [720p] [web] [x264]",
        "名字是 title_long + 画质/类型/编码拼出来的"
    );
    assert_eq!(first.id, "yts:937c8886c8fd31240898b0de40de9e104a926f7e");
    assert_eq!(
        first.info_hash.as_deref(),
        Some("937c8886c8fd31240898b0de40de9e104a926f7e"),
        "⚠️ yts 是少数会把 hash 小写化的 provider（fixture 里原值是大写）"
    );
    assert_eq!(first.size, Some(511_568_773));
    assert_eq!(first.size_text, "488 MB");
    assert_eq!(first.seeders, Some(7), "seeds 字段");
    assert_eq!(first.leechers, Some(1), "peers 字段是 leechers");
    assert_eq!(
        first.date,
        Some(1_705_959_944_000),
        "date_uploaded_unix 是秒"
    );
    assert_eq!(first.date_text, "2024-01-22");
    assert_eq!(first.category.as_deref(), Some("Movies"), "分类写死");
    assert_eq!(
        first.detail_url.as_deref(),
        Some(
            "https://yts.gg/movies/matrix-generation-2024\
             ?movieid=59406&infohash=937c8886c8fd31240898b0de40de9e104a926f7e"
        ),
        "详情页带上 movieid 与 infohash"
    );
    assert!(!first.needs_magnet);
}

#[tokio::test]
async fn second_movie_expands_into_its_own_rows() {
    let body = fixture("yts-matrix.json");
    let url = common::oneshot(200, &body).await;

    let out = yts::search_at(&HttpClient::new(), &url, "matrix").await;

    // 第 3 条开始是第二部电影（前 2 条属于第一部）
    let third = &out.results[2];
    assert_eq!(
        third.name,
        "The Matrix Resurrections (2021) [720p] [bluray] [x264]"
    );
    assert_eq!(third.id, "yts:107facda1820df8212022863fffa19a971563595");
    assert_eq!(third.size, Some(1_428_076_626));
    assert_eq!(third.size_text, "1.3 GB");
    assert_eq!(third.seeders, Some(34));
    assert_eq!(third.leechers, Some(9));
    assert_eq!(third.date_text, "2022-02-19");
    assert_eq!(
        third.detail_url.as_deref(),
        Some(
            "https://yts.gg/movies/the-matrix-resurrections-2021\
             ?movieid=38698&infohash=107facda1820df8212022863fffa19a971563595"
        )
    );

    let last = out.results.last().expect("该有结果");
    assert_eq!(
        last.name,
        "The Matrix Resurrections (2021) [2160p] [web] [x265]"
    );
    assert_eq!(last.size, Some(7_086_696_038));
    assert_eq!(last.size_text, "6.6 GB");
    assert_eq!(last.seeders, Some(78));
    assert_eq!(last.date_text, "2021-12-25");
}

#[tokio::test]
async fn every_fixture_row_makes_it_through() {
    let body = fixture("yts-matrix.json");
    let url = common::oneshot(200, &body).await;

    let out = yts::search_at(&HttpClient::new(), &url, "matrix").await;

    assert!(
        out.results
            .iter()
            .all(|r| r.name.contains(" [") && r.name.ends_with(']')),
        "每条名字都该带 [画质] [类型] [编码]"
    );
    assert!(
        out.results.iter().all(|r| r
            .info_hash
            .as_deref()
            .is_some_and(|h| h == h.to_lowercase())),
        "hash 一律小写"
    );
}

// ---- 名字拼接与缺省值 -----------------------------------------------------

/// JS 的 `t.quality || '-'`：缺失和空串都退化成 `-`。
/// 同时覆盖 `title_long || title` 的回退。
#[tokio::test]
async fn missing_title_long_falls_back_and_missing_fields_become_dash() {
    let body = r#"{"status":"ok","data":{"movies":[
        {"id":1,"title":"Only Title","url":"https://yts.gg/movies/x","torrents":[
            {"hash":"ABCDEF0123456789ABCDEF0123456789ABCDEF01","size_bytes":100,"seeds":1,"peers":2}
        ]}
    ]}}"#;
    let url = common::oneshot(200, body).await;

    let out = yts::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 1);
    let r = &out.results[0];
    assert_eq!(r.name, "Only Title [-] [-] [-]");
    assert_eq!(
        r.info_hash.as_deref(),
        Some("abcdef0123456789abcdef0123456789abcdef01"),
        "大写 hash 要小写化"
    );
    assert_eq!(
        r.detail_url.as_deref(),
        Some("https://yts.gg/movies/x?movieid=1&infohash=abcdef0123456789abcdef0123456789abcdef01")
    );
}

/// 没有 `url` 时 JS 把 `detailUrl` 置 null（而不是拼出半个 URL）。
#[tokio::test]
async fn missing_movie_url_means_no_detail_url() {
    let body = r#"{"data":{"movies":[
        {"id":7,"title_long":"No URL (2020)","torrents":[
            {"hash":"abcdef0123456789abcdef0123456789abcdef01","quality":"1080p","type":"web","video_codec":"x264","size_bytes":5,"seeds":1,"peers":0}
        ]}
    ]}}"#;
    let url = common::oneshot(200, body).await;

    let out = yts::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.results.len(), 1);
    assert_eq!(out.results[0].detail_url, None);
    assert_eq!(out.results[0].name, "No URL (2020) [1080p] [web] [x264]");
}

// ---- 跳过规则 -------------------------------------------------------------

/// 两条 `continue`：没有片名的电影整部跳过；没有 hash 的种子逐条跳过。
#[tokio::test]
async fn movies_without_title_and_torrents_without_hash_are_skipped() {
    let body = r#"{"data":{"movies":[
        {"id":1,"url":"u1","torrents":[{"hash":"abcdef0123456789abcdef0123456789abcdef01"}]},
        {"id":2,"url":"u2","title_long":"Has Title","torrents":[
            {"hash":"","size_bytes":1},
            {"size_bytes":2},
            {"hash":"1111222233334444555566667777888899990000","size_bytes":3}
        ]}
    ]}}"#;
    let url = common::oneshot(200, body).await;

    let out = yts::search_at(&HttpClient::new(), &url, "x").await;

    assert_eq!(out.error, None);
    assert_eq!(
        out.results.len(),
        1,
        "无片名的电影 + 无 hash 的种子都要被丢掉"
    );
    assert_eq!(out.results[0].name, "Has Title [-] [-] [-]");
}

// ---- 空结果与失败路径 -----------------------------------------------------

#[tokio::test]
async fn empty_or_malformed_data_is_no_results() {
    for body in [
        r#"{"status":"ok","data":{"movie_count":0,"movies":[]}}"#,
        r#"{"status":"ok","data":{"movie_count":0}}"#,
        r#"{"status":"ok","data":null}"#,
        r#"{"status":"ok"}"#,
        r#"{"data":{"movies":"nope"}}"#,
        r#"[1,2,3]"#,
    ] {
        let url = common::oneshot(200, body).await;
        let out = yts::search_at(&HttpClient::new(), &url, "matrix").await;
        assert_eq!(out.error, None, "body={body}");
        assert!(out.results.is_empty(), "body={body}");
    }
}

#[tokio::test]
async fn http_error_becomes_yts_unreachable() {
    let url = common::oneshot(502, "nope").await;
    let out = yts::search_at(&HttpClient::new(), &url, "matrix").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("应当有 error");
    assert!(e.contains("YTS unreachable"), "{e}");
}

// ---- 请求契约 -------------------------------------------------------------

#[tokio::test]
async fn request_url_has_encoded_query_term_and_limit() {
    let body = fixture("yts-matrix.json");
    let (url, seen) = common::oneshot_capture(200, &body).await;

    let _ = yts::search_at(&HttpClient::new(), &url, "the matrix & more").await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(
        raw.contains("/list_movies.json?query_term=the%20matrix%20%26%20more&limit=50 "),
        "路径 + 编码后的 query_term + limit=50 都要对: {raw}"
    );
}

/// ⚠️ **有意偏离 JS**（divergence，别当 bug 修；与 `knaben.rs` 同一条）。
#[tokio::test]
async fn divergence_non_json_body_becomes_an_error_instead_of_silent_empty() {
    let url = common::oneshot(200, "<html>502 Bad Gateway</html>").await;
    let out = yts::search_at(&HttpClient::new(), &url, "matrix").await;

    assert!(out.results.is_empty());
    let e = out.error.expect("Rust 版应当报错，JS 版会静默返回空");
    assert!(e.contains("YTS unreachable"), "{e}");
    assert!(e.contains("502 Bad Gateway"), "{e}");
}
