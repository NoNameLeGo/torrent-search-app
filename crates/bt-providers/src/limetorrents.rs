//! LimeTorrents —— HTML 抓取（B 组第 6 个，第一个带**真分页**的）。
//!
//! 从 `src/providers/limetorrents.js`（95 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表是 `.table2 > tbody > tr`，**共 41 行、第一行是表头**（`<tr><th>`，没有 td）
//! → 40 条数据行。数据行 6 个 td：
//!
//! ```text
//! 1  td.tdleft   <div.tt-name>
//!                   <a a:nth-child(1)  href="http://itorrents.net/torrent/<HASH>.torrent?title=…">（下载图标，无文本）
//!                   <a a:nth-child(2)  href="/<slug>-torrent-<id>.html">名称</a>
//!                </div>
//! 2  td.tdnormal "9 months ago - in Other"   ← 日期与分类挤在同一句里
//! 3  td.tdnormal "8.16 MB"
//! 4  td.tdseed   "35"
//! 5  td.tdleech  "0"
//! 6  td          健康度（空）
//! ```
//!
//! ⚠️ **info hash 不是磁力，是从第 1 个 `<a>` 的 itorrents 链接里抠出来的**，
//! 然后交给 `normalize` 组回磁力（所以本站**不需要**惰性解析，`needs_magnet` 恒 false）。
//! 抠不到 hash 的行**整条丢掉** —— 这就是 40 条与 41 行的差别所在。
//!
//! ⚠️ 页面别处也有 `td:nth-child(2)` / `td.tdseed` 这种搭配（探针 `datecell.texts`
//! 的头三条是旁边速度榜的 `"6572 KB/Sec"`）。**必须按行取**（`$row.find(…)` / `dom.find(row, …)`），
//! 用全局选择器会取到别的表的格子。
//!
//! ## ⚠️ 两个上游 bug（照抄，未修）
//!
//! 1. **分类映射基本失效**：JS 的 `switch` 只认 `TV` / `Movie` / `Music` / `App` /
//!    `E-book` / `Anime` / `Games` 这几个拼写，而站上写的是 `TV shows` / `E-books` /
//!    `Applications`（探针 `datecell.texts` 实证）→ **全部落到 `Other`**。
//! 2. **大部分结果没有日期**：站上的相对日期会被截断成 `1 Year+`，而 `normalize.parseDate`
//!    只认 `N unit ago`（要 `ago`）→ `1 Year+` 解析不出，`dateText` 显示 `—`。
//!    只有 `9 months ago` 这类才认（那条还是**相对今天**算的，测试里得带容差）。
//!
//! ## 分页
//!
//! URL 是 `/search/all/<query>/date/<page>/`，`page` 走 JS 的 `coercePage`（0/空 → 1）。

