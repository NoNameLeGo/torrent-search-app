//! BD电影首发（`bdflixs.com`）—— WordPress 搜索 + 详情页磁力（**惰性**）。
//!
//! ## 为什么是 `needs_magnet` 而不是搜索时展开
//!
//! 搜索结果页只回**影片条目**（`article.post-item-list`），磁力全在详情页
//! （实测 `/594.html` 有 102 条）。一个影片页有几十到上百条磁力，而
//! [`bt_core::TorrentResult`] 一行只能放一个磁力 —— 所以这里按项目已有的
//! 6 家惰性模式做：搜索只回影片，用户点某条时 [`resolve_magnet`] 再取**第一条**
//! （详情页第一条是最高规格，实测是 2160p DV）。
//!
//! ## 页面结构（2026-10-25 真 fixture）
//!
//! ```text
//! 搜索页：<article class="post-item-list ...">
//!           <h2 class="entry-title"><a href="https://www.bdflixs.com/594.html">标题</a></h2>
//! 详情页：<a href="magnet:?xt=urn:btih:...">片名[规格] - 26.02GB</a>
//! ```
//!
//! 详情页的锚文本带体积（` - 26.02GB`），但 `resolve_magnet` 只返回磁力本身；
//! 体积要等「展开多条磁力」的能力落地后再用。

use bt_core::dom::{attr, text_trim, Dom};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, extract_info_hash, normalize, RawResult};

use crate::{MagnetOutcome, SearchOutcome};

/// 站点根。
pub const BASE: &str = "https://www.bdflixs.com";

/// 搜索结果里的影片条目（WordPress 主题的固定结构）。
const POST_LINKS: &str = "article.post-item-list h2.entry-title > a";

/// 用默认站点搜索。`page` 忽略（WordPress 搜索页没有可靠的分页契约）。
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
    let links = dom.select(POST_LINKS);
    if links.is_empty() {
        return SearchOutcome::ok(Vec::new());
    }

    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for a in links {
        let name = text_trim(a);
        if name.is_empty() {
            continue;
        }
        let Some(href) = attr(a, "href").filter(|h| !h.is_empty()) else {
            continue;
        };
        let detail = detail_url(base, &href);
        if !seen.insert(detail.clone()) {
            continue;
        }

        results.push(normalize(&RawResult {
            provider: "bdflixs".to_string(),
            name: Some(name),
            detail_url: Some(detail),
            category: Some("Movies".to_string()),
            // 搜索阶段不碰详情页 —— 与 6 家惰性 provider 同一条约定。
            magnet: None,
            ..Default::default()
        }));
    }

    SearchOutcome::ok(results)
}

/// 取详情页的**第一条**磁力。
///
/// 详情页第一条是最高规格（实测 `2160p DV TrueHD`）；「一个影片多条磁力」
/// 要等 UI 有展开能力后再做，不在 provider 里假装成多条结果。
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

/// `/?s=<q>`。
fn search_url(base: &str, query: &str) -> String {
    format!(
        "{}/?s={}",
        base.trim_end_matches('/'),
        encode_uri_component(query)
    )
}

/// 详情页绝对地址。
///
/// ⚠️ 搜索页给的是**绝对**地址（`https://www.bdflixs.com/594.html`）；
/// 测试时 base 是本地一次性服务，所以要把站点根换成 base —— 生产环境
/// `base == BASE`，这段是恒等变换。
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
