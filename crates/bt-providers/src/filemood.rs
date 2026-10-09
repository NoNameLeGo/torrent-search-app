//! FileMood —— HTML 抓取（B 组第二个，第一个是 `linuxtracker`）。
//!
//! 从 `src/providers/filemood.js`（74 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表是一张 `<table>`，每个种子一行 `<tr>`。**全表 65 行，数据行只有 20 行** ——
//! 靠「这一行里有没有 `a.btn-success`（那个 DOWNLOAD 按钮）」分辨，与 JS 的
//! `$(tr).find('a.btn-success').length > 0` 一致。
//!
//! 数据行的列（DOM 顺序）：
//!
//! ```text
//! [td.dn-title]  片名。内含 <a>，且**搜索关键词被 <span class="highlated"> 拆成多段**
//!                （"ubuntu" + "-26.04-desktop-amd64.iso"），所以必须**拼接所有后代文本**
//!                再 trim —— 正好是 `dom::text_trim` 的语义
//! [td.dn-status] "2518/65"  → seeders / leechers
//! [td.dn-size]   "6.5 GB"
//! [td.dn-btn]    <div><a class="btn btn-success" href="…">DOWNLOAD</a></div>
//! ```
//!
//! 详情链接形如 `/ubuntu-26.04-desktop-amd64.iso-<40hex>.html`，**infoHash 直接从
//! 文件名尾部抠**，不必再请求详情页（对齐 JS 的 `parseInfoHash`）。
//!
//! ⚠️ 本 provider **没有日期**：JS 的 `normalize` 调用里没传 `date`，
//! 所以结果恒为 `date: None` + `date_text: "—"`（`formatDate(null)` 的产物）。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, normalize, NumOrText, RawResult};
use bt_core::TorrentResult;

use crate::SearchOutcome;

/// 镜像列表，与 JS 的 `DOMAINS` 一致（本站只有一个域名）。
pub const DOMAINS: &[&str] = &["https://filemood.com"];

/// 结果表的行选择器，与 JS 的 `$('table > tbody > tr')` 一致。
///
/// 裸 `<tr>` 会被解析器补上 `<tbody>`，所以这一层不能省。
const ROW_SELECTOR: &str = "table > tbody > tr";

/// 数据行的标志：行内有一个 DOWNLOAD 按钮。
const BTN_SUCCESS: &str = "a.btn-success";

const TITLE_CELL: &str = "td.dn-title";
const STATUS_CELL: &str = "td.dn-status";
const SIZE_CELL: &str = "td.dn-size";
const DETAIL_LINK: &str = "td.dn-btn > div > a";

/// 从详情页 URL 的文件名尾部抠 40 位 infoHash，逐行复刻 JS 的 `parseInfoHash`：
///
/// ```js
/// if (!detailUrl) return null;
/// let h = detailUrl;
/// if (h.endsWith('.html')) h = h.slice(0, -5);
/// const idx = h.lastIndexOf('-');
/// if (idx < 0) return null;
/// h = h.slice(idx + 1).toLowerCase().trim();
/// return /^[a-f0-9]{40}$/.test(h) ? h : null;
/// ```
///
/// 注意 JS 传入的是**拼好 base 的完整 URL**（`https://filemood.com/…`），
/// 所以这里也收完整 URL —— base 里没有 `-`，最后一个 `-` 仍是分隔符。
fn info_hash_from_detail_url(detail_url: &str) -> Option<String> {
    let h = detail_url.strip_suffix(".html").unwrap_or(detail_url);
    let idx = h.rfind('-')?;
    let tail = h[idx + 1..].to_lowercase();
    let tail = tail.trim();

    // 小写化之后，"十六进制" 等价于 [0-9a-f]。按字节判断，长度也是字节数。
    if tail.len() == 40 && tail.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(tail.to_string())
    } else {
        None
    }
}