use bt_core::dom::{attr, text, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{coerce_page, encode_uri_component, normalize, NumOrText, RawResult};
use bt_core::TorrentResult;

use crate::SearchOutcome;

/// 镜像列表，与 JS 的 `DOMAINS` 一致。
/// ⚠️ `limetorrents.lol` 实测连不上（探活表里也是时有时无），保留只为对齐 JS。
pub const DOMAINS: &[&str] = &[
    "https://limetorrents.fun",
    "https://limetorrents.lol",
    "https://limetorrents.pro",
];

/// 结果行选择器（JS: `.table2 > tbody > tr`，含表头行）。
const ROWS: &str = ".table2 > tbody > tr";

/// 名称链接：`.tt-name` 里的**第 2 个** `<a>`。
const NAME_LINK: &str = "td:nth-child(1) > div.tt-name > a:nth-child(2)";

/// 下载图标链接：`.tt-name` 里的第 1 个 `<a>`，href 指向 itorrents。
const FILE_LINK: &str = "td:nth-child(1) > div.tt-name > a:nth-child(1)";

/// `"9 months ago - in Other"` 那一格。
const DATE_CELL: &str = "td:nth-child(2)";
const SIZE_CELL: &str = "td:nth-child(3)";
const SEEDERS_CELL: &str = "td.tdseed";
const LEECHERS_CELL: &str = "td.tdleech";

/// 站点分类 → 项目标准分类。逐字照抄 JS 的 `switch`。
/// ⚠️ 只认这几个精确拼写，站上的 `TV shows` / `E-books` / `Applications` 全会落到 `Other`。
fn category_from_raw(raw: &str) -> String {
    match raw.trim() {
        "TV" => "Series",
        "Movie" => "Movies",
        "Music" => "Music",
        "App" => "Apps",
        "E-book" => "Books",
        "Anime" => "Anime",
        "Games" => "Games",
        _ => "Other",
    }
    .to_string()
}

/// 从 `http://itorrents.net/torrent/<HASH>.torrent?title=…` 里抠出 info hash。
///
/// 对应 JS 的 `fileLink.match(/itorrents\.net\/torrent\/([0-9A-Fa-f]+)\./)` + `.toLowerCase()`。
/// 注意 JS **不校验长度**（抠到几个字符就是几个字符），这里也同样不校验。
fn itorrents_hash(href: &str) -> Option<String> {
    const MARK: &str = "itorrents.net/torrent/";
    let rest = &href[href.find(MARK)? + MARK.len()..];
    let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    // JS 的正则要求 hex 之后紧跟一个 `.`
    if hex.is_empty() || !rest[hex.len()..].starts_with('.') {
        return None;
    }
    Some(hex.to_ascii_lowercase())
}

/// 拆 `"9 months ago - in Other"`。
///
/// 对应 JS：
/// ```js
/// if (dcText.includes('- in')) {
///   const idx = dcText.indexOf('-');            // 第一个 '-'，不是 '- in' 里那个
///   date = dcText.slice(0, idx).trim();
///   category = categoryFromRaw(dcText.slice(idx + 1).trim().replace(/^in /, '').replace(/\.$/, ''));
/// } else { date = dcText; }
/// ```
fn split_date_category(dc_text: &str) -> (Option<NumOrText>, Option<String>) {
    let t = dc_text.trim();

    if !t.contains("- in") {
        // 没有分类 → 整格当日期（`1 Year+` 就是这条路径，最后解析不出）
        return (Some(NumOrText::Text(t.to_string())), None);
    }

    let Some(idx) = t.find('-') else {
        return (Some(NumOrText::Text(t.to_string())), None);
    };

    let date = t[..idx].trim().to_string();
    let after = t[idx + 1..].trim();
    let after = after.strip_prefix("in ").unwrap_or(after);
    let after = after.strip_suffix('.').unwrap_or(after);

    (Some(NumOrText::Text(date)), Some(category_from_raw(after)))
}

/// 用默认镜像搜索。`page` 与 JS 一样先过 `coercePage`（0/空 → 1）。
pub async fn search(http: &HttpClient, query: &str, page: u32) -> SearchOutcome {
    let p = coerce_page(Some(&NumOrText::Num(page as i64)));
    search_with(http, DOMAINS, query, p).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。
///
/// ⚠️ JS 传的 name 是 **`'LimeTorrents'`**，错误串必须逐字一致。
pub async fn search_with(
    http: &HttpClient,
    bases: &[&str],
    query: &str,
    page: u32,
) -> SearchOutcome {
    let mut errs: Vec<String> = Vec::new();

    for base in bases {
        let outcome = search_at(http, base, query, page).await;
        if !outcome.results.is_empty() {
            return SearchOutcome::ok(outcome.results);
        }
        if let Some(e) = outcome.error {
            errs.push(e);
        }
    }

    SearchOutcome::err(format!("LimeTorrents unreachable ({})", errs.join("; ")))
}

/// `/search/<category>/<query>/date/<page>/` —— 中间那段分类固定 `all`。
fn search_url(base: &str, query: &str, page: u32) -> String {
    format!(
        "{base}/search/all/{}/date/{page}/",
        encode_uri_component(query)
    )
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str, page: u32) -> SearchOutcome {
    let url = search_url(base, query, page);

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::ok(Vec::new());
    };

    let dom = Dom::parse(&html);
    // JS: `if (rows.length === 0) return {…, error: 'no_results_parsed'}`
    if dom.select(ROWS).is_empty() {
        return SearchOutcome::err("no_results_parsed");
    }

    let results: Vec<TorrentResult> = dom
        .select(ROWS)
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    // 与 therarbg 一样：JS 这里返回的是 `error: null`，全被过滤掉也不算错误
    SearchOutcome::ok(results)
}

/// 解析一行。表头行（`<th>`，没有 td）会自然地掉进「没有名称」那条分支。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let name_link = dom.find(row, NAME_LINK).into_iter().next()?;
    let name = text_trim(name_link);
    if name.is_empty() {
        return None;
    }

    // 第 1 个 <a> 是 itorrents 下载链接，hash 只在这里
    let file_href = dom
        .find(row, FILE_LINK)
        .into_iter()
        .next()
        .and_then(|a| attr(a, "href"))?;
    let info_hash = itorrents_hash(&file_href)?;

    // 详情页链接可以没有（JS 允许 null）
    let detail_url = attr(name_link, "href")
        .filter(|h| !h.is_empty())
        .map(|href| {
            if href.starts_with("http") {
                href
            } else {
                format!("{base}{href}")
            }
        });

    let (date, category) = split_date_category(&cell_text(dom, row, DATE_CELL));

    Some(normalize(&RawResult {
        provider: "limetorrents".to_string(),
        name: Some(name),
        size: Some(NumOrText::Text(cell_text(dom, row, SIZE_CELL))),
        seeders: Some(NumOrText::Text(cell_text(dom, row, SEEDERS_CELL))),
        leechers: Some(NumOrText::Text(cell_text(dom, row, LEECHERS_CELL))),
        date,
        // 有 info hash → `normalize` 会自己组出磁力，不需要惰性解析
        info_hash: Some(info_hash),
        detail_url,
        category,
        ..Default::default()
    }))
}

