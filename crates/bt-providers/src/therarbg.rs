//! TheRarBg —— HTML 抓取（B 组第 5 个）。
//!
//! 从 `src/providers/therarbg.js`（91 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果行是 `table > tbody > tr.list-entry`（fixture 里 **39 行**），每行 8 个 td：
//!
//! ```text
//! 1  td.hideCell        分类图标（文本为空）
//! 2  td.cellName        <div><a href="/post-detail/…">名称</a>…</div>
//! 3  td.hideCell        <a href="/get-posts/category:…">分类名</a>
//! 4  td.hideCell[data-order=unix 秒]   上传日期
//! 5  td.hideCell[data-order=同上]      "10 months, 1 week"（相对时间，JS 不读）
//! 6  td.sizeCell[data-order=字节]      大小
//! 7  td                 seeders
//! 8  td                 leechers
//! ```
//!
//! ⚠️ **大小和日期都不在文本里，在 `data-order` 属性里**（字节 / unix 秒），
//! JS 分别 `parseInt(...)` 与 `parseInt(...)*1000`。两条都有探针钉住。
//!
//! ⚠️ 第 2 格里**可能有两个 `<a>`**（fixture 里第 28 行就是一个：post-detail + 一个
//! `/imdb-detail/tt…` 的 M.Q.A 徽章）—— JS 用 `.first()` 取第一个，
//! 所以徽章不会跑进名称里。这也是 `name.count_nonempty_texts = 40` 而只有 39 行的原因。
//!
//! ## 磁力是惰性的
//!
//! 列表页 `a[href^="magnet:?"]` 有 **0 条**（探针 `magnet.count = 0`），
//! 磁力只在详情页（探针 `magnet.first_href`）→ 每条结果 `needs_magnet: true`，
//! 用户点击时走 [`resolve_magnet`]。
//!
//! ## 与 `audiobookbay` 的两处关键差异（都照抄 JS，别"统一"掉）
//!
//! 1. **抓到了行但一条都解析不出来时，error 是 `null` 而不是 `no_results_parsed`**
//!    （只有 `rows.length === 0` 才报 `no_results_parsed`）。于是 `runMirrors` 会拼出
//!    一个**空括号**的错误串 `TheRarBg unreachable ()`。
//! 2. `resolveMagnet` 不返回 `infoHash`（`audiobookbay` 返回），失败串也不同
//!    （`no_html` / `no_magnet_on_page`）。

use bt_core::dom::{attr, text, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, normalize, to_int, NumOrText, RawResult};
use bt_core::TorrentResult;

use crate::{MagnetOutcome, SearchOutcome};

/// 镜像列表，与 JS 的 `DOMAINS` 一致。
/// ⚠️ `therarbg.org` 实测 **ENOTFOUND**（2026-10-10），保留只为对齐 JS。
pub const DOMAINS: &[&str] = &[
    "https://therarbg.com",
    "https://therarbg.to",
    "https://therarbg.org",
];

/// 结果行选择器（JS: `table > tbody > tr.list-entry`）。
const ROWS: &str = "table > tbody > tr.list-entry";

/// 名称链接（JS: `td.cellName > div > a` 的**第一个**）。
const NAME_LINK: &str = "td.cellName > div > a";

/// 大小格（`data-order` 是字节数）。
const SIZE_CELL: &str = "td.sizeCell";

/// 日期格（`data-order` 是 unix 秒）。
const DATE_CELL: &str = "td:nth-child(4)";

/// 分类链接（第 3 格）。
const CATEGORY_LINK: &str = "td:nth-child(3) > a";

/// seeders / leechers 两格。
const SEEDERS_CELL: &str = "td:nth-child(7)";
const LEECHERS_CELL: &str = "td:nth-child(8)";

/// 详情页里的磁力链（列表页没有，见文件头）。
const MAGNET_LINK: &str = r#"a[href^="magnet:?"]"#;

