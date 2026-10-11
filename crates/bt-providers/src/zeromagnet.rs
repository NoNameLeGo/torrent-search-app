//! ZeroMagnet (ØMagnet / 9mag.net) —— HTML 抓取（B 组第 12 个）。
//!
//! 从 `src/providers/zeromagnet.js`（60 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表 `table.file-list > tbody > tr`（fixture **29 行、没有表头行**）：
//!
//! ```text
//! <tr>
//!   <td class="result-title"><a href="/!lFvU"><mark>ubuntu</mark>-24.04.4-desktop-amd64.iso</a></td>
//!   <td class="result-meta"><div>6.2GB</div><div class="result-date">2026-08-07</div></td>
//! </tr>
//! ```
//!
//! ⚠️ 名称里的关键词被 `<mark>` 包住 → 必须**拼接后代文本**（`text_trim` 的语义），
//! 否则每个名字都缺一截。详情链接是短 ID 形式 `/!xxxx`。
//!
//! ## ⚠️ 上游 bug：meta 列白扔（照抄，未修）
//!
//! `td:nth-child(2)` 里就摆着 `6.2GB` 和 `2026-08-07`，JS 只传
//! `name` / `detailUrl` / `category:'Porn'` —— 与 `oxtorrent` / `torrent9` 同款毛病
//! （探针 `meta_sizes` / `meta_dates` 把「数据其实拿得到」这个事实钉住）。
//!
//! ## resolveMagnet
//!
//! 磁力在详情页 `input#input-magnet` 的 **value 属性**里（不是 href）。
//! 错误行为与 `oxtorrent` 一致：HTTP 错误**原样透出**、空正文 `no_html`、
//! 找不到给 `no_magnet_on_page`。成功时**不带 info_hash**（照抄 JS，不抠）。
//! ⚠️ 页面上另有一条 `a[href^="magnet:"]`（复制按钮，同一条磁力），JS 只认 input。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, normalize, RawResult};
use bt_core::TorrentResult;

use crate::{MagnetOutcome, SearchOutcome};

/// 镜像列表，与 JS 的 `DOMAINS` 一致。
pub const DOMAINS: &[&str] = &["https://9mag.net"];

/// 结果行（**不含表头** —— 这一站没有表头行）。
const ROW_SELECTOR: &str = "table.file-list > tbody > tr";

/// 名称 + 详情链接。
const NAME_LINK: &str = "td:nth-child(1) > a";

/// 详情页里的磁力输入框（磁力在 value 属性里）。
const MAGNET_INPUT: &str = "input#input-magnet";

/// 用默认镜像搜索。`page` 与 JS 一样**收下但不用**（URL 里没有页码参数）。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。
///
/// ⚠️ JS 传的 name 是 **`'ZeroMagnet'`** —— 错误串必须逐字一致。
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

    SearchOutcome::err(format!("ZeroMagnet unreachable ({})", errs.join("; ")))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    // JS: `${base}/search?q=${encodeURIComponent(query)}` —— 没有页码参数
    let url = format!("{base}/search?q={}", encode_uri_component(query));

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::ok(Vec::new());
    };

    let dom = Dom::parse(&html);
    // ⚠️ 这一站的错误串是裸 `no_results`（与 oxtorrent 同款，不带 _parsed）
    if dom.select(ROW_SELECTOR).is_empty() {
        return SearchOutcome::err("no_results");
    }

    let results: Vec<TorrentResult> = dom
        .select(ROW_SELECTOR)
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    // 有行但全被过滤掉 → JS 返回 `error: null`（与 oxtorrent / therarbg 同款怪癖）
    SearchOutcome::ok(results)
}

/// 解析一行。只取名称与详情链接 —— meta 列照抄 JS 丢掉（见文件头）。
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
        provider: "zeromagnet".to_string(),
        name: Some(name),
        // ⚠️ 硬编码 `'Porn'` —— 引擎本身就是 Porn 类，JS 原样写死，照抄
        category: Some("Porn".to_string()),
        // 列表页没有磁力 → normalize 自动把 needs_magnet 置 true
        magnet: None,
        detail_url: Some(detail_url),
        ..Default::default()
    }))
}

/// 直接拿一段 HTML 解析（测试与将来的复用入口）。
pub fn parse(base: &str, html: &str) -> Vec<TorrentResult> {
    let dom = Dom::parse(html);
    dom.select(ROW_SELECTOR)
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect()
}

