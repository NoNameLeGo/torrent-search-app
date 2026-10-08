//! LinuxTracker —— HTML 抓取（B 组第一个，用来验证 `bt_core::dom` 这套地基）。
//!
//! 从 `src/providers/linuxtracker.js`（95 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表是一张扁平 `<table>`，每个种子一行 `<tr>`，单元格是 `td.lista`。
//! 但**候选链接有 43 条，主表行只有 33 条** —— 差额来自侧栏 Top10（每个 `<tr>` 只有 2 个 `<td>`），
//! JS 靠 `tds.length < 6` 挡掉，本移植保持一致。
//!
//! 主表行的列布局（实测）：
//!
//! ```text
//! [0] 分类图标  [1] 片名（colspan=2；" (EXT)" 之类的后缀在 <a> 外面，所以链接文本更干净）
//! [2] 下载  [3] 日期 DD/MM/YYYY  [4] 大小  [5] 上传者  [6] seeders  [7] leechers  [8] 推荐人
//! ```
//!
//! ⚠️ 主表里还夹着**展开的描述行**（19 个 td，列含义完全不同：`Added On: …` / `Size: …` /
//! `Seeds 7` …）。这类行只要名字链接非空就会被保留，于是 size/seeders 落在错误的列上、
//! 解析不出数字 —— **这是 JS 的既有行为，本移植照抄**，免得"修好"之后和 Node 版对不上。
//!
//! 详情页 URL 的 `id` 参数就是 40 位 infoHash，所以不必再请求一次详情页。

use bt_core::dom::{attr, closest_tag, text_trim, Dom};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, normalize, NumOrText, RawResult};
use bt_core::TorrentResult;
use chrono::{Local, TimeZone};

use crate::SearchOutcome;

/// 镜像列表，与 JS 的 `DOMAINS` 一致。
pub const DOMAINS: &[&str] = &["https://linuxtracker.org"];

/// 主表行至少有这么多单元格才认（与 JS 的 `tds.length < 6` 一致）。
const MIN_CELLS: usize = 6;

/// 名字链接的选择器（与 JS 逐字一致：含侧栏 Top10 的 2-td 行，靠 `MIN_CELLS` 过滤）。
const NAME_LINKS: &str = r#"td.lista a[href*="torrent-details"]"#;

/// 从详情链接里抠 40 位 infoHash，复刻 JS 的 `/[?&]id=([a-f0-9]{40})/i`。
///
/// ⚠️ 不能用 `bt_core::normalize::extract_info_hash` —— 那个只认 `btih:` 前缀
/// （对应 JS 里另一个函数），本站是 `?page=torrent-details&id=<hex>` 的形式。
///
/// 全程按字节比对，避免在非 ASCII 链接上切出非法 UTF-8 边界。
fn info_hash_from_href(href: &str) -> Option<String> {
    let b = href.as_bytes();
    let mut i = 1usize;
    while i + 3 <= b.len() {
        if &b[i..i + 3] == b"id=" && matches!(b[i - 1], b'?' | b'&') {
            let (start, end) = (i + 3, i + 43);
            if end <= b.len() && b[start..end].iter().all(u8::is_ascii_hexdigit) {
                return std::str::from_utf8(&b[start..end])
                    .ok()
                    .map(str::to_ascii_lowercase);
            }
        }
        i += 1;
    }
    None
}

