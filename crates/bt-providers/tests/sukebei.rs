//! `sukebei` 的测试。
//!
//! ⚠️ **没有 sukebei 自己的 fixture**：`sukebei.nyaa.si` 两个环境都探不通
//! （`UND_ERR_CONNECT_TIMEOUT`，见 `AGENTS.md`「受阻引擎清单」），抓不到真快照。
//!
//! 但这是**安全的**：本站不实现自己的解析器 —— 它调 [`nyaa::search_as`]，
//! 而 `nyaa` 的解析已经被 `tests/nyaa.rs` 用真实快照钉了 7 条。
//! 这里要钉的是**复用路径本身**是不是对的：
//!
//! 1. 结果里的 `provider` 必须是 `"sukebei"`（不是 `"nyaa"` —— 否则来源列会标错）
//! 2. 请求打的是传入的 base（生产环境 = `https://sukebei.nyaa.si`）
//! 3. 错误前缀用 `Sukebei`，不是 `NYAA`（否则用户以为 nyaa 挂了）

mod common;

use bt_core::http::HttpClient;
use bt_providers::{nyaa, sukebei};
use common::fixture;

#[tokio::test]
async fn reusing_the_nyaa_parser_tags_rows_as_sukebei() {
    let url = common::oneshot(200, &fixture("nyaa-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let out = sukebei::search_at(&HttpClient::new(), &base, "ubuntu", 1).await;

    assert_eq!(out.error, None);
    assert_eq!(out.results.len(), 1, "nyaa 的 fixture 只有 1 行");

    let r = &out.results[0];
    // ⚠️ 这条是本次移植的核心断言：解析复用 ≠ 来源标错
    assert_eq!(r.provider, "sukebei");
    assert_eq!(r.id, "sukebei:45008e48c8800b7d7643337b2e70a634e4c69f6a");
    // 其余字段由 nyaa 的解析器决定，已在 tests/nyaa.rs 逐字段钉过，这里只抽两处
    assert_eq!(r.size, Some(654_311_424), "624.0 MiB");
    assert!(r.magnet.is_some(), "磁力在 Link 格里，复用后仍然拿得到");
}

/// 与 nyaa 同款 URL 形状，只换 base。
#[tokio::test]
async fn request_url_keeps_the_nyaa_contract() {
    let (url, seen) = common::oneshot_capture(200, &fixture("nyaa-ubuntu.html")).await;
    let base = url.trim_end_matches('/').to_string();

    let _ = sukebei::search_at(&HttpClient::new(), &base, "ubuntu 22", 2).await;

    let raw = seen.lock().unwrap().clone();
    assert!(raw.starts_with("GET "), "必须是 GET: {raw}");
    assert!(raw.contains("/?f=0&c=0_0&q=ubuntu%2022&page=2 "), "{raw}");
}

/// 错误前缀必须是本站在报错，而不是把 nyaa 的名字顶上去。
#[tokio::test]
async fn http_error_uses_the_sukebei_prefix() {
    let url = common::oneshot(503, "nope").await;

    let out = sukebei::search_at(&HttpClient::new(), &url, "ubuntu", 1).await;

    assert!(out.results.is_empty());
    let e = out.error.as_deref().expect("该有错误");
    assert!(e.starts_with("Sukebei unreachable ("), "{e}");
}

/// 默认域名（生产环境）。
#[test]
fn the_default_domain_is_sukebei_nyaa_si() {
    assert_eq!(sukebei::BASE, "https://sukebei.nyaa.si");
    assert_eq!(sukebei::ID, "sukebei");
    // 复用的是 nyaa 的实现，不是复制品 —— base 必须仍然不同
    assert_ne!(sukebei::BASE, nyaa::BASE);
}
