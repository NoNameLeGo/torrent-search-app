//! 磁力熊（`cilixiong.org`）—— EmpireCMS 表单 POST 搜索 + 详情页磁力（**惰性**）。
//!
//! ## 为什么必须 POST
//!
//! 搜索端点是 `/e/search/index.php`，EmpireCMS 只认 `application/x-www-form-urlencoded`
//! 的 POST；GET 同一组参数会返回「信息提示 / 没有搜索到相关的内容」（2026-10-25 实测）。
//! 参数里 `classid=1,2` **不能省**（电影 + 剧集两个栏目），少了也搜不到。
//!
//! ## 页面结构（2026-10-25 真 fixture）
//!
//! ```text
//! 搜索页：<div class="col"><div class="card ..."><a href="/movie/4167.html">
//!           <h2 class="...">流浪地球2：再次冒险</h2> ...
//! 详情页：<a href="magnet:?xt=urn:btih:...">片名[规格]</a>
//! ```
//!
//! 搜索结果只回影片条目（`needs_magnet`），磁力由 [`resolve_magnet`] 惰性取第一条
//! （详情页实测 4 条）。站点响应偏慢（实测 7s），且连续快速 POST 会超时 ——
//! 这是站点限流，不是解析问题。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{extract_info_hash, normalize, RawResult};

use crate::{MagnetOutcome, SearchOutcome};

/// 站点根。
pub const BASE: &str = "https://www.cilixiong.org";

/// 用默认站点搜索。`page` 忽略（EmpireCMS 搜索页没有可靠的分页契约）。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_at(http, BASE, query).await
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    let url = format!("{}/e/search/index.php", base.trim_end_matches('/'));

    // ⚠️ `classid=1,2` 是电影 + 剧集两个栏目，少了搜索直接返回「没有搜索到」。
    let resp = http
        .post_form(
            &url,
            &[
                ("keyboard", query),
                ("show", "title"),
                ("tempid", "1"),
                ("classid", "1,2"),
            ],
            None,
        )
        .await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for a in dom.select("a") {
        let Some(href) = attr(a, "href").filter(|h| !h.is_empty()) else {
            continue;
        };
        // 只要 /movie/<id>.html 或 /drama/<id>.html；导航里的 /movie/ 不算
        let is_movie = href.starts_with("/movie/") && href.ends_with(".html");
        let is_drama = href.starts_with("/drama/") && href.ends_with(".html");
        if !is_movie && !is_drama {
            continue;
        }

        let name = title_of(&dom, a);
        if name.is_empty() {
            continue;
        }
        let detail = absolute_url(base, &href);
        if !seen.insert(detail.clone()) {
            continue;
        }

        results.push(normalize(&RawResult {
            provider: "cilixiong".to_string(),
            name: Some(name),
            detail_url: Some(detail),
            category: Some(if is_drama { "Series" } else { "Movies" }.to_string()),
            magnet: None,
            ..Default::default()
        }));
    }

    SearchOutcome::ok(results)
}

/// 取详情页的**第一条**磁力。
pub async fn resolve_magnet(http: &HttpClient, detail_url: &str) -> MagnetOutcome {
    let resp = http.get_text(detail_url, None).await;
    if let Some(e) = resp.error {
        return MagnetOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return MagnetOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    let magnet = dom
        .select("a")
        .into_iter()
        .find_map(|a| attr(a, "href").filter(|h| h.starts_with("magnet:")));

    match magnet {
        Some(m) => MagnetOutcome {
            info_hash: extract_info_hash(Some(&m)),
            magnet: Some(m),
            error: None,
        },
        None => MagnetOutcome::err("no_magnet"),
    }
}

/// 卡片标题：优先子 `h2`（磁力熊的卡片把片名放在 `<h2>` 里），否则锚文本。
fn title_of(dom: &Dom, a: ElementRef<'_>) -> String {
    if let Some(h) = dom.find(a, "h2").into_iter().next() {
        let t = text_trim(h);
        if !t.is_empty() {
            return t;
        }
    }
    text_trim(a)
}

/// 相对链接转绝对；测试时把站点根换成 base（生产环境是恒等变换）。
fn absolute_url(base: &str, href: &str) -> String {
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