/// `DD/MM/YYYY` → **本地时区**当日零点的毫秒时间戳。
///
/// ⚠️ 复刻 JS 的 `new Date(y, m-1, d).getTime()`：它用**本地时区**，
/// 而 `dateText` 是 `format_date` 按 UTC 格式化的 —— 于是在东八区会显示成**前一天**
/// （站上写 28/04/2026，界面显示 2026-04-27）。这是 JS 版既有的显示 bug；
/// 本移植照抄是为了阶段一的「逐字段一致」验收，要修就得两版一起修。
///
/// 不匹配时 JS 会把原字符串交给 `normalize` 的宽松解析，这里也一样。
fn parse_eu_date(raw: &str) -> Option<NumOrText> {
    let s = raw.trim();
    if s.is_empty() {
        // JS: `if (!s) return s` —— 空串原样返回（normalize 会当成"没有日期"）
        return Some(NumOrText::Text(String::new()));
    }

    let num = |t: &str, min: usize, max: usize| {
        t.len() >= min && t.len() <= max && t.bytes().all(|b| b.is_ascii_digit())
    };
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 3 || !num(parts[0], 1, 2) || !num(parts[1], 1, 2) || !num(parts[2], 4, 4) {
        return Some(NumOrText::Text(s.to_string()));
    }

    let day = parts[0].parse::<u32>().ok()?;
    let month = parts[1].parse::<u32>().ok()?;
    let year = parts[2].parse::<i32>().ok()?;

    match Local.with_ymd_and_hms(year, month, day, 0, 0, 0).single() {
        Some(dt) => Some(NumOrText::Num(dt.timestamp_millis())),
        // 退化路径：不存在的日期（如 32/13/2026）。JS 的 Date 会自行滚到下一月，
        // 这里退回原文让 normalize 去试。真实页面不会有这种值。
        None => Some(NumOrText::Text(s.to_string())),
    }
}

/// 用默认镜像搜索。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`（单域名时就是"只有一个尝试"）：
///
/// - JS 是**并行**请求所有镜像、按**声明顺序**取第一个**结果非空**的（`allSettled`）
/// - 全部为空 → `error = "<name> unreachable (<各镜像错误用 '; ' 拼接>)"`，
///   **带 provider 前缀**；注意这里会丢掉"单条错误"的原文
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

    SearchOutcome::err(format!("linuxtracker unreachable ({})", errs.join("; ")))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    let url = format!(
        "{base}/index.php?page=torrents&search={}&category=0&active=0",
        encode_uri_component(query)
    );

    let resp = http.get_text(&url, None).await;
    if let Some(e) = &resp.error {
        // JS 原样把 axios 的错误文案抛出去，不改写
        return SearchOutcome::err(e.clone());
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::err("no_html");
    };

    // JS 用 `td.lista a[href*="torrent-details"]`（43 条，含侧栏 Top10）；
    // 一条都没有才算"解析不出结果"
    let dom = Dom::parse(&html);
    if dom.select(NAME_LINKS).is_empty() {
        return SearchOutcome::err("no_results_parsed");
    }

    SearchOutcome::ok(parse_dom(base, &dom))
}

/// 解析段（`search_at` 与测试共用，逻辑只有这一份）。
fn parse_dom(base: &str, dom: &Dom) -> Vec<TorrentResult> {
    let links = dom.select(NAME_LINKS);

    let mut results = Vec::new();
    for link in links {
        let name = text_trim(link);
        if name.is_empty() {
            continue;
        }

        let href = attr(link, "href").filter(|h| !h.is_empty());
        let detail_url = href.as_ref().map(|h| {
            if h.starts_with("http") {
                h.clone()
            } else {
                format!("{base}{h}")
            }
        });
        let info_hash = href.as_deref().and_then(info_hash_from_href);

        // 往上找父 <tr>，按列索引取值
        let Some(tr) = closest_tag(link, "tr") else {
            continue;
        };
        let tds = dom.find(tr, "td");
        if tds.len() < MIN_CELLS {
            continue; // 侧栏 Top10 那种只有 2 个 td 的行
        }
        let cell = |i: usize| tds.get(i).map(|e| text_trim(*e)).unwrap_or_default();

        results.push(normalize(&RawResult {
            provider: "linuxtracker".to_string(),
            id: None,
            name: Some(name),
            info_hash,
            magnet: None,
            size: Some(NumOrText::Text(cell(4))),
            seeders: Some(NumOrText::Text(cell(6))),
            leechers: Some(NumOrText::Text(cell(7))),
            date: parse_eu_date(&cell(3)),
            category: Some("Apps".to_string()),
            detail_url,
            files: None,
        }));
    }

    results
}