/// 把 `"2518/65"` 拆成 (seeders, leechers)，复刻 JS：
///
/// ```js
/// const parts = statusText.split('/').map(s => s.trim());
/// const seeders  = parts[0] || null;   // 空串是 falsy → null
/// const leechers = parts[1] || null;   // 没有第二个 → undefined → null
/// ```
fn split_status(status: &str) -> (Option<NumOrText>, Option<NumOrText>) {
    let mut parts = status.split('/').map(str::trim);

    let mk = |v: &str| {
        if v.is_empty() {
            None
        } else {
            Some(NumOrText::Text(v.to_string()))
        }
    };

    (
        mk(parts.next().unwrap_or("")),
        mk(parts.next().unwrap_or("")),
    )
}

/// 相对链接拼到 base 上，等价于 JS 的 `` href.startsWith('http') ? href : base + href ``，
/// 但**顺手挡住两种会拼坏的情况**（base 结尾有斜杠 / href 开头有斜杠同时出现时的双斜杠）。
///
/// 本站的 href 都以 `/` 开头、base 又不带结尾斜杠，所以这里的输出与 JS **逐字相同**
/// —— 不属于 divergence，只是防患于未然（linuxtracker 那个死链 bug 就是这么来的）。
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

/// 用默认镜像搜索。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`：
///
/// - JS 是**并行**请求所有镜像、按**声明顺序**取第一个**结果非空**的（`allSettled`）
/// - 全部为空 → `error = "<name> unreachable (<各镜像错误用 '; ' 拼接>)"`，
///   且 `errs` 会先过一遍 `filter(Boolean)` —— **没有错误文本的尝试不占位**
/// - 因此「页面正常但没有结果」在这个 provider 里也算**错误**，不是空结果
///
/// ⚠️ 通用版（多镜像 + 并行）留到 C 组统一做；现在只有一个域名，顺序执行等价。
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

    SearchOutcome::err(format!("filemood unreachable ({})", errs.join("; ")))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    // JS: `${base}/result?q=${encodeURIComponent(query)}+in%3Atitle`
    // 注意那个 `+` 是字面量、`%3A` 是 `encodeURIComponent(':')` 的结果，直接写死。
    let url = format!("{base}/result?q={}+in%3Atitle", encode_uri_component(query));

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        // JS 原样把 axios 的错误文案抛出去，不改写
        return SearchOutcome::err(e);
    }

    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        // JS: `if (error || !html) return { results: [], error }` —— 走到这儿时 error 是
        // undefined，于是 runMirrors 的 `filter(Boolean)` 会把它滤掉，
        // 最终错误串是 `filemood unreachable ()`。这里同样返回「无错误 + 空结果」，
        // 让 `search_with` 拼出同一句话（属于 JS 的既有行为，先照抄）。
        return SearchOutcome::ok(Vec::new());
    };

    let dom = Dom::parse(&html);
    let rows = data_rows(&dom);
    if rows.is_empty() {
        // ⚠️ 注意判断的是**数据行数**（含 `a.btn-success` 的行），不是最终结果数
        // —— 与 JS 的 `if (rows.length === 0)` 位置一致
        return SearchOutcome::err("no_results_parsed");
    }

    SearchOutcome::ok(parse_rows(base, &dom, &rows))
}

/// 挑出数据行：`table > tbody > tr` 里含 `a.btn-success` 的那些。
fn data_rows<'a>(dom: &'a Dom) -> Vec<ElementRef<'a>> {
    dom.select(ROW_SELECTOR)
        .into_iter()
        .filter(|tr| !dom.find(*tr, BTN_SUCCESS).is_empty())
        .collect()
}

/// 取 scope 内第一个匹配元素的文本（`.first().text().trim()` 的等价物）。
fn first_text(dom: &Dom, scope: ElementRef<'_>, css: &str) -> String {
    dom.find(scope, css)
        .first()
        .map(|e| text_trim(*e))
        .unwrap_or_default()
}

