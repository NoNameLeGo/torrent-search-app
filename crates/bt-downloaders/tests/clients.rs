//! M3 验收：四个下载客户端，对着 `src/lib/downloaders.js`（214 行，唯一真值来源）。
//!
//! 全程用**脚本化本地服务**，不碰外网。重点钉三件事：
//!
//! 1. **请求契约**：qB 的 form body、Transmission 的 409 重发头、aria2 的
//!    `token:` 平铺、Gopeed 的 `X-Api-Token` 与"test 是 GET 且不带 Content-Type"。
//! 2. **失败路径**：认证失败 / 连不上 / RPC 报错，各有用例。
//! 3. **两个手册点名的坑不许退回去**：qB 的 SID 要在**全部** set-cookie 里找；
//!    URL 只放行 `http:` / `https:`。

mod common;

use common::{body_of, dead_url, nth, scripted, Resp};
use serde_json::{json, Value};

use bt_downloaders::{
    detect_candidates, detect_from, meta, meta_of, push, safe_http_url, test, Auth, Config, Kind,
    PushOutcome, INVALID_URL_SCHEME,
};

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

fn cfg(url: &str) -> Config {
    Config {
        url: url.to_string(),
        ..Default::default()
    }
}

fn cfg_auth(url: &str, user: &str, pass: &str) -> Config {
    Config {
        url: url.to_string(),
        user: user.to_string(),
        pass: pass.to_string(),
        token: String::new(),
    }
}

fn cfg_token(url: &str, token: &str) -> Config {
    Config {
        url: url.to_string(),
        token: token.to_string(),
        ..Default::default()
    }
}

/// 一个含多种特殊字符的磁力，用来把 `encodeURIComponent` 的保留集钉死。
const MAGNET: &str = "magnet:?xt=urn:btih:ABC&dn=ubuntu 24.04 (x64)";

/// `encodeURIComponent(MAGNET)` 的期望值（`(` `)` 保留，空格 %20，`.` 保留）。
const MAGNET_ENCODED: &str = "magnet%3A%3Fxt%3Durn%3Abtih%3AABC%26dn%3Dubuntu%2024.04%20(x64)";

fn lower(s: &str) -> String {
    s.to_lowercase()
}

// ---------------------------------------------------------------------------
// 纯数据：META / Kind / 探测候选 / URL 校验
// ---------------------------------------------------------------------------

#[test]
fn meta_matches_the_js_meta_table() {
    let all = meta();
    assert_eq!(all.len(), 4);

    let pairs: Vec<(&str, &str, &str, &str)> = all
        .iter()
        .map(|m| (m.kind.as_str(), m.label, m.auth.as_str(), m.default_url))
        .collect();

    assert_eq!(
        pairs,
        [
            (
                "qbittorrent",
                "qBittorrent",
                "userpass",
                "http://localhost:8080"
            ),
            (
                "transmission",
                "Transmission",
                "userpass",
                "http://localhost:9091"
            ),
            (
                "aria2",
                "aria2 / Motrix",
                "token",
                "http://localhost:16800/jsonrpc"
            ),
            ("gopeed", "Gopeed", "token", "http://localhost:9999"),
        ]
    );

    assert_eq!(meta_of(Kind::Aria2).auth, Auth::Token);
    assert_eq!(meta_of(Kind::Qbittorrent).auth, Auth::UserPass);
}

#[test]
fn kind_round_trips_and_rejects_unknown() {
    for k in [
        Kind::Qbittorrent,
        Kind::Transmission,
        Kind::Aria2,
        Kind::Gopeed,
    ] {
        assert_eq!(Kind::parse(k.as_str()), Some(k));
    }
    // JS 的 `!CLIENTS[kind]` ⇔ 这里 parse 失败
    assert_eq!(Kind::parse(""), None);
    assert_eq!(Kind::parse("QBITTORRENT"), None, "kind 是大小写敏感的");
    assert_eq!(Kind::parse("utorrent"), None);
}