/// 直接拿一段 HTML 解析（测试与将来的复用入口）。等价于 `search_at` 里那一半。
pub fn parse(base: &str, html: &str) -> Vec<TorrentResult> {
    parse_dom(base, &Dom::parse(html))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://linuxtracker.org";

    fn row(
        href_id: &str,
        name: &str,
        date: &str,
        size: &str,
        seeds: &str,
        leeches: &str,
    ) -> String {
        format!(
            r#"<table><tr>
                 <td class="lista">icon</td>
                 <td class="lista"><a href="index.php?page=torrent-details&amp;id={href_id}">{name}</a> (EXT)</td>
                 <td class="lista">dl</td>
                 <td class="lista">{date}</td>
                 <td class="lista">{size}</td>
                 <td class="lista">uploader</td>
                 <td class="lista">{seeds}</td>
                 <td class="lista">{leeches}</td>
                 <td class="lista">rec</td>
               </tr></table>"#
        )
    }

    #[test]
    fn extracts_info_hash_from_the_id_param() {
        assert_eq!(
            info_hash_from_href(
                "index.php?page=torrent-details&id=FF818211DE0EC5AD17349C6D0F4C1E8A058BA0F9"
            )
            .as_deref(),
            Some("ff818211de0ec5ad17349c6d0f4c1e8a058ba0f9"),
            "大写要小写化"
        );
        // 必须紧跟 ? 或 &
        assert_eq!(info_hash_from_href("x/valid=abc"), None);
        assert_eq!(
            info_hash_from_href("?noid=5a2759c487c21a692df3e521cbcb3df8731bfb5f"),
            None
        );
        // 位数不足
        assert_eq!(info_hash_from_href("?id=5a2759c4"), None);
        // 非 hex
        assert_eq!(
            info_hash_from_href("?id=zz2759c487c21a692df3e521cbcb3df8731bfb5f"),
            None
        );
        assert_eq!(info_hash_from_href(""), None);
    }

    #[test]
    fn eu_dates_use_local_midnight_like_js() {
        let ms = match parse_eu_date("28/04/2026") {
            Some(NumOrText::Num(n)) => n,
            other => panic!("该解析成时间戳，得到 {other:?}"),
        };
        let expected = Local
            .with_ymd_and_hms(2026, 4, 28, 0, 0, 0)
            .single()
            .expect("合法日期")
            .timestamp_millis();
        assert_eq!(
            ms, expected,
            "必须是本地时区当日零点（与 JS 的 new Date(y,m-1,d) 一致）"
        );

        // 不匹配的输入原样返回，交给 normalize
        assert_eq!(
            parse_eu_date("2026-04-28"),
            Some(NumOrText::Text("2026-04-28".to_string()))
        );
        assert_eq!(
            parse_eu_date("28/4/26"),
            Some(NumOrText::Text("28/4/26".to_string())),
            "年份必须 4 位"
        );
        assert_eq!(
            parse_eu_date(""),
            Some(NumOrText::Text(String::new())),
            "空串原样返回"
        );
    }

    #[test]
    fn sidebar_rows_with_two_cells_are_skipped() {
        // 侧栏 Top10 的结构：一个 tr 只有 2 个 td
        let html = r#"<table>
            <tr><td class="lista">5.0000</td><td class="lista"><a href="?page=torrent-details&id=9ac4c70e206139ea618205db73c4e2690516c28c">manjaro</a></td></tr>
        </table>"#;
        assert!(parse(BASE, html).is_empty(), "td 少于 6 个要跳过");
    }

    #[test]
    fn rows_without_name_are_skipped() {
        let html = row(
            "5a2759c487c21a692df3e521cbcb3df8731bfb5f",
            "",
            "28/04/2026",
            "3.14 GB",
            "15",
            "0",
        );
        assert!(
            parse(BASE, html).is_empty(),
            "名字为空的（展开行的空链接）要跳过"
        );
    }

    #[test]
    fn parses_one_full_row() {
        let html = row(
            "5a2759c487c21a692df3e521cbcb3df8731bfb5f",
            "Fedora-KDE-Desktop-Live-x86_64-44",
            "28/04/2026",
            "3.14 GB",
            "15",
            "0",
        );
        let out = parse(BASE, &html);
        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.provider, "linuxtracker");
        assert_eq!(
            r.name, "Fedora-KDE-Desktop-Live-x86_64-44",
            "取 <a> 的文本，(EXT) 后缀在外面不算"
        );
        assert_eq!(r.size, Some(3_371_549_327));
        assert_eq!(r.size_text, "3.1 GB");
        assert_eq!(r.seeders, Some(15));
        assert_eq!(r.leechers, Some(0));
        assert_eq!(r.category.as_deref(), Some("Apps"));
        assert_eq!(
            r.detail_url.as_deref(),
            Some("https://linuxtracker.org/index.php?page=torrent-details&id=5a2759c487c21a692df3e521cbcb3df8731bfb5f"),
            "相对链接要拼上 base"
        );
        assert_eq!(
            r.info_hash.as_deref(),
            Some("5a2759c487c21a692df3e521cbcb3df8731bfb5f")
        );
        assert!(r
            .magnet
            .as_deref()
            .unwrap()
            .starts_with("magnet:?xt=urn:btih:5a2759c4"));
    }

    /// 展开的描述行（19 个 td）列含义不同：size/seeders 落在错误列上 → 解析不出数字。
    /// JS 也是这个结果，**别"修"**。
    #[test]
    fn expanded_description_rows_yield_degraded_fields_like_js() {
        let html = r#"<table><tr>
            <td class="lista">icon</td>
            <td class="lista"><a href="index.php?page=torrent-details&id=ff818211de0ec5ad17349c6d0f4c1e8a058ba0f9">4MLinux 52 0 core iso</a></td>
            <td class="lista">Added On: 09/08/2026</td>
            <td class="lista"></td>
            <td class="lista">Size: 16.13 MB</td>
            <td class="lista"></td>
            <td class="lista">Seeds 7</td>
            <td class="lista"></td>
            <td class="lista">Leechers 0</td>
            <td class="lista"></td>
            <td class="lista">Completed 8</td>
            <td class="lista"></td>
            <td class="lista">---</td>
            <td class="lista">TheLinuxMan</td>
            <td class="lista">N/A</td>
            <td class="lista">N/A</td>
            <td class="lista">---</td>
            <td class="lista"></td>
            <td class="lista"></td>
        </tr></table>"#;
        let out = parse(BASE, html);
        assert_eq!(out.len(), 1, "名字非空 → 会被保留（与 JS 一致）");
        let r = &out[0];
        assert_eq!(r.name, "4MLinux 52 0 core iso");
        assert_eq!(r.size, None, "\"Size: 16.13 MB\" 解析不出数字");
        assert_eq!(r.size_text, "—");
        assert_eq!(r.seeders, None, "\"Seeds 7\" 解析不出数字");
        assert_eq!(r.leechers, None);
        assert_eq!(r.date, None);
        assert_eq!(r.date_text, "—");
    }

    /// 没有 torrent-details 链接的页面 → `search_at` 该报 `no_results_parsed`
    /// （这条走 `parse` 不方便，直接构造 HTML 断言 parse 为空即可）。
    #[test]
    fn page_without_any_name_link_parses_to_nothing() {
        assert!(parse(BASE, "<html><body><p>nothing here</p></body></html>").is_empty());
    }
}
