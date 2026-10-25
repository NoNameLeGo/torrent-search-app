//! POMO（`pomo.mom`）—— 4K 原盘站，搜索 + 详情页磁力（**惰性**）。
//!
//! ## 与 bdflixs 同一套形状
//!
//! 搜索页只回影片卡片（`div.bg-cardbg h3`），磁力全在详情页（实测 `/199` 有 56 条）。
//! 所以同样走 `needs_magnet` + [`resolve_magnet`]：搜索不碰详情，用户点击时取第一条
//! （详情页第一条是最高规格）。
//!
//! ## 页面结构（2026-10-25 真 fixture）
//!
//! ```text
//! 搜索页：<div class="bg-cardbg ..."><a href="https://pomo.mom/199" ...>
//!           <h3 class="...">流浪地球2</h3>
//! 详情页：<a href="magnet:?xt=urn:btih:...">片名[规格]</a>
//! ```
//!
//! ⚠️ 站点装了 `Ixc_login` 登录插件：匿名能读到磁力（实测），但部分链接可能要 VIP。
//! provider 只取页面上**匿名可见**的磁力，不做登录。

use bt_core::dom::{attr, closest_tag, text_trim, Dom};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, extract_info_hash, normalize, RawResult};

use crate::{MagnetOutcome, SearchOutcome};

/// 站点根。
pub const BASE: &str = "https://pomo.mom";

/// 搜索结果的影片卡片标题。
const CARD_TITLES: &str = "div.bg-cardbg h3";

/// 用默认站点搜索。`page` 忽略（搜索页没有可靠的分页契约）。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_at(http, BASE, query).await
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    let url = search_url(base, query);

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    let titles = dom.select(CARD_TITLES);
    if titles.is_empty() {
        return SearchOutcome::ok(Vec::new());
    }

    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for h3 in titles {
        let name = text_trim(h3);
        if name.is_empty() {
            continue;
        }
        let Some(a) = closest_tag(h3, "a") else {
            continue;
        };
        let Some(href) = attr(a, "href").filter(|h| !h.is_empty()) else {
            continue;
        };
        let detail = detail_url(base, &href);
        if !seen.insert(detail.clone()) {
            continue;
        }

        results.push(normalize(&RawResult {
            provider: "pomo".to_string(),
            name: Some(name),
            detail_url: Some(detail),
            category: Some("Movies".to_string()),
            magnet: None,
            ..Default::default()
        }));
    }

    SearchOutcome::ok(results)
}

/// 取详情页的**第一条**磁力（最高规格；与 bdflixs 同一条约定）。
pub async fn resolve_magnet(http: &HttpClient, detail_url: &str) -> MagnetOutcome {
    let resp = http.get_text(detail_url, None).await;
    if let Some(e) = resp.error {
        return MagnetOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return MagnetOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    let magnet = dom.select("a").into_iter().find_map(|a| {
        let href = attr(a, "href")?;
        if href.starts_with("magnet:") {
            Some(href)
        } else {
            None
        }
    });

    match magnet {
        Some(m) => MagnetOutcome {
            info_hash: extract_info_hash(Some(&m)),
            magnet: Some(m),
            error: None,
        },
        None => MagnetOutcome::err("no_magnet"),
    }
}

/// `/?keyword=<q>`。
fn search_url(base: &str, query: &str) -> String {
    format!(
        "{}/?keyword={}",
        base.trim_end_matches('/'),
        encode_uri_component(query)
    )
}

/// 详情页绝对地址（测试时把站点根换成 base；生产环境是恒等变换）。
fn detail_url(base: &str, href: &str) -> String {
    let base = base.trim_end_matches('/');
    if let Some(rest) = href.strip_prefix(BASE) {
        format!("{base}{rest}")
    } else if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if href.starts_with('/') {
        format!("{base}{href}")
    } else {
        format!("{base}/{href}")
    }
}