#[test]
fn detect_candidates_match_the_js_table_in_order() {
    let c = detect_candidates();
    let rows: Vec<(&str, &str, &str, &str)> = c
        .iter()
        .map(|x| {
            (
                x.kind.as_str(),
                x.url.as_str(),
                x.user.as_str(),
                x.pass.as_str(),
            )
        })
        .collect();

    assert_eq!(
        rows,
        [
            (
                "qbittorrent",
                "http://localhost:8080",
                "admin",
                "adminadmin"
            ),
            ("qbittorrent", "http://localhost:8080", "admin", ""),
            (
                "qbittorrent",
                "http://127.0.0.1:8080",
                "admin",
                "adminadmin"
            ),
            (
                "qbittorrent",
                "http://localhost:8081",
                "admin",
                "adminadmin"
            ),
            ("transmission", "http://localhost:9091", "", ""),
            ("transmission", "http://127.0.0.1:9091", "", ""),
            // Motrix 默认
            ("aria2", "http://localhost:16800/jsonrpc", "", ""),
            // aria2 默认
            ("aria2", "http://localhost:6800/jsonrpc", "", ""),
            ("gopeed", "http://localhost:9999", "", ""),
        ]
    );
}

#[test]
fn safe_http_url_only_allows_http_and_https() {
    // 归一化：JS 的 `new URL(x).toString()` 会给裸主机补结尾斜杠
    assert_eq!(
        safe_http_url("http://localhost:8080").as_deref(),
        Some("http://localhost:8080/")
    );
    assert_eq!(
        safe_http_url("https://example.com/x").as_deref(),
        Some("https://example.com/x")
    );
    // JS 先 `String(raw).trim()`
    assert_eq!(
        safe_http_url("  http://localhost:8080  ").as_deref(),
        Some("http://localhost:8080/")
    );

    // ⚠️ 信任边界：只放行这两种协议
    for bad in [
        "ftp://localhost/x",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:text/html,x",
        "",
        "   ",
        "not a url",
        "localhost:8080", // 没有协议 → URL 解析失败
        "//localhost:8080",
    ] {
        assert_eq!(safe_http_url(bad), None, "{bad:?} 不该被放行");
    }
}

#[tokio::test]
async fn push_and_test_reject_a_bad_scheme_before_connecting() {
    // 用死地址：如果实现先连再校验，错误就会是"连不上"而不是 invalid_url_scheme
    let url = dead_url().await;
    let bad = format!("ftp://{}", url.trim_start_matches("http://"));

    assert_eq!(
        push(Kind::Aria2, &cfg(&bad), MAGNET).await.unwrap_err(),
        INVALID_URL_SCHEME
    );
    assert_eq!(
        test(Kind::Gopeed, &cfg(&bad)).await.unwrap_err(),
        INVALID_URL_SCHEME
    );
}

#[tokio::test]
async fn detect_from_returns_none_for_an_empty_candidate_list() {
    assert!(detect_from(&[]).await.is_none());
}

// ---------------------------------------------------------------------------
// qBittorrent
// ---------------------------------------------------------------------------

fn qb_login_ok() -> Resp {
    Resp::new(200, "Ok.").with_header("set-cookie", "SID=abc123; path=/")
}

#[tokio::test]
async fn qb_push_logs_in_then_adds_with_the_cookie_and_form_bodies() {
    let (url, seen) = scripted(vec![qb_login_ok(), Resp::new(200, "Ok.")]).await;

    let out = push(
        Kind::Qbittorrent,
        &cfg_auth(&url, "ad min", "p@ss:w/rd"),
        MAGNET,
    )
    .await
    .unwrap();

    assert_eq!(out, PushOutcome { status: 200 });

    // 第 1 跳：登录。body 必须是 encodeURIComponent 编的 form
    let first = nth(&seen, 0);
    assert!(first.starts_with("POST /api/v2/auth/login "), "{first}");
    assert!(lower(&first).contains("content-type: application/x-www-form-urlencoded"));
    assert_eq!(
        body_of(&first),
        "username=ad%20min&password=p%40ss%3Aw%2Frd",
        "encodeURIComponent 的保留集被写错了"
    );

    // 第 2 跳：add，带着 SID cookie，body 是 urls=<encodeURIComponent(magnet)>
    let second = nth(&seen, 1);
    assert!(second.starts_with("POST /api/v2/torrents/add "), "{second}");
    assert!(
        lower(&second).contains("cookie: sid=abc123; path=/"),
        "整个 set-cookie 条目原样当 Cookie 头（照抄 JS）：{second}"
    );
    assert_eq!(body_of(&second), format!("urls={MAGNET_ENCODED}"));
}

