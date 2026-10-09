//! Rutor —— 俄站 HTML 抓取（B 组第 3 个）。
//!
//! 从 `src/providers/rutor.js`（99 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表是 `div#index > table > tbody > tr`，**共 101 行、第一行是表头**
//! （`Добавлен / Название / Размер / Пиры`）→ 100 条数据行。
//! 表头行没有 `a:nth-child(3)`，所以按片名选择器取自然就跳过了。
//!
//! 数据行的第 2 个 `<td>` 里有三个 `<a>`：
//!
//! ```text
//! a:nth-child(1)  class="downgif"  下载图标
//! a:nth-child(2)  href="magnet:…"  ← 磁力链，本站列表页直接带
//! a:nth-child(3)  href="/torrent/…" ← 片名 + 详情页
//! ```
//!
//! ⚠️ **数据行的 `td` 数不固定：5 个（带评论数）或 4 个** —— 实测 28 / 72。
//! 所以 size **必须按内容找**（「含数字」且「含容量单位」），不能按列序号 ——
//! 这正是 JS 的写法，原因就写在那里。
//!
//! ## 编码
//!
//! JS 里那个 `getWin1251()` **名字是误导的** —— 它体内只是 `getText(url)`，
//! 而站点实际就是 UTF-8（实测响应头 `charset=UTF-8`）。本移植同样按 UTF-8 处理。
//!
//! ## ⚠️ 上游已知问题：`rutor.ru` 是死域名
//!
//! 2026-10-09 实测 `https://rutor.ru/` 是**「域名出售」停放页**
//! （title = «Домен продается. Купить в магистре…»）。DOMAINS 里保留它（对齐 JS），
//! 但镜像回退实际只会用到前两个。

use bt_core::dom::{attr, text, text_trim, Dom};
use bt_core::http::HttpClient;
use bt_core::normalize::{
    coerce_page, encode_uri_component, extract_info_hash, normalize, ru_date, NumOrText, RawResult,
};
use bt_core::TorrentResult;

use crate::SearchOutcome;

/// 镜像列表，与 JS 的 `DOMAINS` 一致（`rutor.ru` 已死，见文件头）。
pub const DOMAINS: &[&str] = &["https://rutor.info", "https://rutor.is", "https://rutor.ru"];

/// 结果行选择器（与 JS 的 `div#index > table > tbody > tr` 逐字一致）。
const ROW_SELECTOR: &str = "div#index > table > tbody > tr";

/// 片名链接：第 2 个 td 里的第 3 个 `<a>`。
const NAME_LINK: &str = "td:nth-child(2) > a:nth-child(3)";

/// 磁力链接：第 2 个 td 里的第 2 个 `<a>`（href 就是 `magnet:?…`）。
const MAGNET_LINK: &str = "td:nth-child(2) > a:nth-child(2)";

/// 容量单位（含西里尔）。**故意不匹配裸 `B` / `Б`** ——
/// 片名里像 "Black Box (2026)" 既有数字又有 `B`，会被误判成 size。
const SIZE_UNITS: &[&str] = &["GB", "MB", "KB", "TB", "ГБ", "МБ", "КБ", "ТБ"];

/// 大小写不敏感的子串查找，等价于 JS 的 `/…/i` 测试。
fn contains_ci(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let hchars: Vec<char> = hay.chars().collect();
    let nchars: Vec<char> = needle.chars().collect();
    if nchars.len() > hchars.len() {
        return false;
    }
    (0..=hchars.len() - nchars.len()).any(|i| {
        nchars
            .iter()
            .enumerate()
            .all(|(k, nc)| hchars[i + k].to_lowercase().eq(nc.to_lowercase()))
    })
}

/// 大小写不敏感的子串替换，等价于 JS 的 `.replace(/…/gi, …)`。
///
/// 手写而不用正则：西里尔字母的大小写折叠按 `char::to_lowercase` 走（可能一对多），
/// 逐字符比较比按字节切片安全。
fn replace_ci(hay: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return hay.to_string();
    }
    let hchars: Vec<char> = hay.chars().collect();
    let nchars: Vec<char> = needle.chars().collect();

    let mut out = String::with_capacity(hay.len());
    let mut i = 0usize;
    while i < hchars.len() {
        let hit = hchars.len() - i >= nchars.len()
            && nchars
                .iter()
                .enumerate()
                .all(|(k, nc)| hchars[i + k].to_lowercase().eq(nc.to_lowercase()));
        if hit {
            out.push_str(with);
            i += nchars.len();
        } else {
            out.push(hchars[i]);
            i += 1;
        }
    }
    out
}

