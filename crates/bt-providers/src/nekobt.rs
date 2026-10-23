//! NekoBT（`nekobt.to`，中文/日文番剧）—— HTML 抓取。
//!
//! 从 `src/providers/nekobt.js`（65 行）移植。站是 Svelte 渲染的 Tailwind 页面，
//! 但结果仍在 `<table class="table">` 里，磁力直接在行内。
//!
//! ## 选择器链（逐字照抄 JS，别"顺手简化"）
//!
//! ```text
//! tr
//!  ├ td:nth-child(1)  勾选框
//!  ├ td:nth-child(2)  分类图标
//!  ├ td:nth-child(3) > div:nth-child(1) > div > a   ← 名称链接
//!  │        └ span > span:nth-child(1)              ← **名字只取这一层**
//!  ├ td:nth-child(4) > div > a:nth-child(1)         ← 磁力
//!  └ td:nth-child(5/6/7/8) > span                  ← 大小 / 日期 / 种子 / 下载
//! ```
//!
//! ⚠️ **名字只取 `span > span:nth-child(1)`**：那一层里通常只有标题，
//! 但旁边还有语言徽章等元素，用整行 `.text()` 会拼进多余文本。
//!
//! ## 测试强度的说明（重要）
//!
//! 本机没有 `cheerio`，所以**算不出"应该有几行"**（HTML provider 的 Node 侧
//! 只能在 CI 跑）。因此这里不做精确条数断言，而是：
//! ① 断言**非空**（防止选择器写错后与 Node 侧"0 对 0"假通过 —— 差分门比条数，
//!   两边都 0 也会算一致，那是这套 harness 的盲点）；
//! ② 断言每一行都带磁力 + 名字 + 40 位 hash；
//! ③ 精确的逐字段一致性交给 **M6 差分门**（它拿同一份 fixture 与 Node 版逐字段比）。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{
    encode_uri_component, extract_info_hash, normalize, NumOrText, RawResult, TorrentResult,
};

use crate::SearchOutcome;

/// 站点根（JS 的 `BASE`）。
pub const BASE: &str = "https://nekobt.to";

/// 唯一镜像。JS 里 `DOMAINS = [BASE]`，仍走 `runMirrors`（照抄）。
const DOMAINS: &[&str] = &[BASE];

const ROWS: &str = "table.table > tbody > tr";
const NAME_LINK: &str = "td:nth-child(3) > div:nth-child(1) > div > a";
const NAME_INNER: &str = "span > span:nth-child(1)";
const MAGNET_LINK: &str = "td:nth-child(4) > div > a:nth-child(1)";
const SIZE_CELL: &str = "td:nth-child(5) > span";
const DATE_CELL: &str = "td:nth-child(6) > span";
const SEEDERS_CELL: &str = "td:nth-child(7) > span";
const LEECHERS_CELL: &str = "td:nth-child(8) > span";

/// 用默认镜像搜索。`page` 与 JS 一样**被忽略**。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。⚠️ JS 传的 name 是 **`'NekoBT'`**（大写）。
pub async fn search_with(http: &HttpClient, bases: &[&str], query: &str) -> SearchOutcome {
    crate::run_mirrors("NekoBT", bases, |b| search_at(http, b, query)).await
}

/// `/search?query=<q>`
fn search_url(base: &str, query: &str) -> String {
    format!("{base}/search?query={}", encode_uri_component(query))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    let url = search_url(base, query);

    let resp = http.get_text(&url, None).await;
    // JS: `if (error || !html) return { base, results: [], error }`
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    let rows = dom.select(ROWS);
    // JS: `if (rows.length === 0) return []`（不是错误）
    if rows.is_empty() {
        return SearchOutcome::ok(Vec::new());
    }

    let results = rows
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    SearchOutcome::ok(results)
}

/// 解析一行；缺名字或缺磁力就丢掉（JS 的两个 `return`）。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let name_link = dom.find(row, NAME_LINK).into_iter().next()?;
    let name = dom
        .find(name_link, NAME_INNER)
        .into_iter()
        .next()
        .map(text_trim)
        .unwrap_or_default();
    if name.is_empty() {
        return None;
    }

    let magnet = dom
        .find(row, MAGNET_LINK)
        .into_iter()
        .next()
        .and_then(|a| attr(a, "href"))
        .filter(|m| !m.is_empty())?;

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
        provider: "nekobt".to_string(),
        name: Some(name),
        // JS 这里都不 trim，交给 normalize 里的解析器处理
        size: Some(NumOrText::Text(cell_text(dom, row, SIZE_CELL))),
        date: Some(NumOrText::Text(cell_text(dom, row, DATE_CELL))),
        seeders: Some(NumOrText::Text(cell_text(dom, row, SEEDERS_CELL))),
        leechers: Some(NumOrText::Text(cell_text(dom, row, LEECHERS_CELL))),
        info_hash: extract_info_hash(Some(&magnet)),
        magnet: Some(magnet),
        category: Some("Anime".to_string()),
        detail_url,
        ..Default::default()
    }))
}

/// 取某个格子的 `.text()`（JS 原样不 trim）。
fn cell_text(dom: &Dom, row: ElementRef<'_>, cell: &str) -> String {
    dom.find(row, cell)
        .into_iter()
        .next()
        .map(bt_core::dom::text)
        .unwrap_or_default()
}