/// ⚠️ 手册点名的坑：**必须遍历全部 `set-cookie`**。旧实现只看第一个，
/// 于是"第一条不是 SID"时误判登录失败。
#[tokio::test]
async fn qb_login_scans_all_set_cookie_entries_not_just_the_first() {
    let (url, seen) = scripted(vec![
        Resp::new(200, "Ok.")
            .with_header("set-cookie", "lang=en; path=/")
            .with_header("set-cookie", "SID=xyz789; path=/"),
        Resp::new(200, "Ok."),
    ])
    .await;

    push(Kind::Qbittorrent, &cfg_auth(&url, "admin", "pw"), MAGNET)
        .await
        .expect("第二条 set-cookie 里有 SID，必须认");

    let second = nth(&seen, 1);
    assert!(lower(&second).contains("cookie: sid=xyz789"), "{second}");
}

#[tokio::test]
async fn qb_bad_credentials_403_is_the_pinned_message() {
    let (url, _seen) = scripted(vec![Resp::new(403, "Fails.")]).await;
    let err = push(Kind::Qbittorrent, &cfg(&url), MAGNET)
        .await
        .unwrap_err();
    assert_eq!(err, "登录失败（用户名或密码错误）");
}

/// 登录回 200 但**没有** SID cookie —— 同一个文案（JS 就是这么判的）。
#[tokio::test]
async fn qb_login_200_without_sid_cookie_is_reported_as_bad_credentials() {
    let (url, _seen) = scripted(vec![
        Resp::new(200, "Ok.").with_header("set-cookie", "lang=en")
    ])
    .await;
    let err = test(Kind::Qbittorrent, &cfg(&url)).await.unwrap_err();
    assert_eq!(err, "登录失败（用户名或密码错误）");
}

/// `test` 只登录，**不产生下载副作用** —— 脚本只给一条响应，多一次请求就会失败。
#[tokio::test]
async fn qb_test_only_logs_in() {
    let (url, seen) = scripted(vec![qb_login_ok()]).await;
    test(Kind::Qbittorrent, &cfg(&url)).await.unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(nth(&seen, 0).starts_with("POST /api/v2/auth/login "));
}

#[tokio::test]
async fn qb_connection_refused_is_a_readable_error() {
    let err = test(Kind::Qbittorrent, &cfg(&dead_url().await))
        .await
        .unwrap_err();
    assert!(err.contains("请求下载器失败"), "{err}");
}

// ---------------------------------------------------------------------------
// Transmission
// ---------------------------------------------------------------------------

