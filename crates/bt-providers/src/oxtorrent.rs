//! OxTorrent —— 法站 HTML 抓取（B 组第 8 个）。
//!
//! 从 `src/providers/oxtorrent.js`（61 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表 `table > tbody > tr`（fixture 里 **3 行**，**没有表头行** —— 与 torrent9 不同），
//! 每行 4 个 td：
//!
//! ```text
//! 1  <i class="Logiciels"></i> <a href="/torrent/48951/…">Ubuntu 10.04 Desktop (32 bits)</a>
//! 2  700.4 MB     ← 大小
//! 3  9            ← seeders（`<img alt="seeders">9`）
//! 4  2            ← leechers
//! ```
//!
//! ## ⚠️ 上游 bug：后三格 + 分类全被丢掉（照抄，未修）
//!
//! 与 `torrent9` 同一个毛病，而且这里更明显：
//!
//! ```js
//! normalize({ provider: 'oxtorrent', name, detailUrl, needsMagnet: true });
//! ```
//!
//! `700.4 MB` / `9` / `2` 就在同一行，`<i class="Logiciels">` 连分类都标好了
//! （法语 `Logiciels` = 软件 → 本该是 `Apps`），**JS 一个都不要** ——
//! 于是卡片上除名字外全是 `—`，还要等用户点击时再去详情页跑一趟。
//!
//! ## 磁力也是惰性的
//!
//! 列表页没有磁力，详情页在 `div.btn-magnet > a` 里
//! （页面别处还有个 `div.btn-download > a` 指向**同一条**磁力，JS 只认前者）。
//! `resolveMagnet` **不返回 infoHash**（与 `therarbg` 一样，与 `torrent9` 不同）。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, normalize, RawResult};
use bt_core::TorrentResult;

use crate::{MagnetOutcome, SearchOutcome};

/// 镜像列表，与 JS 的 `DOMAINS` 一致。
/// ⚠️ `oxtorrent.so` 实测 **ENOTFOUND**（2026-10-10），保留只为对齐 JS。
pub const DOMAINS: &[&str] = &["https://oxtorrent.co", "https://oxtorrent.so"];

/// 结果行（**不含表头** —— 这一站的结果表没有表头行）。
const ROWS: &str = "table > tbody > tr";

/// 名称 + 详情链接。
const NAME_LINK: &str = "td:nth-child(1) > a";

/// 详情页里的磁力链（注意同级还有个 `btn-download`，JS 不认）。
const MAGNET_LINK: &str = "div.btn-magnet > a";

/// 用默认镜像搜索。`page` 与 JS 一样**收下但不用** —— 这个站的 `searchOne` 收了
/// `page` 却根本没往 URL 里放。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。
///
/// ⚠️ JS 传的 name 是 **`'OxTorrent'`**，错误串必须逐字一致。
pub async fn search_with(http: &HttpClient, bases: &[&str], query: &str) -> SearchOutcome {
    let mut errs: Vec<String> = Vec::new();

    for base in bases {
        let outcome = search_at(http, base, query).await;
        if !outcome.results.is_empty() {
            return SearchOutcome::ok(outcome.results);
        }
        if let Some(e) = outcome.error {
            errs.push(e);
        }
    }

    SearchOutcome::err(format!("OxTorrent unreachable ({})", errs.join("; ")))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    // JS: `${base}/recherche/${encodeURIComponent(query)}` —— 没有页码参数
    let url = format!("{base}/recherche/{}", encode_uri_component(query));

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::ok(Vec::new());
    };

    let dom = Dom::parse(&html);
    // ⚠️ 这一站的错误串是 `no_results`（不带 `_parsed`），别"顺手统一"掉
    if dom.select(ROWS).is_empty() {
        return SearchOutcome::err("no_results");
    }

    let results: Vec<TorrentResult> = dom
        .select(ROWS)
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    // 有行但全被过滤掉 → JS 返回 `error: null`（与 therarbg / limetorrents 同款怪癖）
    SearchOutcome::ok(results)
}

/// 解析一行。只取名称与详情链接 —— 后三格与分类照抄 JS 丢掉（见文件头）。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let link = dom.find(row, NAME_LINK).into_iter().next()?;

    // JS: `if (!name || !href) return;` —— 两个都得有
    let name = text_trim(link);
    if name.is_empty() {
        return None;
    }
    let href = attr(link, "href").filter(|h| !h.is_empty())?;

    let detail_url = if href.starts_with("http") {
        href
    } else {
        format!("{base}{href}")
    };

    Some(normalize(&RawResult {
        provider: "oxtorrent".to_string(),
        name: Some(name),
        // 列表页没有磁力 → normalize 自动把 needs_magnet 置 true
        magnet: None,
        detail_url: Some(detail_url),
        ..Default::default()
    }))
}