/// 站点分类 → 项目标准分类。逐字照抄 JS 的 `switch`：
/// ⚠️ 只认这几个**精确**拼写，所以站上的 `E-books` / `Other` 之类一律落到 `Other`
/// （fixture 第 3 行的分类就因此从 `E-books` 变成 `Other`）。
fn category_from_raw(raw: &str) -> String {
    match raw.trim() {
        "Anime" => "Anime",
        "Apps" => "Apps",
        "Books" => "Books",
        "Games" => "Games",
        "Movies" => "Movies",
        "Music" => "Music",
        "XXX" => "Porn",
        "Tv" => "Series",
        _ => "Other",
    }
    .to_string()
}

/// 用默认镜像搜索。`page` 与 JS 一样**收下但不用** —— URL 里没有分页参数。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。
///
/// ⚠️ JS 传的 name 是 **`'TheRarBg'`（驼峰大写）**，错误串必须逐字一致。
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

    SearchOutcome::err(format!("TheRarBg unreachable ({})", errs.join("; ")))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    // JS: `${base}/get-posts/keywords:${encodeURIComponent(query)}`
    let url = format!("{base}/get-posts/keywords:{}", encode_uri_component(query));

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

    // ⚠️ 与 audiobookbay 不同：这里**全被过滤掉也算正常**（JS 返回的是 `error: null`），
    // 不报 `no_results_parsed`。要改就两版一起改。
    SearchOutcome::ok(results)
}

/// 解析一行 `tr.list-entry`。对应 JS 里 `rows.each(…)` 的那个回调。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    // JS 是 `.first()` —— 第 2 格里可能有第二个 `<a>`（IMDb 徽章），用的是第一个
    let link = dom.find(row, NAME_LINK).into_iter().next()?;

    let name = text_trim(link);
    if name.is_empty() {
        return None;
    }
    let href = attr(link, "href").filter(|h| !h.is_empty())?;

    // JS: `href.startsWith('http') ? href : `${base}${href}`` —— 裸拼接。
    // 本站 href 一定是 `/post-detail/…`（fixture 39 行全以 `/` 开头），不会拼出死链。
    let detail_url = if href.starts_with("http") {
        href
    } else {
        format!("{base}{href}")
    };

    // `data-order` 是纯数字字符串：`"8493465"` / `"1764263429"`。
    // JS 直接 `parseInt(x, 10)`（不走它自己的 `toInt` 助手），
    // 这里用 `to_int` —— 它多一步「去掉逗号」，而 `data-order` 里不会有逗号
    // （fixture 实测），所以两条路在这一站等价。
    let size = data_order(dom, row, SIZE_CELL)
        .as_deref()
        .and_then(|s| to_int(Some(&NumOrText::Text(s.to_string()))))
        .map(NumOrText::Num);

    let date = data_order(dom, row, DATE_CELL)
        .as_deref()
        .and_then(|s| to_int(Some(&NumOrText::Text(s.to_string()))))
        .map(|secs| NumOrText::Num(secs.saturating_mul(1000)));

    let category = dom
        .find(row, CATEGORY_LINK)
        .into_iter()
        .next()
        .map(|a| category_from_raw(&text_trim(a)))
        .unwrap_or_else(|| category_from_raw(""));

    Some(normalize(&RawResult {
        provider: "therarbg".to_string(),
        name: Some(name),
        size,
        seeders: Some(NumOrText::Text(cell_text(dom, row, SEEDERS_CELL))),
        leechers: Some(NumOrText::Text(cell_text(dom, row, LEECHERS_CELL))),
        date,
        category: Some(category),
        detail_url: Some(detail_url),
        // 列表页没有磁力 → JS 显式传 needsMagnet: true，`normalize` 也会自己算出来
        magnet: None,
        ..Default::default()
    }))
}

/// 取某个格子上的 `data-order` 属性。空的（`""`）按 JS 的 falsy 处理成 `None`。
fn data_order(dom: &Dom, row: ElementRef<'_>, cell: &str) -> Option<String> {
    dom.find(row, cell)
        .into_iter()
        .next()
        .and_then(|td| attr(td, "data-order"))
        .filter(|s| !s.is_empty())
}

/// 取某个格子的文本（JS 不 trim，交给 `normalize` 里的 `toInt`）。
fn cell_text(dom: &Dom, row: ElementRef<'_>, cell: &str) -> String {
    dom.find(row, cell)
        .into_iter()
        .next()
        .map(text)
        .unwrap_or_default()
}