/// 文本里有没有容量单位（大小写不敏感）。
fn has_size_unit(t: &str) -> bool {
    SIZE_UNITS.iter().any(|u| contains_ci(t, u))
}

/// 复刻 JS 的 size 规整：西里尔单位换拉丁、**第一个**逗号换点。
fn normalize_size(text: &str) -> String {
    let s = replace_ci(text, "ГБ", "GB");
    let s = replace_ci(&s, "МБ", "MB");
    let s = replace_ci(&s, "КБ", "KB");
    // JS 的 `.replace(',', '.')` 只换第一个
    s.replacen(',', ".", 1)
}

/// 把页面里的相对链接拼到 base 上。
fn join_base(base: &str, href: &str) -> String {
    if href.starts_with("http") {
        return href.to_string();
    }
    match (base.ends_with('/'), href.starts_with('/')) {
        (true, true) => format!("{base}{}", &href[1..]),
        (false, false) => format!("{base}/{href}"),
        _ => format!("{base}{href}"),
    }
}

/// 用默认镜像搜索。`page` 与 JS 一致是 **1 起始**，这里减 1 后交给站点。
pub async fn search(http: &HttpClient, query: &str, page: u32) -> SearchOutcome {
    let p = coerce_page(Some(&NumOrText::Num(page as i64))).saturating_sub(1);
    search_with(http, DOMAINS, query, p).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`（名字沿用 JS 的 `'Rutor'`，大小写不能改，
/// 否则错误串对不上）。
pub async fn search_with(
    http: &HttpClient,
    bases: &[&str],
    query: &str,
    page0: u32,
) -> SearchOutcome {
    let mut errs: Vec<String> = Vec::new();

    for base in bases {
        let outcome = search_at(http, base, query, page0).await;
        if !outcome.results.is_empty() {
            return SearchOutcome::ok(outcome.results);
        }
        if let Some(e) = outcome.error {
            errs.push(e);
        }
    }

    SearchOutcome::err(format!("Rutor unreachable ({})", errs.join("; ")))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
/// `page0` 是**站点实际用的 0 起始页码**。
pub async fn search_at(http: &HttpClient, base: &str, query: &str, page0: u32) -> SearchOutcome {
    // JS: `${base}/search/${page}/0/010/2/${encodeURIComponent(query)}`
    // 中间三段是分类/匹配方式/排序，原样保留。
    let url = format!(
        "{base}/search/{page0}/0/010/2/{}",
        encode_uri_component(query)
    );

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::ok(Vec::new());
    };

    let dom = Dom::parse(&html);
    // JS: `if (rows.length <= 1) return {…, error: 'no_results_parsed'}` —— 1 行说明只有表头
    if dom.select(ROW_SELECTOR).len() <= 1 {
        return SearchOutcome::err("no_results_parsed");
    }

    SearchOutcome::ok(parse_dom(base, &dom))
}

/// 解析段（`search_at` 与测试共用，逻辑只有这一份）。
fn parse_dom(base: &str, dom: &Dom) -> Vec<TorrentResult> {
    let rows = dom.select(ROW_SELECTOR);
    let mut results = Vec::new();

    // 跳过第 1 行（表头）
    for row in rows.iter().skip(1) {
        let name = dom
            .find(*row, NAME_LINK)
            .first()
            .map(|e| text_trim(*e))
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }

        let magnet = dom
            .find(*row, MAGNET_LINK)
            .first()
            .and_then(|a| attr(*a, "href"))
            .filter(|h| !h.is_empty());
        let Some(magnet) = magnet else {
            continue;
        };

        let detail_href = dom
            .find(*row, NAME_LINK)
            .first()
            .and_then(|a| attr(*a, "href"))
            .filter(|h| !h.is_empty());
        let detail_url = detail_href.map(|h| join_base(base, &h));

        let tds = dom.find(*row, "td");
        let mut size_text = String::new();
        let mut seeders_text = String::new();
        let mut leechers_text = String::new();

        for td in &tds {
            let t = text_trim(*td);
            // 「含数字」且「含容量单位」才算 size —— 评论数那格只有数字，会被跳过
            if size_text.is_empty() && t.chars().any(|c| c.is_ascii_digit()) && has_size_unit(&t) {
                size_text = t;
            }
            // 带两个 span 的 td 是 peers：第一个 up（seeders），最后一个是 down（leechers）
            let spans = dom.find(*td, "span");
            if spans.len() >= 2 {
                seeders_text = text_trim(spans[0]);
                leechers_text = text_trim(*spans.last().unwrap());
            }
        }

        // ⚠️ 日期取的是 td 的**原始文本**（JS 在这个位置没有 trim）
        let date_raw = tds.first().map(|e| text(*e)).unwrap_or_default();

        results.push(normalize(&RawResult {
            provider: "rutor".to_string(),
            id: None,
            name: Some(name),
            info_hash: extract_info_hash(Some(&magnet)),
            magnet: Some(magnet),
            // JS 无条件传字符串（可能是空串），保持一致
            size: Some(NumOrText::Text(normalize_size(&size_text))),
            seeders: Some(NumOrText::Text(seeders_text)),
            leechers: Some(NumOrText::Text(leechers_text)),
            date: Some(NumOrText::Text(ru_date(&date_raw))),
            // rutor 的搜索列表没有逐条分类，交给前端推断
            category: None,
            detail_url,
            files: None,
        }));
    }

    results
}

/// 直接拿一段 HTML 解析（测试与将来的复用入口）。
pub fn parse(base: &str, html: &str) -> Vec<TorrentResult> {
    parse_dom(base, &Dom::parse(html))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://rutor.info";
    const H1: &str = "5c1d6707dade6bb1150ea1c7020a67cfa2b908f5";
    const MAGNET1: &str =
        "magnet:?xt=urn:btih:5c1d6707dade6bb1150ea1c7020a67cfa2b908f5&dn=rutor.info&tr=udp://opentor.net:6969";

    /// 造一个数据行。`with_comments=true` 时多一格评论数（5 个 td），否则 4 个。
    fn row(
        date: &str,
        name: &str,
        href: &str,
        magnet: &str,
        size: &str,
        seeds: &str,
        leeches: &str,
        with_comments: bool,
    ) -> String {
        let comments = if with_comments {
            "<td align=\"right\">14<img src=\"/i/com.gif\" alt=\"C\" /></td>"
        } else {
            ""
        };
        format!(
            r#"<tr class="gai">
                 <td>{date}</td>
                 <td>
                   <a class="downgif" href="//d.rutor.info/download/1"><img src="/i/d.gif" alt="D" /></a>
                   <a href="{magnet}"><img src="/i/m.gif" alt="M" /></a>
                   <a href="{href}">{name}</a>
                 </td>
                 {comments}
                 <td align="right">{size}</td>
                 <td align="center"><span class="green"><img src="/t/up.gif" alt="S" />&nbsp;{seeds}</span>&nbsp;<img src="/t/down.gif" alt="L" /><span class="red">&nbsp;{leeches}</span></td>
               </tr>"#
        )
    }

    fn page(rows: &str) -> String {
        format!(
            r#"<div id="index"><table><tbody>
                 <tr class="backgr"><td>Добавлен</td><td colspan="2">Название</td><td>Размер</td><td>Пиры</td></tr>
                 {rows}
               </tbody></table></div>"#
        )
    }

    fn one(date: &str, name: &str, size: &str, seeds: &str, leeches: &str, wc: bool) -> String {
        page(&row(
            date,
            name,
            "/torrent/1/x",
            MAGNET1,
            size,
            seeds,
            leeches,
            wc,
        ))
    }

    #[test]
    fn parses_a_row_end_to_end() {
        let html = one(
            "30 Июн 26",
            "Rufus 4.15 (2026) PC",
            "10.88 MB",
            "44",
            "2",
            true,
        );
        let out = parse(BASE, &html);

        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.provider, "rutor");
        assert_eq!(r.name, "Rufus 4.15 (2026) PC");
        assert_eq!(r.info_hash.as_deref(), Some(H1));
        assert_eq!(r.magnet.as_deref(), Some(MAGNET1));
        assert_eq!(r.size_text, "10.9 MB");
        assert_eq!(r.seeders, Some(44));
        assert_eq!(r.leechers, Some(2));
        assert_eq!(r.category, None, "搜索列表没有逐条分类");
        assert_eq!(
            r.detail_url.as_deref(),
            Some("https://rutor.info/torrent/1/x")
        );
        assert!(!r.needs_magnet, "列表页自带磁力，不需要惰性解析");
    }

    #[test]
    fn header_row_alone_yields_nothing() {
        let html = page("");
        assert!(parse(BASE, &html).is_empty());
    }

    #[test]
    fn rows_missing_name_or_magnet_are_skipped() {
        // 没有 magnet 的 <a>（href 是空的）
        let no_magnet = page(&row(
            "30 Июн 26",
            "n",
            "/torrent/1/x",
            "",
            "1 MB",
            "1",
            "0",
            false,
        ));
        assert!(parse(BASE, &no_magnet).is_empty(), "没有磁力就整条丢掉");

        // 有磁力但没有片名链接文本
        let no_name = page(&row(
            "30 Июн 26",
            "",
            "/torrent/1/x",
            MAGNET1,
            "1 MB",
            "1",
            "0",
            false,
        ));
        assert!(parse(BASE, &no_name).is_empty(), "没有片名就整条丢掉");
    }

    #[test]
    fn comments_cell_is_not_mistaken_for_size() {
        // 5 个 td：评论数那格是 "14"（有数字、没有单位）→ 不能当成 size
        let html = one("30 Июн 26", "n", "4.56 GB", "9", "0", true);
        assert_eq!(parse(BASE, &html)[0].size_text, "4.6 GB");

        // 4 个 td（没有评论数那格）也要能正常工作
        let html = one("30 Июн 26", "n", "1.63 GB", "9", "0", false);
        assert_eq!(parse(BASE, &html)[0].size_text, "1.6 GB");
    }

    /// 片名里同时出现数字和 `B`（"Black Box"）**不能**被当成 size —— 这就是
    /// JS 注释里说的「别匹配裸 B」的理由。
    #[test]
    fn a_name_containing_a_digit_and_letter_b_is_not_a_size() {
        let html = one("30 Июн 26", "Black Box 2 (2026)", "1 MB", "1", "0", false);
        assert_eq!(parse(BASE, &html)[0].size_text, "1 MB");
    }

    #[test]
    fn cyrillic_units_and_comma_decimals_are_normalized() {
        let html = one("30 Июн 26", "n", "1,44 ГБ", "1", "0", false);
        // 逗号变点、ГБ 变 GB，然后 normalize 才算得出字节数
        let r = &parse(BASE, &html)[0];
        assert_eq!(r.size_text, "1.4 GB");
        assert!(r.size.is_some());
    }

    #[test]
    fn seeders_come_from_the_first_span_and_leechers_from_the_last() {
        let html = one("30 Июн 26", "n", "1 MB", "123", "45", false);
        let r = &parse(BASE, &html)[0];
        assert_eq!(r.seeders, Some(123));
        assert_eq!(r.leechers, Some(45));
    }

    #[test]
    fn rows_without_two_spans_have_no_seeders() {
        // 注意三个 <a> 的顺序不能乱：a2 = 磁力、a3 = 片名
        let html = format!(
            r#"<div id="index"><table><tbody>
                 <tr class="backgr"><td>Добавлен</td></tr>
                 <tr><td>30 Июн 26</td>
                     <td><a class="downgif" href="/download/1"><img /></a>
                         <a href="{MAGNET1}"><img /></a>
                         <a href="/torrent/1/x">n</a></td>
                     <td align="right">1 MB</td>
                     <td align="center">single span</td></tr>
               </tbody></table></div>"#
        );
        let r = &parse(BASE, &html)[0];
        assert_eq!(r.seeders, None, "没有两个 span 就没有 peers");
        assert_eq!(r.leechers, None);
    }

    #[test]
    fn date_comes_from_the_raw_first_cell() {
        // ruDate 会把俄语月份换成英文缩写，再由 normalize 解析
        let html = one("30 Июн 26", "n", "1 MB", "1", "0", false);
        assert_eq!(ru_date("30 Июн 26"), "30 Jun 26");
        assert!(parse(BASE, &html)[0].date.is_some());
    }

    #[test]
    fn join_base_handles_both_shapes() {
        assert_eq!(
            join_base("https://rutor.info", "/torrent/1/x"),
            "https://rutor.info/torrent/1/x"
        );
        assert_eq!(
            join_base("https://rutor.info", "https://x/y"),
            "https://x/y"
        );
        assert_eq!(join_base("https://x/", "/y"), "https://x/y");
        assert_eq!(join_base("https://x", "y"), "https://x/y");
    }

    #[test]
    fn replace_ci_is_case_insensitive_including_cyrillic() {
        assert_eq!(replace_ci("1.44 гб", "ГБ", "GB"), "1.44 GB");
        assert_eq!(replace_ci("1.44 ГБ", "ГБ", "GB"), "1.44 GB");
        assert_eq!(replace_ci("aGBb", "gb", "X"), "aXb");
        assert_eq!(replace_ci("no unit", "ГБ", "GB"), "no unit");
    }

    #[test]
    fn normalize_size_replaces_only_the_first_comma() {
        assert_eq!(normalize_size("1,44 ГБ"), "1.44 GB");
        assert_eq!(normalize_size("1,2,3 МБ"), "1.2,3 MB");
    }

    #[test]
    fn has_size_unit_rejects_a_bare_letter() {
        assert!(has_size_unit("10.88 MB"));
        assert!(has_size_unit("1,44 ГБ"));
        assert!(!has_size_unit("Black Box 2"));
        assert!(!has_size_unit("14"));
    }
}