/// 解析段（`search_at` 与测试共用，逻辑只有这一份）。
fn parse_rows(base: &str, dom: &Dom, rows: &[ElementRef<'_>]) -> Vec<TorrentResult> {
    let mut results = Vec::new();

    for tr in rows {
        let name = first_text(dom, *tr, TITLE_CELL);
        if name.is_empty() {
            continue;
        }

        let detail_href = dom
            .find(*tr, DETAIL_LINK)
            .first()
            .and_then(|a| attr(*a, "href"))
            .filter(|h| !h.is_empty());
        let detail_url = detail_href.map(|h| join_base(base, &h));

        // JS: `if (!infoHash) continue` —— 抠不到 hash 就整条丢掉
        let Some(info_hash) = detail_url.as_deref().and_then(info_hash_from_detail_url) else {
            continue;
        };

        let (seeders, leechers) = split_status(&first_text(dom, *tr, STATUS_CELL));

        results.push(normalize(&RawResult {
            provider: "filemood".to_string(),
            id: None,
            name: Some(name),
            info_hash: Some(info_hash),
            magnet: None,
            // JS 无条件把 size 字符串交给 normalize（可能是空串），这里保持一致
            size: Some(NumOrText::Text(first_text(dom, *tr, SIZE_CELL))),
            seeders,
            leechers,
            date: None,
            category: Some("Other".to_string()),
            detail_url,
            files: None,
        }));
    }

    results
}

/// 直接拿一段 HTML 解析（测试与将来的复用入口）。
pub fn parse(base: &str, html: &str) -> Vec<TorrentResult> {
    let dom = Dom::parse(html);
    let rows = data_rows(&dom);
    parse_rows(base, &dom, &rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://filemood.com";
    const H1: &str = "dafc8c076ca2f3ed376eeae7c76a0d6be2415c45";

    /// 造一行数据行（含 DOWNLOAD 按钮）。
    ///
    /// 片名刻意拆成两个 `<span>` —— 真实页面就是这么高亮关键词的，
    /// 顺带钉住「必须拼接所有后代文本」这条语义。
    fn row(href: &str, name: &str, status: &str, size: &str) -> String {
        format!(
            r#"<table><tr>
                 <td class="dn-title"><p class="filedir"><a href="{href}">
                   <span class="highlated">{name}</span></a></p></td>
                 <td class="dn-status"><p class="text"><b>{status}</b></p></td>
                 <td class="dn-size"><p class="text"><b>{size}</b></p></td>
                 <td class="dn-btn"><div style="width: 90px">
                   <a class="btn btn-mini btn-success" href="{href}">DOWNLOAD</a></div></td>
               </tr></table>"#
        )
    }

    fn detail(hash: &str) -> String {
        format!("/ubuntu-26.04-desktop-amd64.iso-{hash}.html")
    }

    #[test]
    fn parses_a_row_end_to_end() {
        let html = row(
            &detail(H1),
            "ubuntu-26.04-desktop-amd64.iso",
            "2518/65",
            "6.5 GB",
        );
        let out = parse(BASE, &html);

        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.provider, "filemood");
        assert_eq!(
            r.name, "ubuntu-26.04-desktop-amd64.iso",
            "跨 <span> 要把文本拼起来"
        );
        assert_eq!(r.info_hash.as_deref(), Some(H1));
        assert_eq!(r.size, Some(6_979_321_856));
        assert_eq!(r.size_text, "6.5 GB");
        assert_eq!(r.seeders, Some(2518));
        assert_eq!(r.leechers, Some(65));
        assert_eq!(r.category.as_deref(), Some("Other"), "该站分类写死 Other");
        assert_eq!(r.date, None, "JS 没传 date");
        assert_eq!(r.date_text, "—", "formatDate(null) 的产物，不是空串");

        let expected_detail = format!("{BASE}{}", detail(H1));
        assert_eq!(r.detail_url.as_deref(), Some(expected_detail.as_str()));
        assert!(!r.needs_magnet);
    }

    #[test]
    fn rows_without_the_download_button_are_ignored() {
        // 有 dn-title 但没有 a.btn-success → 不是数据行
        let html = format!(
            "<table><tr><td class=\"dn-title\">{}</td></tr></table>",
            detail(H1)
        );
        assert!(parse(BASE, &html).is_empty());
    }

    #[test]
    fn rows_without_a_title_are_skipped() {
        let html = format!(
            "<table><tr><td class=\"dn-status\">1/2</td><td class=\"dn-size\">1 GB</td>\
             <td class=\"dn-btn\"><div><a class=\"btn-success\" href=\"{}\">D</a></div></td></tr></table>",
            detail(H1)
        );
        assert!(parse(BASE, &html).is_empty());
    }

    #[test]
    fn rows_without_a_detail_link_are_skipped() {
        let html = "<table><tr><td class=\"dn-title\">no link here</td>\
             <td class=\"dn-btn\"><div><a class=\"btn-success\">D</a></div></td></tr></table>";
        assert!(parse(BASE, &html).is_empty(), "抠不到 infoHash 就整条丢");
    }

    #[test]
    fn rows_with_a_bad_hash_are_skipped() {
        for bad in [
            "tooshort",                                 // 长度不对
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz", // 非十六进制
            "dafc8c076ca2f3ed376eeae7c76a0d6be2415c4",  // 39 位
        ] {
            let html = row(&format!("/x-{bad}.html"), "n", "1/1", "1 GB");
            assert!(parse(BASE, &html).is_empty(), "bad hash {bad} 应被跳过");
        }
    }

    #[test]
    fn hash_is_lowercased() {
        let upper = "DAFC8C076CA2F3ED376EEAE7C76A0D6BE2415C45";
        let out = parse(BASE, &row(&detail(upper), "n", "1/1", "1 GB"));
        assert_eq!(out[0].info_hash.as_deref(), Some(H1), "大写要小写化");
    }

    #[test]
    fn info_hash_parser_edge_cases() {
        // 没有 .html 后缀也认（JS 的 endsWith 只是可选裁剪）
        assert_eq!(
            info_hash_from_detail_url(&format!("{BASE}/x-{H1}")).as_deref(),
            Some(H1)
        );
        // 没有 '-' → None
        assert_eq!(
            info_hash_from_detail_url("https://filemood.com/abcdef.html"),
            None
        );
        // 空串 → None（JS 的 `if (!detailUrl)`）
        assert_eq!(info_hash_from_detail_url(""), None);
        // 前后空白要 trim 掉（JS 的 .trim() 在 lastIndexOf 之后）
        assert_eq!(
            info_hash_from_detail_url(&format!("{BASE}/x-{H1} .html")).as_deref(),
            Some(H1)
        );
    }

    #[test]
    fn status_without_a_slash_sets_only_seeders() {
        let out = parse(BASE, &row(&detail(H1), "n", "42", "1 GB"));
        assert_eq!(out[0].seeders, Some(42));
        assert_eq!(out[0].leechers, None, "parts[1] 是 undefined → null");
    }

    #[test]
    fn empty_status_gives_neither_seeders_nor_leechers() {
        let out = parse(BASE, &row(&detail(H1), "n", "", "1 GB"));
        // JS: ''.split('/') === [''] → parts[0] = '' → falsy → null
        assert_eq!(out[0].seeders, None);
        assert_eq!(out[0].leechers, None);
    }

    #[test]
    fn status_parts_are_trimmed() {
        let out = parse(BASE, &row(&detail(H1), "n", " 12 / 3 ", "1 GB"));
        assert_eq!(out[0].seeders, Some(12));
        assert_eq!(out[0].leechers, Some(3));
    }

    #[test]
    fn join_base_handles_both_shapes() {
        // 本站的实际形态：base 不带结尾斜杠、href 以 / 开头 → 结果与 JS 逐字相同
        assert_eq!(
            join_base("https://filemood.com", "/a-b.html"),
            "https://filemood.com/a-b.html"
        );
        // 绝对链接原样保留
        assert_eq!(
            join_base("https://filemood.com", "https://x/y.html"),
            "https://x/y.html"
        );
        // 两种会拼坏的情况
        assert_eq!(join_base("https://x/", "/y"), "https://x/y");
        assert_eq!(join_base("https://x", "y"), "https://x/y");
    }
}