/// 惰性解析磁力。对应 JS 的 `resolveMagnet(detailUrl)`。
///
/// ⚠️ JS 在这里显式传了 `{ 'User-Agent': pickUA() }`；本移植**不传** ——
/// `bt_core::http` 默认就送 `pick_ua()` 池里的一条（`default_request_carries_pool_ua_and_accept_headers`
/// 钉住了这点），两边效果一致，少一层无用参数。
///
/// ⚠️ 返回里**没有 `info_hash`**（照抄 JS，虽然从 magnet 里能抠出来）。
pub async fn resolve_magnet(http: &HttpClient, detail_url: &str) -> MagnetOutcome {
    let resp = http.get_text(detail_url, None).await;
    // JS: `if (error || !html) return { magnet: null, error: error || 'no_html' }`
    if let Some(e) = resp.error {
        return MagnetOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return MagnetOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    // 源玛里是 `&amp;dn=…`，`attr()` 取到的是**解码后**的值（dom.rs 已用探针钉过）
    match dom
        .select(MAGNET_LINK)
        .first()
        .and_then(|a| attr(*a, "href"))
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

    #[test]
    fn category_mapping_matches_the_js_switch() {
        assert_eq!(category_from_raw("Anime"), "Anime");
        assert_eq!(category_from_raw("Apps"), "Apps");
        assert_eq!(category_from_raw("Books"), "Books");
        assert_eq!(category_from_raw("Games"), "Games");
        assert_eq!(category_from_raw("Movies"), "Movies");
        assert_eq!(category_from_raw("Music"), "Music");
        assert_eq!(category_from_raw("XXX"), "Porn");
        assert_eq!(category_from_raw("Tv"), "Series");
        assert_eq!(category_from_raw("Other"), "Other");
        // ⚠️ 站上有但 JS 的 switch 没列的，一律落到 Other
        assert_eq!(category_from_raw("E-books"), "Other");
        assert_eq!(category_from_raw(""), "Other");
        // 大小写敏感 + 会 trim
        assert_eq!(category_from_raw("tv"), "Other");
        assert_eq!(category_from_raw("  Tv  "), "Series");
    }

    /// 一行 8 个 td，大小/日期走 `data-order` —— 用合成页面钉住取值路径。
    fn page_with(rows: &str) -> String {
        format!("<html><body><table><tbody>{rows}</tbody></table></body></html>")
    }

    fn row(
        name: &str,
        href: &str,
        cat: &str,
        size: &str,
        date: &str,
        seeds: &str,
        leech: &str,
    ) -> String {
        format!(
            r#"<tr class="list-entry">
                 <td class="hideCell"><img src="/i.gif" /></td>
                 <td class="cellName"><div class="wrapper">
                   <a href="{href}" style="font-weight: 700">{name}</a></a>
                   <span class="tooltip"><img src="/t.jpg" /></span></div></td>
                 <td class="hideCell"><a href="/get-posts/category:Other/">{cat}</a></td>
                 <td class="hideCell" data-order="{date}"><div>{date}</div></td>
                 <td class="hideCell" data-order="{date}"><div>10 months</div></td>
                 <td style="text-align: left;" class="sizeCell" data-order="{size}">{size}</td>
                 <td style="color: green">{seeds}</td>
                 <td style="color: red">{leech}</td>
               </tr>"#
        )
    }

    fn parse_page(base: &str, rows: &str) -> Vec<TorrentResult> {
        let dom = Dom::parse(&page_with(rows));
        dom.select(ROWS)
            .iter()
            .filter_map(|r| parse_row(&dom, *r, base))
            .collect()
    }

    const BASE: &str = "https://therarbg.com";

    #[test]
    fn parses_a_row_end_to_end() {
        let out = parse_page(
            BASE,
            &row(
                "Ubuntu Linux Bible 11E",
                "/post-detail/810de9/x/",
                "Other",
                "8493465",
                "1764263429",
                "149",
                "3",
            ),
        );

        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.provider, "therarbg");
        assert_eq!(r.name, "Ubuntu Linux Bible 11E");
        assert_eq!(
            r.detail_url.as_deref(),
            Some("https://therarbg.com/post-detail/810de9/x/")
        );
        assert_eq!(r.size, Some(8_493_465), "字节数直接来自 data-order");
        assert_eq!(r.size_text, "8.1 MB");
        assert_eq!(r.seeders, Some(149));
        assert_eq!(r.leechers, Some(3));
        assert_eq!(r.date, Some(1_764_263_429_000), "秒 × 1000");
        assert_eq!(r.date_text, "2025-11-27");
        assert_eq!(r.category.as_deref(), Some("Other"));
        assert_eq!(r.magnet, None);
        assert_eq!(r.info_hash, None);
        assert!(r.needs_magnet, "列表页没有磁力");
    }

    /// ⚠️ 第 2 格里的**第二个** `<a>`（IMDb 徽章）不能顶掉名称 —— JS 用 `.first()`。
    #[test]
    fn the_imdb_badge_link_does_not_replace_the_name() {
        let rows = r#"<tr class="list-entry">
                 <td class="hideCell"><img src="/i.gif" /></td>
                 <td class="cellName"><div class="wrapper">
                   <a href="/post-detail/7879f1/x/">Celtics City S01E08</a>
                   <a href="/imdb-detail/tt34555174/"><span class="badge">M.Q.A</span></a>
                 </div></td>
                 <td class="hideCell"><a href="/get-posts/category:Tv/">Tv</a></td>
                 <td class="hideCell" data-order="1764263429"><div></div></td>
                 <td class="hideCell" data-order="1764263429"><div></div></td>
                 <td class="sizeCell" data-order="2461965209">2.3 GB</td>
                 <td>2</td>
                 <td>1</td>
               </tr>"#;

        let out = parse_page(BASE, rows);

        assert_eq!(out.len(), 1, "一行只出一条（不是两个 <a> 两条）");
        assert_eq!(out[0].name, "Celtics City S01E08");
        assert_eq!(
            out[0].detail_url.as_deref(),
            Some("https://therarbg.com/post-detail/7879f1/x/")
        );
        assert_eq!(out[0].category.as_deref(), Some("Series"), "Tv → Series");
    }

    #[test]
    fn rows_without_name_or_href_are_skipped() {
        let rows = format!(
            "{}{}{}",
            row("", "/post-detail/1/x/", "Other", "1", "1", "1", "0"),
            row("no href", "", "Other", "1", "1", "1", "0"),
            row("ok", "/post-detail/2/x/", "Other", "1", "1", "1", "0")
        );
        let out = parse_page(BASE, &rows);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "ok");
    }

    /// 没有行 → `no_results_parsed`；有行但全被过滤 → **`error: None`**（JS 的怪癖）。
    #[test]
    fn rows_present_but_unparseable_yields_empty_results_without_error() {
        let rows = row("", "", "Other", "1", "1", "1", "0");
        let out = parse_page(BASE, &rows);

        assert!(out.is_empty(), "全被过滤掉了");
    }

    #[test]
    fn missing_data_order_leaves_size_and_date_empty() {
        let rows = r#"<tr class="list-entry">
                 <td class="hideCell"></td>
                 <td class="cellName"><div><a href="/post-detail/1/x/">n</a></div></td>
                 <td class="hideCell"><a href="/c/">Other</a></td>
                 <td class="hideCell"><div>2025-01-01</div></td>
                 <td class="hideCell"><div>1 year</div></td>
                 <td class="sizeCell">1.5 GB</td>
                 <td>7</td>
                 <td>1</td>
               </tr>"#;
        let out = parse_page(BASE, rows);

        assert_eq!(out[0].size, None, "data-order 不在就拿不到字节数");
        assert_eq!(out[0].size_text, "—");
        assert_eq!(out[0].date, None);
        assert_eq!(out[0].date_text, "—");
        assert_eq!(out[0].seeders, Some(7), "peers 是文本，照样能取");
    }

    #[test]
    fn absolute_detail_urls_are_kept_verbatim() {
        let out = parse_page(
            BASE,
            &row(
                "n",
                "https://mirror.example/post-detail/1/x/",
                "Other",
                "1",
                "1",
                "1",
                "0",
            ),
        );
        assert_eq!(
            out[0].detail_url.as_deref(),
            Some("https://mirror.example/post-detail/1/x/")
        );
    }
}
