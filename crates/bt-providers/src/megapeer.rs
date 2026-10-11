//! MegaPeer —— 俄站 HTML 抓取（B 组第 10 个）。
//!
//! 从 `src/providers/megapeer.js`（87 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表是 `div#index > table > tbody > tr.table_fon`（fixture 里 50 行，
//! **没有表头行**）。名称在 `td:nth-child(2)` 里第 2 个 `<a>`，磁力不在列表页 ——
//! 要等用户点击时走 `resolve_magnet`（详情页 `a[href^="magnet:?xt="]`）。
//!
//! ## ⚠️ 两种行形状 —— 行字段映射**与 JS 有意不同**（divergence）
//!
//! 实测 50 行里 **43 行 4-td / 7 行 5-td**（多一格评论数，恒在 td3）：
//!
//! ```text
//! 4-td: [日期] [名称(colspan=2)]     [大小]                       [peers]
//! 5-td: [日期] [名称]               [评论数] [大小]               [peers]
//! ```
//!
//! JS 按固定列索引取（`td:nth-child(3)` 当 size、`td:nth-child(4)` 当 peers），
//! 于是 **5-td 行的 size 变成评论数、peers 落空**。本移植改成
//! 「**倒数第 2 格 = size、最后一格 = peers**」—— 两种行都取对
//! （rutor 处理「td 数不固定」是同款问题的另一个解法，口径一致）。
//! 名称格两种行都在 td2，选择器照抄 JS。
//!
//! ## ⚠️ 编码：站点是 windows-1251 —— 与 JS 版**有意不同**（divergence）
//!
//! 实测响应字节是 windows-1251（meta 也声明了），而 JS 的 axios 对文本响应
//! 一律 `buffer.toString('utf8')`（1.7.x 源码写死，不读 charset）→
//! **标题里所有西里尔字母变 U+FFFD 乱码**。Rust 的 reqwest `text()` 会读
//! Content-Type 的 charset 并按 encoding_rs 解码 → 俄语正确。
//!
//! 这是照抄不划算的一类差异：照抄乱码 = 照抄一个数据级 bug，而且要在 Rust 里
//! 刻意写 lossy 才能复现。本移植取「正确解码」，测试里用 `divergence_*` 钉住。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{
    encode_uri_component, extract_info_hash, normalize, ru_date, NumOrText, RawResult,
};
use bt_core::TorrentResult;

use crate::{MagnetOutcome, SearchOutcome};

/// 镜像列表，与 JS 的 `DOMAINS` 一致。
pub const DOMAINS: &[&str] = &["https://megapeer.vip"];

/// 结果行（这一站没有表头行，50 行就是 50 条）。
const ROW_SELECTOR: &str = "div#index > table > tbody > tr.table_fon";

/// 名称 + 详情链接（两种行形状都在 td2，照抄 JS）。
const NAME_LINK: &str = "td:nth-child(2) > a:nth-child(2)";

/// 大小：**倒数第 2 格**（4-td 行是 td3、5-td 行是 td4；JS 的 `td:nth-child(3)` 会错）。
const SIZE_CELL: &str = "td:nth-last-child(2)";

/// seeders：最后一格里的第 1 个 `<font>`（JS 是 `td:nth-child(4) > font:nth-child(2)`）。
const SEED_FONT: &str = "td:last-child > font:nth-child(2)";

/// leechers：最后一格里的第 2 个 `<font>`。
const LEECH_FONT: &str = "td:last-child > font:nth-child(4)";

/// 日期（原始俄语文本，交给 `ru_date` 转月份名）。
const DATE_CELL: &str = "td:nth-child(1)";

/// 详情页里的磁力链。
const MAGNET_LINK: &str = "a[href^=\"magnet:?xt=\"]";

/// 用默认镜像搜索。`page` 与 JS 一样**收下但不用** —— `searchOne` 收了 `page`
/// 却没往 URL 里放（这个站没有页码参数）。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。
///
/// ⚠️ JS 传的 name 是 **`'megapeer'`（全小写）** —— 错误串必须逐字一致。
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

    SearchOutcome::err(format!("megapeer unreachable ({})", errs.join("; ")))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    // JS: `${base}/browse.php?search=${encodeURIComponent(query)}&age=&cat=0&stype=0&sort=0&ascdesc=0`
    let url = format!(
        "{base}/browse.php?search={}&age=&cat=0&stype=0&sort=0&ascdesc=0",
        encode_uri_component(query)
    );

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    // JS: `if (error || !html) return { results: [], error }` —— 空正文无错误时
    // error 也仍是 null，等价于「空结果、无错误」。
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::ok(Vec::new());
    };

    let dom = Dom::parse(&html);
    // JS: `if (items.length === 0) return { …, error: 'no_results_parsed' }`
    if dom.select(ROW_SELECTOR).is_empty() {
        return SearchOutcome::err("no_results_parsed");
    }

    let results: Vec<TorrentResult> = dom
        .select(ROW_SELECTOR)
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    // 有行但全被过滤掉 → JS 返回 `error: null`（与 oxtorrent / therarbg 同款怪癖）
    SearchOutcome::ok(results)
}

