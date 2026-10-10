//! Torrent9 —— 法站 HTML 抓取（B 组第 7 个）。
//!
//! 从 `src/providers/torrent9.js`（126 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表 `table > tbody > tr`（fixture 里 **3 行**，第一行是 `<tr><th>` 表头），每行 5 个 td：
//!
//! ```text
//! 1  <i 图标></i><a href="/torrent/49542/…"><h3><span class="blue">Ubuntu</span> Ultimate…</h3></a>
//! 2  30/08/2018        ← 日期
//! 3  1.9Go             ← 大小（法语单位）
//! 4  3                 ← seeders
//! 5  3                 ← leechers
//! ```
//!
//! ## ⚠️ 上游 bug：**第 2~5 格全被丢掉**（照抄，未修）
//!
//! JS 的 `searchOn` 只往 `normalize()` 里塞 `name` + `detailUrl`：
//!
//! ```js
//! normalize({ provider: 'torrent9', name, magnet: null, detailUrl });
//! ```
//!
//! 日期/大小/seed/leech 明明就在同一行的第 2~5 格里，它不要 —— 于是**卡片上除名字外全是 `—`**，
//! 并且每条都要等用户点击时再去详情页跑一趟。
//! 详情页那边其实也白解析了一遍（见下），所以这是个纯粹的「数据白扔」bug。
//! 照抄的理由与别处一致：阶段一的验收点是「与 Node 版逐字段一致」，改了就对不上。
//!
//! ## ⚠️ 详情页解析有一半是死代码（**本移植故意不抄**）
//!
//! JS 的 `parseDetail` 会从详情页抠出 `size`（`Poids du torrent`）、`seeders`、
//! `leechers`、`date`（`Date d'ajout`）、`category`（`Catégories` → `categoryFromRaw`），
//! 但唯一调用它的 `resolveMagnet` **只返回 `{ magnet, infoHash, error }`** —— 其余字段算完就扔。
//! 所以本移植只抄了「名称守卫 + 磁力 + info hash」这条真正可达的路径，
//! 连 `categoryFromRaw` 都没搬（它只服务于那条死路径）。
//! 哪天要做「点击后补齐卡片字段」，照 `src/providers/torrent9.js` 的 `parseDetail` 补即可。
//!
//! ## 磁力的两条分支都归到 `no_magnet`
//!
//! `fetchDetails` 在 HTTP 出错、正文为空、或 `parseDetail` 返回 null（缺 h1 或磁力）时，
//! 一律让 `resolveMagnet` 报 `no_magnet` —— 与 `therarbg`（会把 HTTP 错误原样透出）**不同**。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, extract_info_hash, normalize, RawResult};
use bt_core::TorrentResult;

use crate::{MagnetOutcome, SearchOutcome};

/// 镜像列表，与 JS 的 `DOMAINS` 一致。
/// ⚠️ `torrent9.so` 实测能连上但结果页是空的（0 个 `tr`），保留只为对齐 JS。
pub const DOMAINS: &[&str] = &[
    "https://www6.torrent9.to",
    "https://www.torrent9.to",
    "https://torrent9.so",
    "https://ww1.torrent9.to",
];

/// 结果行（含表头行；表头是 `<th>`，取不到 `<td> > a` 自然就跳过了）。
const ROWS: &str = "table > tbody > tr";

/// 名称 + 详情链接。
const NAME_LINK: &str = "td:nth-child(1) > a";

/// 详情页里的磁力链。
const MAGNET_LINK: &str = r#"a[href^="magnet:?"]"#;

/// 详情页的名称（`parseDetail` 的第一道守卫）。
const DETAIL_H1: &str = "div.movie-section h1";

/// 用默认镜像搜索。`page` 与 JS 一样**收下但不用** —— URL 里没有分页参数。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。
///
/// ⚠️ JS 传的 name 是 **`'Torrent9'`**，错误串必须逐字一致。
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

    SearchOutcome::err(format!("Torrent9 unreachable ({})", errs.join("; ")))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    // JS: `${base}/search_torrent/${encodeURIComponent(query)}.html`
    let url = format!("{base}/search_torrent/{}.html", encode_uri_component(query));

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

    // ⚠️ 这一站和 audiobookbay 一样：**两个分支都报错**
    //（与 therarbg / limetorrents 那种「全被过滤掉也算正常」不同）
    if results.is_empty() {
        return SearchOutcome::err("no_results_parsed");
    }
    SearchOutcome::ok(results)
}

/// 解析一行。只取名称与详情链接 —— 其余四格照抄 JS 丢掉（见文件头）。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let link = dom.find(row, NAME_LINK).into_iter().next()?;
    let href = attr(link, "href").filter(|h| !h.is_empty())?;

    // JS: `href.startsWith('http') ? href : `${base}${href}`` —— 裸拼接
    let detail_url = if href.starts_with("http") {
        href
    } else {
        format!("{base}{href}")
    };

    // JS: `$a.text().trim() || '(untitled)'`
    let raw_name = text_trim(link);
    let name = if raw_name.is_empty() {
        "(untitled)".to_string()
    } else {
        raw_name
    };

    Some(normalize(&RawResult {
        provider: "torrent9".to_string(),
        name: Some(name),
        // 列表页不解析磁力 → normalize 会自己把 needs_magnet 置 true
        magnet: None,
        detail_url: Some(detail_url),
        ..Default::default()
    }))
}