#[tokio::test]
async fn transmission_push_posts_torrent_add() {
    let (url, seen) = scripted(vec![Resp::new(200, r#"{"result":"success"}"#)]).await;

    let out = push(Kind::Transmission, &cfg(&url), MAGNET).await.unwrap();
    assert_eq!(out, PushOutcome { status: 200 });

    let raw = nth(&seen, 0);
    assert!(raw.starts_with("POST /transmission/rpc "), "{raw}");
    let body: Value = serde_json::from_str(body_of(&raw)).unwrap();
    assert_eq!(body["method"], "torrent-add");
    assert_eq!(body["arguments"]["filename"], MAGNET);
}

/// ⚠️ **409 重试**：首次 409 带会话头 → 带头发第二次。
#[tokio::test]
async fn transmission_retries_once_with_the_session_id_on_409() {
    let (url, seen) = scripted(vec![
        Resp::new(409, "conflict").with_header("X-Transmission-Session-Id", "sess-42"),
        Resp::new(200, r#"{"result":"success"}"#),
    ])
    .await;

    push(Kind::Transmission, &cfg(&url), MAGNET).await.unwrap();

    let first = nth(&seen, 0);
    assert!(
        !lower(&first).contains("x-transmission-session-id"),
        "第一次不该带会话头：{first}"
    );
    let second = nth(&seen, 1);
    assert!(
        lower(&second).contains("x-transmission-session-id: sess-42"),
        "重发那次必须带上会话头：{second}"
    );
}

/// 409 但**没有**会话头 → 报错，且**不重发**（JS 是 `throw e`）。
#[tokio::test]
async fn transmission_409_without_session_header_fails_without_retrying() {
    let (url, seen) = scripted(vec![Resp::new(409, "conflict")]).await;

    let err = push(Kind::Transmission, &cfg(&url), MAGNET)
        .await
        .unwrap_err();
    assert!(err.contains("409"), "{err}");
    assert_eq!(seen.lock().unwrap().len(), 1, "不该重发");
}

#[tokio::test]
async fn transmission_reports_a_non_success_result() {
    let (url, _seen) = scripted(vec![Resp::new(200, r#"{"result":"duplicate torrent"}"#)]).await;
    let err = push(Kind::Transmission, &cfg(&url), MAGNET)
        .await
        .unwrap_err();
    assert_eq!(err, "Transmission: duplicate torrent");
}

/// `result` 缺失 / 空串 → **放行**（JS 的软检查 `data.result && …`）。
#[tokio::test]
async fn transmission_accepts_a_missing_or_empty_result() {
    for body in [r#"{}"#, r#"{"result":""}"#, "not json at all"] {
        let (url, _seen) = scripted(vec![Resp::new(200, body)]).await;
        push(Kind::Transmission, &cfg(&url), MAGNET)
            .await
            .unwrap_or_else(|e| panic!("正文 {body:?} 在 JS 里算成功，这里却报错：{e}"));
    }
}

#[tokio::test]
async fn transmission_sends_basic_auth_only_when_credentials_are_given() {
    // 有凭据 → 带 Authorization
    let (url, seen) = scripted(vec![Resp::new(200, r#"{"result":"success"}"#)]).await;
    push(Kind::Transmission, &cfg_auth(&url, "u", "p"), MAGNET)
        .await
        .unwrap();
    assert!(lower(&nth(&seen, 0)).contains("authorization: basic "));

    // 两者都空 → **完全不带**认证（JS 的 `(user || pass) ? … : undefined`）
    let (url2, seen2) = scripted(vec![Resp::new(200, r#"{"result":"success"}"#)]).await;
    push(Kind::Transmission, &cfg(&url2), MAGNET).await.unwrap();
    assert!(!lower(&nth(&seen2, 0)).contains("authorization:"));
}

#[tokio::test]
async fn transmission_connection_refused_is_a_readable_error() {
    let err = test(Kind::Transmission, &cfg(&dead_url().await))
        .await
        .unwrap_err();
    assert!(err.contains("请求下载器失败"), "{err}");
}

// ---------------------------------------------------------------------------
// aria2 / Motrix
// ---------------------------------------------------------------------------

#[tokio::test]
async fn aria2_push_nests_the_uri_array_once() {
    let (url, seen) = scripted(vec![Resp::new(
        200,
        r#"{"jsonrpc":"2.0","id":"ts","result":"gid"}"#,
    )])
    .await;

    let out = push(Kind::Aria2, &cfg(&url), MAGNET).await.unwrap();
    assert_eq!(out, PushOutcome { status: 200 });

    let raw = nth(&seen, 0);
    assert!(raw.starts_with("POST /jsonrpc "), "{raw}");
    let body: Value = serde_json::from_str(body_of(&raw)).unwrap();
    assert_eq!(body["jsonrpc"], "2.0");
    assert_eq!(body["id"], "ts");
    assert_eq!(body["method"], "aria2.addUri");
    // ⚠️ 双层数组：参数本身是「URL 数组」
    assert_eq!(body["params"], json!([[MAGNET]]));
}

/// ⚠️ `token:` 是**平铺进 params 数组**的第一个元素，不是嵌套的。
#[tokio::test]
async fn aria2_token_is_prepended_flat_into_params() {
    let (url, seen) = scripted(vec![Resp::new(200, r#"{"result":"gid"}"#)]).await;

    push(Kind::Aria2, &cfg_token(&url, "s3cr3t"), MAGNET)
        .await
        .unwrap();

    let body: Value = serde_json::from_str(body_of(&nth(&seen, 0))).unwrap();
    assert_eq!(body["params"], json!(["token:s3cr3t", [MAGNET]]));
}

#[tokio::test]
async fn aria2_reports_rpc_errors_from_the_message_field() {
    let (url, _seen) = scripted(vec![Resp::new(
        200,
        r#"{"jsonrpc":"2.0","id":"ts","error":{"code":1,"message":"Unauthorized"}}"#,
    )])
    .await;

    let err = push(Kind::Aria2, &cfg(&url), MAGNET).await.unwrap_err();
    assert_eq!(err, "aria2: Unauthorized");
}

/// ⚠️ 真值语义：`error: 0` / `""` / `false` 在 JS 里都是**假** → 不算错。
#[tokio::test]
async fn aria2_treats_falsy_error_values_as_success() {
    for body in [
        r#"{"result":"gid","error":0}"#,
        r#"{"result":"gid","error":""}"#,
        r#"{"result":"gid","error":false}"#,
        r#"{"result":"gid","error":null}"#,
    ] {
        let (url, _seen) = scripted(vec![Resp::new(200, body)]).await;
        push(Kind::Aria2, &cfg(&url), MAGNET)
            .await
            .unwrap_or_else(|e| panic!("{body} 里的 error 是假值，不该报错：{e}"));
    }
}

/// 没 message 时退回 `JSON.stringify(error)`。
#[tokio::test]
async fn aria2_falls_back_to_the_stringified_error() {
    let (url, _seen) = scripted(vec![Resp::new(200, r#"{"error":{"code":42}}"#)]).await;
    let err = push(Kind::Aria2, &cfg(&url), MAGNET).await.unwrap_err();
    assert_eq!(err, r#"aria2: {"code":42}"#);
}

#[tokio::test]
async fn aria2_test_calls_get_version() {
    let (url, seen) = scripted(vec![Resp::new(200, r#"{"result":{"version":"1.36.0"}}"#)]).await;
    test(Kind::Aria2, &cfg(&url)).await.unwrap();
    let body: Value = serde_json::from_str(body_of(&nth(&seen, 0))).unwrap();
    assert_eq!(body["method"], "aria2.getVersion");
    assert_eq!(body["params"], json!([]));
}

#[tokio::test]
async fn aria2_connection_refused_is_a_readable_error() {
    let err = test(Kind::Aria2, &cfg(&dead_url().await))
        .await
        .unwrap_err();
    assert!(err.contains("请求下载器失败"), "{err}");
}

// ---------------------------------------------------------------------------
// Gopeed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gopeed_push_posts_the_req_shape_and_returns_the_status() {
    let (url, seen) = scripted(vec![Resp::new(201, r#"{"code":0,"msg":"","data":{}}"#)]).await;

    let out = push(Kind::Gopeed, &cfg_token(&url, "tk"), MAGNET)
        .await
        .unwrap();
    assert_eq!(out, PushOutcome { status: 201 }, "gopeed 取真实状态码");

    let raw = nth(&seen, 0);
    assert!(raw.starts_with("POST /api/v1/tasks "), "{raw}");
    assert!(lower(&raw).contains("x-api-token: tk"), "{raw}");
    let body: Value = serde_json::from_str(body_of(&raw)).unwrap();
    assert_eq!(body, json!({ "req": { "url": MAGNET } }));
}

#[tokio::test]
async fn gopeed_reports_non_zero_code_with_msg() {
    let (url, _seen) = scripted(vec![Resp::new(200, r#"{"code":7,"msg":"bad request"}"#)]).await;
    let err = push(Kind::Gopeed, &cfg(&url), MAGNET).await.unwrap_err();
    assert_eq!(err, "Gopeed: bad request");
}

#[tokio::test]
async fn gopeed_falls_back_to_code_n_when_msg_is_empty() {
    let (url, _seen) = scripted(vec![Resp::new(200, r#"{"code":7,"msg":""}"#)]).await;
    let err = push(Kind::Gopeed, &cfg(&url), MAGNET).await.unwrap_err();
    assert_eq!(err, "Gopeed: code 7");
}

/// ⚠️ `typeof data.code === 'number'` —— 字符串 code **不算失败**。
#[tokio::test]
async fn gopeed_ignores_a_non_numeric_code() {
    let (url, _seen) = scripted(vec![Resp::new(200, r#"{"code":"7","msg":"x"}"#)]).await;
    push(Kind::Gopeed, &cfg(&url), MAGNET)
        .await
        .expect("字符串 code 过不了 typeof number，JS 会放行");
}

/// ⚠️ JS 的 `test` 是 **GET**，且 `headers = {}` —— **不带 Content-Type**。
#[tokio::test]
async fn gopeed_test_is_a_get_without_content_type() {
    let (url, seen) = scripted(vec![Resp::new(200, r#"{"code":0,"data":[]}"#)]).await;

    test(Kind::Gopeed, &cfg_token(&url, "tk")).await.unwrap();

    let raw = nth(&seen, 0);
    assert!(raw.starts_with("GET /api/v1/tasks "), "{raw}");
    assert!(
        !lower(&raw).contains("content-type:"),
        "test 那次不该带 Content-Type（照抄 JS）：{raw}"
    );
    assert!(lower(&raw).contains("x-api-token: tk"));
}

#[tokio::test]
async fn gopeed_test_fails_on_a_non_2xx() {
    let (url, _seen) = scripted(vec![Resp::new(401, "unauthorized")]).await;
    let err = test(Kind::Gopeed, &cfg(&url)).await.unwrap_err();
    assert!(err.contains("401"), "{err}");
}

#[tokio::test]
async fn gopeed_connection_refused_is_a_readable_error() {
    let err = test(Kind::Gopeed, &cfg(&dead_url().await))
        .await
        .unwrap_err();
    assert!(err.contains("请求下载器失败"), "{err}");
}

// ---------------------------------------------------------------------------
// detect：命中第一个即返回
// ---------------------------------------------------------------------------

#[tokio::test]
async fn detect_returns_the_first_candidate_that_answers() {
    // 候选 1：qB 登录失败（403）→ 必须被跳过
    let (bad_url, bad_seen) = scripted(vec![Resp::new(403, "Fails.")]).await;
    // 候选 2：Transmission 的 session-get 成功 → 命中
    let (good_url, _good_seen) = scripted(vec![Resp::new(200, r#"{"result":"success"}"#)]).await;

    let candidates = vec![
        bt_downloaders::DetectCandidate {
            kind: Kind::Qbittorrent,
            url: bad_url,
            user: "admin".into(),
            pass: "wrong".into(),
        },
        bt_downloaders::DetectCandidate {
            kind: Kind::Transmission,
            url: good_url.clone(),
            user: String::new(),
            pass: String::new(),
        },
        // 第三条根本不该被尝试
        bt_downloaders::DetectCandidate {
            kind: Kind::Gopeed,
            url: "http://127.0.0.1:1".into(),
            user: String::new(),
            pass: String::new(),
        },
    ];

    let hit = detect_from(&candidates).await.expect("第二个候选该命中");

    assert_eq!(hit.kind, Kind::Transmission);
    assert_eq!(hit.config.url, good_url);
    assert_eq!(
        bad_seen.lock().unwrap().len(),
        1,
        "第一个候选只该被试一次（登录就是它唯一的一次请求）"
    );
}