/// 解析一行。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let link = dom.find(row, NAME_LINK).into_iter().next()?;

    // JS 顺序：先看 href（`if (!nameHref) return null`），再要名字非空
    let href = attr(link, "href").filter(|h| !h.is_empty())?;
    let name = text_trim(link);
    if name.is_empty() {
        return None;
    }

    // JS: `nameHref.startsWith('http') ? nameHref : `${base}${nameHref}``（直接拼接，无斜杠处理）
    let detail_url = if href.starts_with("http") {
        href
    } else {
        format!("{base}{href}")
    };

    let cell_text = |sel: &str| {
        dom.find(row, sel)
            .first()
            .map(|e| text_trim(*e))
            .unwrap_or_default()
    };

    let size = cell_text(SIZE_CELL);
    let seeders = cell_text(SEED_FONT);
    let leechers = cell_text(LEECH_FONT);
    // JS: `$li.find('td:nth-child(1)').first().text().trim()` → ruDate
    let date_raw = cell_text(DATE_CELL);

    Some(normalize(&RawResult {
        provider: "megapeer".to_string(),
        name: Some(name),
        // JS 无条件传字符串（取不到就是空串），保持一致：空串经 normalize 会落成 None
        size: Some(NumOrText::Text(size)),
        seeders: Some(NumOrText::Text(seeders)),
        leechers: Some(NumOrText::Text(leechers)),
        date: Some(NumOrText::Text(ru_date(&date_raw))),
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
/// ⚠️ 与 oxtorrent / torrent9 都不同：**所有失败统一吞成 `no_magnet`**
/// （HTTP 错误、空正文、页面里没有链接 —— JS 的 `fetchDetails` 前两关返回 null、
/// 第三关也 null，最后都落进同一个 `no_magnet` 分支）。
/// 成功时**带 `info_hash`**（从磁力里抠，与 audiobookbay 一样）。
pub async fn resolve_magnet(http: &HttpClient, detail_url: &str) -> MagnetOutcome {
    let resp = http.get_text(detail_url, None).await;
    if resp.error.is_some() {
        return MagnetOutcome::err("no_magnet");
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return MagnetOutcome::err("no_magnet");
    };

    let dom = Dom::parse(&html);
    match dom
        .select(MAGNET_LINK)
        .first()
        .and_then(|a| attr(*a, "href"))
        .filter(|h| !h.is_empty())
    {
        Some(href) => MagnetOutcome {
            info_hash: extract_info_hash(Some(&href)),
            magnet: Some(href),
            error: None,
        },
        None => MagnetOutcome::err("no_magnet"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://megapeer.vip";

    /// 造一行。`comments` 为 `Some(数字)` 时是 5-td 形状（评论格在 td3）。
    fn row(
        date: &str,
        name: &str,
        href: &str,
        size: &str,
        seeds: &str,
        leeches: &str,
        comments: Option<&str>,
    ) -> String {
        let (name_colspan, comment_cell) = match comments {
            Some(n) => (
                "",
                format!("<td align=\"right\" width=\"1px\">{n} <img src=\"/pic/com.gif\" alt=\"C\"></td>"),
            ),
            None => (" colspan=\"2\"", String::new()),
        };
        format!(
            "<tr class=\"table_fon\">\
               <td>{date}</td>\
               <td{name_colspan}><a href=\"/download/1\"><img src=\"/pic/d_small.gif\" alt=\"D\"></a>\
                 <a href=\"{href}\" class=\"url\">{name}</a></td>\
               {comment_cell}\
               <td align=\"right\">{size}</td>\
               <td align=\"center\"><img src=\"/pic/seed.gif\" alt=\"S\"> <font color=\"#008000\">{seeds}</font>\
                 <img src=\"/pic/leech.gif\" alt=\"L\"> <font color=\"#8b0000\">{leeches}</font></td>\
             </tr>"
        )
    }

    fn page(rows: &str) -> String {
        format!("<div id=\"index\"><table><tbody>{rows}</tbody></table></div>")
    }

    fn parse_page(rows: &str) -> Vec<TorrentResult> {
        parse(BASE, &page(rows))
    }

    #[test]
    fn parses_a_4td_row_end_to_end() {
        let out = parse_page(&row(
            "11 Окт 26",
            "Movie 1080p",
            "/torrent/214934/movie-1080p",
            "9.62 GB",
            "1",
            "17",
            None,
        ));

        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.provider, "megapeer");
        assert_eq!(r.name, "Movie 1080p");
        assert_eq!(
            r.detail_url.as_deref(),
            Some("https://megapeer.vip/torrent/214934/movie-1080p")
        );
        assert_eq!(r.size_text, "9.6 GB");
        assert_eq!(r.seeders, Some(1));
        assert_eq!(r.leechers, Some(17));
        // ⚠️ parseDate 用**本地时区零点**、formatDate 按 **UTC** 输出 →
        // CI（UTC）上这里应是 2026-10-11；东八区本地跑会是 2026-10-10。
        assert!(r.date.is_some());
        assert!(
            r.date_text == "2026-10-11" || r.date_text == "2026-10-10",
            "date_text 意外: {}",
            r.date_text
        );
        assert!(r.needs_magnet, "列表页没有磁力");
        assert_eq!(r.magnet, None);
        assert_eq!(r.info_hash, None);
        assert_eq!(r.category, None);
    }

    /// ⚠️ divergence：5-td 行的 size 必须取到**大小**（JS 会拿到评论数 "1"）。
    #[test]
    fn divergence_5td_row_takes_real_size_not_the_comment_count() {
        let out = parse_page(&row(
            "11 Окт 26",
            "Show 1080p",
            "/torrent/211947/show-1080p",
            "34.62 GB",
            "26",
            "101",
            Some("1"),
        ));

        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.size_text, "34.6 GB", "不能是评论数 1");
        // 34.62 GB 的字节数 —— 期望值由 Node 端 normalize.js 生成，不是手算的
        assert_eq!(r.size, Some(37_172_941_947));
        // JS 的 peers 在 5-td 行上会整体落空，本移植照样取得到
        assert_eq!(r.seeders, Some(26));
        assert_eq!(r.leechers, Some(101));
    }

    /// 两种形状混在一页里，逐行各自取对。
    #[test]
    fn mixed_4td_and_5td_rows_each_take_their_own_cells() {
        let rows = format!(
            "{}{}",
            row("11 Окт 26", "a", "/torrent/1/a", "9.62 GB", "1", "17", None),
            row("11 Окт 26", "b", "/torrent/2/b", "34.62 GB", "26", "101", Some("3")),
        );
        let out = parse_page(&rows);

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].size_text, "9.6 GB");
        assert_eq!(out[0].seeders, Some(1));
        assert_eq!(out[1].size_text, "34.6 GB");
        assert_eq!(out[1].seeders, Some(26));
    }

    #[test]
    fn rows_without_name_or_href_are_skipped() {
        let rows = format!(
            "{}{}{}",
            row("11 Окт 26", "", "/torrent/1/x", "1 GB", "1", "0", None),
            "<tr class=\"table_fon\"><td>11 Окт 26</td><td><a>no href</a></td><td>1 GB</td><td>x</td></tr>",
            row("11 Окт 26", "ok", "/torrent/2/x", "1 GB", "1", "0", None)
        );
        let out = parse_page(&rows);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "ok");
    }

    #[test]
    fn absolute_detail_urls_are_kept_verbatim() {
        let out = parse_page(&row(
            "11 Окт 26",
            "n",
            "https://mirror.example/torrent/1/x",
            "1 GB",
            "1",
            "0",
            None,
        ));
        assert_eq!(
            out[0].detail_url.as_deref(),
            Some("https://mirror.example/torrent/1/x")
        );
    }

    /// 没有表头行 —— 3 行就是 3 条。
    #[test]
    fn every_row_is_a_result_because_there_is_no_header_row() {
        let rows = format!(
            "{}{}{}",
            row("11 Окт 26", "a", "/t/1", "1 GB", "1", "0", None),
            row("11 Окт 26", "b", "/t/2", "2 GB", "2", "1", None),
            row("11 Окт 26", "c", "/t/3", "3 GB", "3", "2", Some("9"))
        );
        assert_eq!(parse_page(&rows).len(), 3);
    }

    #[test]
    fn date_comes_from_the_raw_first_cell_via_ru_date() {
        assert_eq!(ru_date("11 Окт 26"), "11 Oct 26");
        assert_eq!(ru_date("10 Ноя 14"), "10 Nov 14");
        let out = parse_page(&row("30 Июн 26", "n", "/t/1", "1 GB", "1", "0", None));
        assert!(out[0].date.is_some());
    }

    /// peers 缺失（没有两个 font）时不硬凑。
    #[test]
    fn rows_without_fonts_have_no_peers() {
        let rows = "<tr class=\"table_fon\"><td>11 Окт 26</td>\
                    <td><a href=\"/t/1\">n</a></td>\
                    <td align=\"right\">1 GB</td>\
                    <td align=\"center\">nothing</td></tr>";
        let out = parse_page(rows);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].seeders, None);
        assert_eq!(out[0].leechers, None);
    }

    /// ⚠️ divergence：Rust 侧文本是**正确解码的 windows-1251**，
    /// 而 JS（axios utf8 硬解码）会把这里所有西里尔字母变 U+FFFD。
    #[test]
    fn divergence_cyrillic_names_stay_readable() {
        let out = parse_page(&row(
            "11 Окт 26",
            "Тёмная материя / Dark Matter (2026) WEB-DL 1080p",
            "/t/1",
            "1 GB",
            "1",
            "0",
            None,
        ));
        assert!(out[0].name.contains("Тёмная материя"));
        assert!(!out[0].name.contains('\u{FFFD}'), "不该有替换字符");
    }
}