/// 惰性解析磁力。对应 JS 的 `resolveMagnet(detailUrl)`。
///
/// ⚠️ 磁力在 `input#input-magnet` 的 **value 属性**里 —— 不是 href。
/// 错误行为与 `oxtorrent` 一致：HTTP 错误**原样透出**（不同于 megapeer 的统一吞）、
/// 空正文 `no_html`、无磁力 `no_magnet_on_page`。成功时**不带 `info_hash`**（照抄 JS）。
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
        .select(MAGNET_INPUT)
        .first()
        .and_then(|el| attr(*el, "value"))
        .filter(|v| !v.is_empty())
    {
        Some(magnet) => MagnetOutcome {
            magnet: Some(magnet),
            // ⚠️ JS 的 resolveMagnet 只返回 { magnet, error } —— 不抠 infoHash（与 oxtorrent 同）
            info_hash: None,
            error: None,
        },
        None => MagnetOutcome::err("no_magnet_on_page"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://9mag.net";

    /// 造一行：名称被 `<mark>` 拆成两段（后半段通过参数传入）。
    fn row(name_suffix: &str, href: &str) -> String {
        format!(
            "<tr><td class=\"result-title\"><a href=\"{href}\"><mark>u</mark>{name_suffix}</a></td>\
             <td class=\"result-meta\"><div>6.2GB</div><div class=\"result-date\">2026-08-07</div></td></tr>"
        )
    }

    fn page(rows: &str) -> String {
        format!(
            "<html><body><table class=\"table file-list\"><tbody>{rows}</tbody></table></body></html>"
        )
    }

    fn parse_page(rows: &str) -> Vec<TorrentResult> {
        parse(BASE, &page(rows))
    }

    #[test]
    fn parses_a_row_end_to_end() {
        let out = parse_page(&row("buntu-24.04.4-desktop-amd64.iso", "/!lFvU"));

        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.provider, "zeromagnet");
        // ⚠️ 名称被 <mark> 拆成两段 —— 必须是拼接后的完整文本
        assert_eq!(r.name, "ubuntu-24.04.4-desktop-amd64.iso");
        assert_eq!(r.id, "zeromagnet:ubuntu-24.04.4-desktop-amd64.iso");
        assert_eq!(r.detail_url.as_deref(), Some("https://9mag.net/!lFvU"));
        assert_eq!(r.category.as_deref(), Some("Porn"));
        assert!(r.needs_magnet, "列表页没有磁力");

        // ⚠️ td2 的 meta 里明明有 6.2GB / 2026-08-07 —— JS 不要，照抄
        assert_eq!(r.size, None);
        assert_eq!(r.size_text, "—");
        assert_eq!(r.date, None);
        assert_eq!(r.date_text, "—");
        assert_eq!(r.seeders, None);
        assert_eq!(r.leechers, None);
        assert_eq!(r.magnet, None);
        assert_eq!(r.info_hash, None);
    }

    #[test]
    fn rows_without_name_or_href_are_skipped() {
        let rows = format!(
            "{}{}{}",
            // 名字是空的（mark 之后没有文本）
            "<tr><td class=\"result-title\"><a href=\"/!aa\"><mark></mark></a></td>\
             <td class=\"result-meta\"></td></tr>",
            // 名字存在但 href 是空的
            "<tr><td class=\"result-title\"><a href=\"\">n</a></td>\
             <td class=\"result-meta\"></td></tr>",
            row("ok-1.iso", "/!ok")
        );
        let out = parse_page(&rows);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "ok-1.iso");
    }

    #[test]
    fn absolute_detail_urls_are_kept_verbatim() {
        let out = parse_page(&row("x.iso", "https://mirror.example/!ab"));
        assert_eq!(
            out[0].detail_url.as_deref(),
            Some("https://mirror.example/!ab")
        );
    }

    /// 没有表头行 —— 3 行就是 3 条。
    #[test]
    fn every_row_is_a_result_because_there_is_no_header_row() {
        let rows = format!(
            "{}{}{}",
            row("a.iso", "/!a"),
            row("b.iso", "/!b"),
            row("c.iso", "/!c")
        );
        assert_eq!(parse_page(&rows).len(), 3);
    }
}