/// 惰性解析磁力。对应 JS 的 `resolveMagnet(detailUrl)`。
///
/// ⚠️ 三种失败（HTTP 错 / 空正文 / 缺 h1 或磁力）**都归到 `no_magnet`** —— 照抄 JS。
pub async fn resolve_magnet(http: &HttpClient, detail_url: &str) -> MagnetOutcome {
    match detail_magnet(http, detail_url).await {
        Some((magnet, info_hash)) => MagnetOutcome {
            magnet: Some(magnet),
            info_hash,
            error: None,
        },
        None => MagnetOutcome::err("no_magnet"),
    }
}

/// 详情页 → `(磁力, info hash)`。对应 JS 的 `fetchDetails` + `parseDetail` 里那条可达路径。
async fn detail_magnet(http: &HttpClient, detail_url: &str) -> Option<(String, Option<String>)> {
    let resp = http.get_text(detail_url, None).await;
    if resp.error.is_some() {
        return None;
    }
    let html = resp.html.filter(|h| !h.is_empty())?;
    let dom = Dom::parse(&html);

    let name = dom
        .select(DETAIL_H1)
        .first()
        .map(|h| text_trim(*h))
        .unwrap_or_default();
    let magnet = dom
        .select(MAGNET_LINK)
        .first()
        .and_then(|a| attr(*a, "href"))
        .filter(|h| !h.is_empty());

    // JS: `if (!name || !magnet) return null;` —— 缺名字或磁力整条不算
    if name.is_empty() {
        return None;
    }
    let magnet = magnet?;

    // JS 用 `extractInfoHash(magnet)`，是**小写化**的（磁力串里可能是大写）
    let hash = extract_info_hash(Some(&magnet));
    Some((magnet, hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://www6.torrent9.to";
    const MAGNET: &str =
        "magnet:?xt=urn:btih:FEBD9A2CB755EC82E6E7A015A8DC497FDE9DD507&tr=udp://x:6969/announce";

    fn row(name: &str, href: &str) -> String {
        format!(
            "<tr><td><i class=\"fa\"></i><a title=\"{name}\" href=\"{href}\"><h3>{name}</h3></a></td>\
             <td>30/08/2018</td><td>1.9Go</td><td>3</td><td>3</td></tr>"
        )
    }

    fn header_row() -> &'static str {
        "<tr><th>Nom du torrent</th><th>Date</th><th>Taille</th><th>Seed</th><th>Leech</th></tr>"
    }

    fn page(rows: &str) -> String {
        format!(
            "<html><body><table><thead>{}</thead><tbody>{rows}</tbody></table></body></html>",
            header_row()
        )
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
            "Ubuntu Ultimate Edition 1.4 DVD",
            "/torrent/49542/ubuntu-ultimate-edition-1-4-dvd",
        ));

        assert_eq!(out.len(), 1, "表头行不算");
        let r = &out[0];
        assert_eq!(r.provider, "torrent9");
        assert_eq!(r.name, "Ubuntu Ultimate Edition 1.4 DVD");
        assert_eq!(
            r.detail_url.as_deref(),
            Some("https://www6.torrent9.to/torrent/49542/ubuntu-ultimate-edition-1-4-dvd")
        );
        assert!(r.needs_magnet);
        assert_eq!(r.magnet, None);
        assert_eq!(r.info_hash, None);

        // ⚠️ 站上第 2~5 格明明有日期/大小/peers，JS 不要 —— 照抄
        assert_eq!(r.size, None);
        assert_eq!(r.size_text, "—");
        assert_eq!(r.seeders, None);
        assert_eq!(r.leechers, None);
        assert_eq!(r.date, None);
        assert_eq!(r.date_text, "—");
        assert_eq!(r.category, None);
    }

    #[test]
    fn a_link_without_text_becomes_untitled() {
        let rows = "<tr><td><a href=\"/torrent/1/x\"><h3></h3></a></td></tr>";
        let out = parse_page(rows);

        assert_eq!(out[0].name, "(untitled)", "JS 的 `|| '(untitled)'`");
    }

    #[test]
    fn rows_without_href_are_skipped() {
        let rows = format!(
            "{}{}",
            "<tr><td><a>no href</a></td></tr>",
            row("ok", "/torrent/2/x")
        );
        let out = parse_page(&rows);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "ok");
    }

    #[test]
    fn absolute_detail_urls_are_kept_verbatim() {
        let out = parse_page(&row("n", "https://mirror.example/torrent/1/x"));
        assert_eq!(
            out[0].detail_url.as_deref(),
            Some("https://mirror.example/torrent/1/x")
        );
    }

    #[test]
    fn extract_info_hash_lowercases_the_detected_hash() {
        // 磁力串里是大写，normalize 的小写化只发生在 infoHash 字段上
        assert_eq!(
            extract_info_hash(Some(MAGNET)).as_deref(),
            Some("febd9a2cb755ec82e6e7a015a8dc497fde9dd507")
        );
    }
}
