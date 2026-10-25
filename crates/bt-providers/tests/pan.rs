//! `pan`（PanSou 客户端）的离线测试。
//!
//! 用一份**合成** fixture（`pansou-search.json`）钉住上游的 `merged_by_type` 形状；
//! 真实实例在用户机器上（默认 `http://127.0.0.1:8888`），CI 不碰外网。

mod common;

use bt_core::http::HttpClient;
use bt_providers::pan::PanSouClient;
use common::fixture;

#[tokio::test]
async fn parses_merged_by_type() {
    let url = common::oneshot(200, &fixture("pansou-search.json")).await;
    let base = url.trim_end_matches('/').to_string();
    let client = PanSouClient::new(&base, None);

    let out = client.search(&HttpClient::new(), "流浪地球", 100).await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 3, "quark 2 + baidu 1");

    let quark: Vec<_> = out
        .results
        .iter()
        .filter(|r| r.pan_type == "quark")
        .collect();
    assert_eq!(quark.len(), 2);
    assert!(quark.iter().any(|r| r.code.as_deref() == Some("1234")));
    assert!(
        quark.iter().any(|r| r.code.is_none()),
        "空 password 应该是 None"
    );
    assert!(quark.iter().any(|r| r.title.contains("流浪地球2")));

    let baidu = out.results.iter().find(|r| r.pan_type == "baidu").unwrap();
    assert_eq!(baidu.code.as_deref(), Some("abcd"));
    assert_eq!(baidu.url, "https://pan.baidu.com/s/ccc333");
    assert_eq!(baidu.source.as_deref(), Some("tg:test_channel"));
}

#[tokio::test]
async fn request_contract_is_json_post() {
    let (url, req) = common::oneshot_capture(200, &fixture("pansou-search.json")).await;
    let base = url.trim_end_matches('/').to_string();
    let client = PanSouClient::new(&base, None);

    let _ = client.search(&HttpClient::new(), "流浪地球", 100).await;

    let raw = req.lock().unwrap().clone();
    assert!(raw.starts_with("POST /api/search"), "请求行：{raw}");
    assert!(
        raw.to_lowercase()
            .contains("content-type: application/json"),
        "必须是 JSON POST：{raw}"
    );
    assert!(raw.contains("\"kw\":\"流浪地球\""), "body 缺 kw：{raw}");
    assert!(raw.contains("\"res\":\"merge\""), "body 缺 res：{raw}");
}

#[tokio::test]
async fn token_becomes_bearer_header() {
    let (url, req) = common::oneshot_capture(200, &fixture("pansou-search.json")).await;
    let base = url.trim_end_matches('/').to_string();
    let client = PanSouClient::new(&base, Some("test-token"));

    let _ = client.search(&HttpClient::new(), "x", 10).await;

    let raw = req.lock().unwrap().to_lowercase();
    assert!(
        raw.contains("authorization: bearer test-token"),
        "缺 Authorization：{raw}"
    );
}

#[tokio::test]
async fn empty_merged_is_ok() {
    let url = common::oneshot(200, r#"{"total":0,"merged_by_type":{}}"#).await;
    let base = url.trim_end_matches('/').to_string();
    let client = PanSouClient::new(&base, None);

    let out = client.search(&HttpClient::new(), "x", 10).await;

    assert!(out.results.is_empty());
    assert_eq!(out.error, None);
}

#[tokio::test]
async fn http_error_is_reported() {
    let url = common::oneshot(500, "boom").await;
    let base = url.trim_end_matches('/').to_string();
    let client = PanSouClient::new(&base, None);

    let out = client.search(&HttpClient::new(), "x", 10).await;

    assert!(out.results.is_empty());
    assert!(out.error.is_some());
}

#[tokio::test]
async fn health_checks_the_endpoint() {
    let url = common::oneshot(200, r#"{"status":"ok"}"#).await;
    let base = url.trim_end_matches('/').to_string();
    let client = PanSouClient::new(&base, None);

    assert!(client.health(&HttpClient::new()).await);

    let bad = common::oneshot(500, "boom").await;
    let bad = PanSouClient::new(bad.trim_end_matches('/'), None);
    assert!(!bad.health(&HttpClient::new()).await);
}