/// 惰性解析磁力。对应 JS 的 `resolveMagnet(detailUrl)`。
///
/// ⚠️ 错误串三种：HTTP 错误**原样透出**（与 `torrent9` 不同）、空正文给 `no_html`、
/// 找不到链接给 `no_magnet_on_page`。成功时**不带 `info_hash`**（照抄 JS）。
pub async fn resolve_magnet(http: &HttpClient, detail_url: &str) -> MagnetOutcome {
    let resp = http.get_text(detail_url, None).await;
    if let Some(e) = resp.error {
        return MagnetOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return MagnetOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    match dom
        .select(MAGNET_LINK)
        .first()
        .and_then(|a| attr(*a, "href"))
        .filter(|h| !h.is_empty())
    {
        Some(href) => MagnetOutcome {
            magnet: Some(href),
            info_hash: None,
            error: None,
        },
        None => MagnetOutcome::err("no_magnet_on_page"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://oxtorrent.co";

    fn row(name: &str, href: &str, size: &str, seeds: &str, leech: &str) -> String {
        format!(
            "<tr><td><i class=\"Logiciels\"></i><a href=\"{href}\" title=\"{name} en Torrent\">{name}</a></td>\
             <td>{size}</td><td><img src=\"/static/img/uploader.png\" alt=\"seeders\">{seeds}</td>\
             <td><img src=\"/static/img/downloader.png\" alt=\"leechers\">{leech}</td></tr>"
        )
    }

    fn page(rows: &str) -> String {
        format!("<html><body><table><tbody>{rows}</tbody></table></body></html>")
    }

    fn parse_page(rows: &str) -> Vec<TorrentResult> {
        let dom = Dom::parse(&page(rows));
        dom.select(ROWS)
            .iter()
            .filter_map(|r| parse_row(&dom, *r, BASE))
            .collect()
    }

    #[test]
    fn parses_a_row_end_to_end() {
        let out = parse_page(&row(
            "Ubuntu 10.04 Desktop (32 bits)",
            "/torrent/48951/ubuntu-10-04-desktop-32-bits",
            "700.4 MB",
            "9",
            "2",
        ));

        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.provider, "oxtorrent");
        assert_eq!(r.name, "Ubuntu 10.04 Desktop (32 bits)");
        assert_eq!(
            r.detail_url.as_deref(),
            Some("https://oxtorrent.co/torrent/48951/ubuntu-10-04-desktop-32-bits")
        );
        assert!(r.needs_magnet);
        assert_eq!(r.magnet, None);
        assert_eq!(r.info_hash, None);

        // ⚠️ 站上第 2~4 格明明有 700.4 MB / 9 / 2 —— JS 不要，照抄
        assert_eq!(r.size, None);
        assert_eq!(r.size_text, "—");
        assert_eq!(r.seeders, None);
        assert_eq!(r.leechers, None);
        assert_eq!(r.date, None);
        assert_eq!(r.category, None);
    }

    #[test]
    fn rows_without_name_or_href_are_skipped() {
        let rows = format!(
            "{}{}{}",
            row("", "/torrent/1/x", "1 MB", "1", "0"),
            "<tr><td><a>no href</a></td></tr>",
            row("ok", "/torrent/2/x", "1 MB", "1", "0")
        );
        let out = parse_page(&rows);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "ok");
    }

    #[test]
    fn absolute_detail_urls_are_kept_verbatim() {
        let out = parse_page(&row(
            "n",
            "https://mirror.example/torrent/1/x",
            "1 MB",
            "1",
            "0",
        ));
        assert_eq!(
            out[0].detail_url.as_deref(),
            Some("https://mirror.example/torrent/1/x")
        );
    }

    /// 这一站**没有表头行** —— 3 行就是 3 条，不像 rutor / limetorrents 要减 1。
    #[test]
    fn every_row_is_a_result_because_there_is_no_header_row() {
        let rows = format!(
            "{}{}{}",
            row("a", "/torrent/1/x", "1 MB", "1", "0"),
            row("b", "/torrent/2/x", "2 MB", "2", "1"),
            row("c", "/torrent/3/x", "3 MB", "3", "2")
        );
        assert_eq!(parse_page(&rows).len(), 3);
    }
}
