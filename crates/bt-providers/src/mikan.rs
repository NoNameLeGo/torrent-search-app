//! Mikan（`mikanani.me`，蜜柑计划）—— HTML 抓取。
//!
//! 从 `src/providers/mikan.js`（82 行）移植。
//!
//! ## 与别的 HTML 站不同的两点
//!
//! 1. **磁力不在 `href` 里，而在 `data-clipboard-text` 属性上**
//!    （站上那个「复制磁力」按钮）。`td:nth-child(2) > a[data-clipboard-text]`。
//! 2. **名称与大小/日期都走 `ownText`**（只取直接子文本节点）—— 标题里带
//!    `<span>` 包着的字幕组标签，`.text()` 会把它们拼进来，而 JS 不会。
//!
//! ## 一处**丢字段**（不是遗漏，是当前模型装不下）
//!
//! JS 会带上 `fileDownloadLink`（第 5 格那个 `.torrent` 直链），而 Rust 侧的
//! `TorrentResult` **没有这个字段**（16 家老 provider 都不需要它）。
//! 为了不牵动 `bt-core` 的结果模型 + UI + 全部 provider，这里**丢弃**该字段。
//! 影响：UI 上少一个「下载种子文件」的入口，磁力与详情页都还在。
//! 要补的话是独立一件事（加字段 → 改 normalize → 改 UI → 16 家回归）。

use bt_core::dom::{attr, own_text, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{
    encode_uri_component, extract_info_hash, normalize, NumOrText, RawResult, TorrentResult,
};

use crate::SearchOutcome;

/// 站点根（JS 的 `BASE`）。
pub const BASE: &str = "https://mikanani.me";

/// 唯一镜像。JS 里 `DOMAINS = [BASE]`，仍走 `runMirrors`（照抄）。
const DOMAINS: &[&str] = &[BASE];

/// 结果行：站上给每行加了 `js-search-results-row` 类。
const ROWS: &str = "tr.js-search-results-row";

/// 名称链接：第 2 格里的**第 1 个** `<a>`（后面还有磁力/种子的图标链接）。
const NAME_LINK: &str = "td:nth-child(2) > a:nth-child(1)";

/// 磁力：第 2 格里那个带 `data-clipboard-text` 的 `<a>`。
const MAGNET_LINK: &str = "td:nth-child(2) > a[data-clipboard-text]";

const SIZE_CELL: &str = "td:nth-child(3)";
const DATE_CELL: &str = "td:nth-child(4)";

/// 用默认镜像搜索。`page` 与 JS 一样**被忽略**（该站一次给完）。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。JS 传的 name 是小写 `'mikan'`。
pub async fn search_with(http: &HttpClient, bases: &[&str], query: &str) -> SearchOutcome {
    crate::run_mirrors("mikan", bases, |b| search_at(http, b, query)).await
}

/// `/Home/Search?searchstr=<q>`（注意是 `searchstr`，不是 `q`）。
fn search_url(base: &str, query: &str) -> String {
    format!(
        "{base}/Home/Search?searchstr={}",
        encode_uri_component(query)
    )
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    let url = search_url(base, query);

    let resp = http.get_text(&url, None).await;
    // JS: `if (error || !html) return { results: [], error: error || 'no_html' }`
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    let rows = dom.select(ROWS);
    // JS: `if (rows.length === 0) return { results: [], error: null }`
    if rows.is_empty() {
        return SearchOutcome::ok(Vec::new());
    }

    let results = rows
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    SearchOutcome::ok(results)
}

/// 解析一行；缺名称或缺磁力就整行丢掉（JS 的 `return null`）。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let name_link = dom.find(row, NAME_LINK).into_iter().next()?;
    let name = own_text(name_link);
    if name.is_empty() {
        return None;
    }

    let magnet = dom
        .find(row, MAGNET_LINK)
        .into_iter()
        .next()
        .and_then(|a| attr(a, "data-clipboard-text"))
        .filter(|m| !m.is_empty())?;

    let size = own_text_cell(dom, row, SIZE_CELL);
    let date = own_text_cell(dom, row, DATE_CELL);

    let detail_url = attr(name_link, "href")
        .filter(|h| !h.is_empty())
        .map(|href| {
            if href.starts_with("http") {
                href
            } else {
                format!("{base}{href}")
            }
        });

    Some(normalize(&RawResult {
        provider: "mikan".to_string(),
        name: Some(name),
        // JS: `size: size || null` / `date: dateText || null`
        size: non_empty_nt(size),
        date: non_empty_nt(date),
        info_hash: extract_info_hash(Some(&magnet)),
        magnet: Some(magnet),
        category: Some("Anime".to_string()),
        detail_url,
        ..Default::default()
    }))
}

/// 取格子的 `ownText`（JS 的 `ownText($row.find(cell))`）。
fn own_text_cell(dom: &Dom, row: ElementRef<'_>, cell: &str) -> String {
    dom.find(row, cell)
        .into_iter()
        .next()
        .map(own_text)
        .unwrap_or_default()
}

/// 空串 → `None`（对齐 JS 的 `x || null`）。
fn non_empty_nt(s: String) -> Option<NumOrText> {
    if s.trim().is_empty() {
        None
    } else {
        Some(NumOrText::Text(s))
    }
}