/// 取某个格子的文本（JS 在这里不 trim，交给 `normalize` 里的各类解析器 trim）。
fn cell_text(dom: &Dom, row: ElementRef<'_>, cell: &str) -> String {
    dom.find(row, cell)
        .into_iter()
        .next()
        .map(text)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://limetorrents.fun";
    const HASH: &str = "232cd67eb3ffbd7c37bf9ec3ee887417e5ae1ee6";
    const FILE_HREF: &str =
        "http://itorrents.net/torrent/232CD67EB3FFBD7C37BF9EC3EE887417E5AE1EE6.torrent?title=x";

    /// ⚠️ 本机跟外网无关：这里只钉 URL 拼接 + `coercePage` 的接法（JS 在 `search` 里做）。
    /// 真正的请求路径由集成测试的 `request_url_matches_the_js_contract` 盯着。
    #[test]
    fn page_is_coerced_like_the_js_version() {
        let p0 = coerce_page(Some(&NumOrText::Num(0)));
        assert_eq!(p0, 1);
        assert_eq!(
            search_url(BASE, "ubuntu", p0),
            "https://limetorrents.fun/search/all/ubuntu/date/1/"
        );
        assert_eq!(
            search_url(BASE, "ubuntu 22", 7),
            "https://limetorrents.fun/search/all/ubuntu%2022/date/7/"
        );
    }

    #[test]
    fn category_mapping_matches_the_js_switch() {
        assert_eq!(category_from_raw("TV"), "Series");
        assert_eq!(category_from_raw("Movie"), "Movies");
        assert_eq!(category_from_raw("Music"), "Music");
        assert_eq!(category_from_raw("App"), "Apps");
        assert_eq!(category_from_raw("E-book"), "Books");
        assert_eq!(category_from_raw("Anime"), "Anime");
        assert_eq!(category_from_raw("Games"), "Games");
        // ⚠️ 站上真实写的是这几个 —— 全都对不上 switch，落到 Other
        assert_eq!(category_from_raw("TV shows"), "Other");
        assert_eq!(category_from_raw("E-books"), "Other");
        assert_eq!(category_from_raw("Applications"), "Other");
        assert_eq!(category_from_raw(""), "Other");
        assert_eq!(category_from_raw("  TV  "), "Series", "会先 trim");
    }

    #[test]
    fn itorrents_hash_is_lowercased_and_requires_a_dot() {
        assert_eq!(
            itorrents_hash(FILE_HREF).as_deref(),
            Some("232cd67eb3ffbd7c37bf9ec3ee887417e5ae1ee6")
        );
        assert_eq!(
            itorrents_hash("http://itorrents.net/torrent/ABCD.torrent").as_deref(),
            Some("abcd"),
            "JS 不校验长度"
        );
        assert_eq!(
            itorrents_hash("http://itorrents.net/torrent/ABCD-torrent"),
            None,
            "hex 后面必须紧跟 ."
        );
        assert_eq!(itorrents_hash("https://example.com/x.torrent"), None);
        assert_eq!(itorrents_hash(""), None);
    }

    #[test]
    fn date_and_category_are_split_at_the_first_dash() {
        let (d, c) = split_date_category("9 months ago - in Other");
        assert_eq!(d, Some(NumOrText::Text("9 months ago".to_string())));
        assert_eq!(c.as_deref(), Some("Other"));

        // 站上的 `TV shows.` 形态（句尾带点）→ 去掉点之后仍然对不上 switch
        let (d, c) = split_date_category("1 Year+ - in TV shows.");
        assert_eq!(d, Some(NumOrText::Text("1 Year+".to_string())));
        assert_eq!(c.as_deref(), Some("Other"));

        // 没有 '- in' → 整格当日期，没有分类
        let (d, c) = split_date_category("1 Year+");
        assert_eq!(d, Some(NumOrText::Text("1 Year+".to_string())));
        assert_eq!(c, None);

        // 两侧空白会被 trim
        let (d, _) = split_date_category("  2 days ago - in Movies  ");
        assert_eq!(d, Some(NumOrText::Text("2 days ago".to_string())));
    }

    fn row(
        name: &str,
        detail: &str,
        file: &str,
        dc: &str,
        size: &str,
        seeds: &str,
        leech: &str,
    ) -> String {
        // ⚠️ 用 `r##"…"##`：内容里有 `="#`（bgcolor），单层 `r#"` 会在那里提前结束
        format!(
            r##"<tr bgcolor="#F4F4F4"><td class="tdleft"><div class="tt-name">
                 <a href="{file}" rel="nofollow" class="csprite_dl14"></a>
                 <a href="{detail}">{name}</a>
               </div><div class="tt-options"></div></td>
               <td class="tdnormal">{dc}</a></td>
               <td class="tdnormal">{size}</td>
               <td class="tdseed">{seeds}</td>
               <td class="tdleech">{leech}</td>
               <td class="tdright"><div class="hb2"></div></td></tr>"##
        )
    }

    fn header_row() -> &'static str {
        r#"<tr><th class="thleft">Torrent Name</th><th class="thnormal">Added</th>
             <th class="thnormal">Size</th><th class="thnormal">Seed</th>
             <th class="thnormal">Leech</th><th class="thright">Health</th></tr>"#
    }

    fn page(rows: &str) -> String {
        format!("<html><body><table class=\"table2\"><tbody>{rows}</tbody></table></body></html>")
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
        let rows = format!(
            "{}{}",
            header_row(),
            row(
                "Clinton D., Negus C. Ubuntu Linux Bible 11ed 2025",
                "/Clinton-D --Negus-C -Ubuntu-Linux-Bible-11ed-2025-torrent-19390961.html",
                FILE_HREF,
                "9 months ago - in Other",
                "8.16 MB",
                "35",
                "0",
            )
        );
        let out = parse_page(&rows);

        assert_eq!(out.len(), 1, "表头行不算结果");
        let r = &out[0];
        assert_eq!(r.provider, "limetorrents");
        assert_eq!(r.name, "Clinton D., Negus C. Ubuntu Linux Bible 11ed 2025");
        assert_eq!(r.info_hash.as_deref(), Some(HASH));
        assert_eq!(
            r.magnet.as_deref(),
            Some(
                "magnet:?xt=urn:btih:232cd67eb3ffbd7c37bf9ec3ee887417e5ae1ee6\
                 &dn=Clinton%20D.%2C%20Negus%20C.%20Ubuntu%20Linux%20Bible%2011ed%202025"
            )
        );
        assert!(!r.needs_magnet, "列表页自带 hash，磁力是算出来的");
        assert_eq!(r.size, Some(8_556_380));
        assert_eq!(r.size_text, "8.2 MB");
        assert_eq!(r.seeders, Some(35));
        assert_eq!(r.leechers, Some(0));
        assert_eq!(r.category.as_deref(), Some("Other"));
        assert_eq!(
            r.detail_url.as_deref(),
            Some(
                "https://limetorrents.fun\
                 /Clinton-D --Negus-C -Ubuntu-Linux-Bible-11ed-2025-torrent-19390961.html"
            ),
            "详情链接里的空格原样保留（JS 是裸拼接）"
        );
        assert!(r.date.is_some(), "\"9 months ago\" 是能解析的");
    }

    /// 抠不到 itorrents hash 的行**整条丢掉**（JS 的 `if (!infoHash) return`）。
    #[test]
    fn rows_without_an_itorrents_hash_are_skipped() {
        let rows = format!(
            "{}{}{}",
            header_row(),
            row(
                "no hash",
                "/a.html",
                "https://example.com/nope.torrent",
                "1 day ago",
                "1 MB",
                "1",
                "0"
            ),
            row("no file link", "/b.html", "", "1 day ago", "1 MB", "1", "0")
        );
        let out = parse_page(&rows);

        assert!(out.is_empty(), "两条都该被丢掉");
    }

    #[test]
    fn rows_without_a_name_are_skipped() {
        let rows = format!(
            "{}{}{}",
            header_row(),
            row("", "/a.html", FILE_HREF, "1 day ago", "1 MB", "1", "0"),
            row("ok", "/b.html", FILE_HREF, "1 day ago", "1 MB", "1", "0")
        );
        let out = parse_page(&rows);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "ok");
    }

    /// 站上的 `1 Year+` 解析不出日期 —— 钉住这个上游行为。
    #[test]
    fn a_truncated_relative_date_yields_no_date() {
        let rows = format!(
            "{}{}",
            header_row(),
            row(
                "n",
                "/a.html",
                FILE_HREF,
                "1 Year+ - in Other",
                "1 MB",
                "1",
                "0"
            )
        );
        let out = parse_page(&rows);

        assert_eq!(out[0].date, None, "\"1 Year+\" 没有 ago，parseDate 不认");
        assert_eq!(out[0].date_text, "—");
    }

    /// 详情链接缺失时 `detail_url` 是 `None`（JS 允许），但结果仍然保留。
    #[test]
    fn a_row_without_a_detail_href_is_still_kept() {
        let rows = format!(
            "{}{}",
            header_row(),
            r#"<tr><td class="tdleft"><div class="tt-name">
                 <a href="http://itorrents.net/torrent/ABCDEF.torrent"></a>
                 <a>name only</a>
               </div></td>
               <td class="tdnormal">1 day ago</td>
               <td class="tdnormal">1 MB</td>
               <td class="tdseed">1</td>
               <td class="tdleech">0</td>
               <td></td></tr>"#
        );
        let out = parse_page(&rows);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].detail_url, None);
    }
}
