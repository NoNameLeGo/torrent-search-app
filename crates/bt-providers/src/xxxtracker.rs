//! XXXTracker（`xxxtor.com`）—— HTML 抓取（2026-10-24，第 24 家）。
//!
//! 从 `src/providers/xxxtracker.js`（97 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表 `table > tbody > tr`（fixture **101 行 = 1 行表头 + 100 条结果**，
//! 源码里 `<table width="100%">` 下**没有 tbody**，解析器会补）。**一行只有 4 个 td**
//! —— 第 2 格写的是 `colspan = "2"`（等号两边带空格，照样生效）：
//!
//! ```text
//! <tr class="gai">
//!   <td>18 Окт 26</td>
//!   <td colspan = "2"><a href="magnet:?xt=urn:btih:…"><img …></a>
//!     <a class="downgif" href="/download/119076/"><img …></a>
//!     <a href="/torrent/119076/">Honey April - … SiteRip</a></td>
//!   <td align="right">1.03&nbsp;GB</td>
//!   <td align="center"><span class="green">…7</span>&nbsp;<img …/><span class="red">3</span></td>
//! </tr>
//! ```
//!
//! ⇒ JS 的 `a:nth-child(3)` 是那个**名字/详情**锚点，`a:nth-child(1)` 是磁力。
//! 表头行（class `backgr`）靠**位置**跳过（`i === 0`），不是靠 class。
//!
//! ## ⚠️ 上游 bug：leechers 恒空（照抄，未修）
//!
//! JS 写的是 `td:nth-child(4) > span:nth-child(2)`，而第 4 格的元素子节点顺序是
//! `span.green` / `img` / `span.red` ⇒ **第 2 个子元素是 img、不是 span**，
//! 选择器永远匹配不到。cheerio 探针 `leechers.first_3` 实测 = `[]`（空数组）确认了这点。
//!
//! 这与 `zeromagnet` 的「meta 列白扔」同款：**纯显示问题先照抄**（铁律见 AGENTS.md）。
//! 要修得两版同轮改（把选择器换成 `span:nth-child(3)` 或 `.red`），
//! 修完 M6 差分仍会一致 —— 但那是另一件事，别顺手做。
//!
//! ## 日期
//!
//! 站点给俄语月份缩写（`18 Окт 26`），JS 的 `normalizeUploadDate` **只做 3 段映射**。
//! ⚠️ 不要换成 `bt_core::normalize::ru_date` —— 那个更宽（还会翻 `Сегодня` / `Вчера`、
//! 处理 `Мая` 的格变化），用在这站会在非 3 段输入上与 JS 分叉。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{
    encode_uri_component, extract_info_hash, normalize, NumOrText, RawResult,
};
use bt_core::TorrentResult;

use crate::SearchOutcome;

/// 镜像列表，与 JS 的 `DOMAINS` 一致（只有一家）。
pub const DOMAINS: &[&str] = &["https://xxxtor.com"];

/// 结果行 —— **第 0 行是表头**（class `backgr`），靠位置跳过。
const ROW_SELECTOR: &str = "table > tbody > tr";

/// 名字 + 详情链接（第 2 格里的第 3 个 `a`）。
const NAME_LINK: &str = "td:nth-child(2) > a:nth-child(3)";

/// 磁力（第 2 格里的第 1 个 `a`）。
const MAGNET_LINK: &str = "td:nth-child(2) > a:nth-child(1)";

const SIZE_CELL: &str = "td:nth-child(3)";
const SEEDERS_CELL: &str = "td:nth-child(4) > span:nth-child(1)";

/// ⚠️ **这个选择器在真实页面上永远匹配不到**（见文件头「上游 bug」）。照抄 JS。
const LEECHERS_CELL: &str = "td:nth-child(4) > span:nth-child(2)";

const DATE_CELL: &str = "td:nth-child(1)";

/// 用默认镜像搜索。`page` 与 JS 一样**收下但不用**（URL 里没有页码参数）。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。
///
/// ⚠️ JS 传的 name 是 **`'xxxtracker'`（小写）** —— 错误串必须逐字一致。
pub async fn search_with(http: &HttpClient, bases: &[&str], query: &str) -> SearchOutcome {
    crate::run_mirrors("xxxtracker", bases, |b| search_at(http, b, query)).await
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    // JS: `${base}/b.php?search=${encodeURIComponent(query)}` —— 没有页码参数
    let url = format!("{base}/b.php?search={}", encode_uri_component(query));

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::ok(Vec::new());
    };

    let dom = Dom::parse(&html);
    let rows = dom.select(ROW_SELECTOR);
    // JS: `if (rows.length <= 1) return { results: [], error: 'no_results_parsed' }`
    // —— 「只有表头」也算错，不是空结果。错误串逐字照抄（裸的，不带 `no_results`）。
    if rows.len() <= 1 {
        return SearchOutcome::err("no_results_parsed");
    }

    let results: Vec<TorrentResult> = rows
        .iter()
        .skip(1)
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    // 有行但全被过滤掉 → JS 返回 `error: null`（与 oxtorrent / therarbg 同款怪癖）
    SearchOutcome::ok(results)
}

/// 解析一行。任何一条不满足 JS 的 `return` 条件就整行跳过。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    // JS 先看名字，再要求磁力以 `magnet:` 开头（不是"有 href 就行"）
    let name_el = dom.find(row, NAME_LINK).into_iter().next()?;
    let name = text_trim(name_el);
    if name.is_empty() {
        return None;
    }

    let magnet_el = dom.find(row, MAGNET_LINK).into_iter().next()?;
    let magnet = attr(magnet_el, "href").filter(|h| h.starts_with("magnet:"))?;

    let info_hash = extract_info_hash(Some(&magnet));

    let cell_text = |sel: &str| -> Option<String> {
        dom.find(row, sel)
            .into_iter()
            .next()
            .map(|el| text_trim(el))
    };

    let size = cell_text(SIZE_CELL);
    let seeders = cell_text(SEEDERS_CELL);
    // 恒为 None（选择器匹配不到），照抄 JS —— 见文件头「上游 bug」
    let leechers = cell_text(LEECHERS_CELL);
    let date = cell_text(DATE_CELL).map(|s| normalize_upload_date(&s));

    // 详情链接与名字是**同一个 a**；相对路径拼 base（JS 的判断是 `startsWith('http')`）
    let detail_url = attr(name_el, "href").filter(|h| !h.is_empty()).map(|h| {
        if h.starts_with("http") {
            h
        } else {
            format!("{base}{h}")
        }
    });

    Some(normalize(&RawResult {
        provider: "xxxtracker".to_string(),
        name: Some(name),
        size: size.map(NumOrText::Text),
        seeders: seeders.map(NumOrText::Text),
        leechers: leechers.map(NumOrText::Text),
        date: date.map(NumOrText::Text),
        info_hash,
        magnet: Some(magnet),
        detail_url,
        // ⚠️ 硬编码 `'Porn'` —— JS 原样写死（与 zeromagnet 同），照抄
        category: Some("Porn".to_string()),
        ..Default::default()
    }))
}

/// 复刻 JS 的 `normalizeUploadDate`：**只**处理「日 月缩写 年」三段式。
///
/// 与 [`bt_core::normalize::ru_date`] 的差别是**故意的**：
/// 那个更宽（还会翻 `Сегодня`/`Вчера`、处理 `Мая`），而这一站的 JS 只做三段映射。
/// 用宽的那版会在非三段输入（如纯 `Сегодня`）上与 JS 分叉。
fn normalize_upload_date(s: &str) -> String {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 3 {
        return s.to_string();
    }
    let key = parts[1].to_lowercase();
    let eng = match key.as_str() {
        "янв" => "Jan",
        "фев" => "Feb",
        "мар" => "Mar",
        "апр" => "Apr",
        "май" => "May",
        "июн" => "Jun",
        "июл" => "Jul",
        "авг" => "Aug",
        "сен" => "Sep",
        "окт" => "Oct",
        "ноя" => "Nov",
        "дек" => "Dec",
        // JS: `MONTH_MAP[parts[1].toLowerCase()] || parts[1]` —— 认不出就原样
        _ => parts[1],
    };
    format!("{} {} {}", parts[0], eng, parts[2])
}

/// 直接拿一段 HTML 解析（测试与将来的复用入口）。
pub fn parse(base: &str, html: &str) -> Vec<TorrentResult> {
    let dom = Dom::parse(html);
    let rows = dom.select(ROW_SELECTOR);
    if rows.len() <= 1 {
        return Vec::new();
    }
    rows.iter()
        .skip(1)
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://xxxtor.com";

    /// 造一行：`td2` 里三个 `a`（磁力 / 下载 / 名字），`td4` 里 span-img-span。
    fn row(date: &str, size: &str, seeds: &str, peers: &str, name: &str, id: &str) -> String {
        format!(
            "<tr class=\"gai\"><td>{date}</td>\
             <td colspan = \"2\">\
               <a href=\"magnet:?xt=urn:btih:{id}&amp;dn=x\"><img src=\"/s/magnet.gif\" /></a> \
               <a class=\"downgif\" href=\"/download/1/\"><img src=\"/s/down.gif\" /></a>\
               <a href=\"/torrent/1/\">{name}</a></td>\
             <td align=\"right\">{size}</td>\
             <td align=\"center\"><span class=\"green\">{seeds}</span>&nbsp;\
               <img src=\"/s/arrowdown.gif\" /><span class=\"red\">{peers}</span></td></tr>"
        )
    }

    fn header() -> String {
        "<tr class=\"backgr\"><td>Добавлен</td><td colspan=\"2\">Название</td>\
         <td>Размер</td><td>Пиры</td></tr>"
            .to_string()
    }

    fn page(rows: &str) -> String {
        format!("<html><body><table width=\"100%\">{rows}</table></body></html>")
    }

    #[test]
    fn parses_a_row_end_to_end() {
        // 1.03 + **NBSP**(U+00A0) + GB —— 真实页面就是 NBSP，别写成普通空格
        let html = page(&format!(
            "{}{}",
            header(),
            row(
                "18 Окт 26",
                "1.03\u{a0}GB",
                "7",
                "3",
                "Honey April - Honeys First Creampie",
                "ec3ba0e839485f06fdff4d694230fa198bb28b16"
            )
        ));
        let out = parse(BASE, &html);

        assert_eq!(out.len(), 1, "表头必须被跳过");
        let r = &out[0];
        assert_eq!(r.provider, "xxxtracker");
        assert_eq!(r.name, "Honey April - Honeys First Creampie");
        assert_eq!(r.id, "xxxtracker:ec3ba0e839485f06fdff4d694230fa198bb28b16");
        assert_eq!(
            r.detail_url.as_deref(),
            Some("https://xxxtor.com/torrent/1/")
        );
        assert_eq!(r.category.as_deref(), Some("Porn"));
        assert_eq!(
            r.info_hash.as_deref(),
            Some("ec3ba0e839485f06fdff4d694230fa198bb28b16")
        );
        assert!(r.magnet.as_deref().unwrap_or("").starts_with("magnet:"));
        assert!(!r.needs_magnet, "搜索阶段就带磁力");

        // 期望值由真 JS 跑出来（`normalize.js` + `parseSize` 认 NBSP）：
        //   "1.03\u{a0}GB" → 1105954079 字节 → "1.0 GB"
        assert_eq!(r.size, Some(1_105_954_079));
        assert_eq!(r.size_text, "1.0 GB");
        assert_eq!(r.seeders, Some(7));
        // ⚠️ leechers 恒 None —— `span:nth-child(2)` 匹配不到（上游 bug，照抄）
        assert_eq!(r.leechers, None);
        // "18 Окт 26" → "18 Oct 26" → 本地时区当天零点；这里只钉住「有值 + 日期文本」
        assert!(r.date.is_some(), "俄语月份必须被翻译后解析出来");
        assert!(!r.date_text.is_empty());
    }

    #[test]
    fn rows_without_magnet_or_name_are_skipped() {
        // 名字在、但磁力锚点不是 magnet:（这是 JS 的硬条件）
        let no_magnet = "<tr><td>18 Окт 26</td>\
             <td colspan=\"2\"><a href=\"/s/x.gif\"><img /></a>\
               <a href=\"/download/2/\">d</a><a href=\"/torrent/2/\">名字在</a></td>\
             <td>1 MB</td><td><span class=\"green\">1</span></td></tr>";
        // 名字是空的
        let no_name = "<tr><td>18 Окт 26</td>\
             <td colspan=\"2\"><a href=\"magnet:?xt=urn:btih:aaaa\"></a>\
               <a href=\"/download/3/\">d</a><a href=\"/torrent/3/\">  </a></td>\
             <td>1 MB</td><td><span class=\"green\">1</span></td></tr>";
        let ok = row("18 Окт 26", "1 MB", "2", "1", "ok", "bbbb");

        let out = parse(
            BASE,
            &page(&format!("{}{}{}{}", header(), no_magnet, no_name, ok)),
        );
        assert_eq!(out.len(), 1, "两条坏行必须被跳过");
        assert_eq!(out[0].name, "ok");
    }

    #[test]
    fn only_header_means_an_error_not_an_empty_list() {
        // JS: `rows.length <= 1` → error 'no_results_parsed'（不是空结果）
        let dom = Dom::parse(&page(&header()));
        assert_eq!(dom.select(ROW_SELECTOR).len(), 1);
        assert!(parse(BASE, &page(&header())).is_empty());
    }

    /// 日期映射只认三段式，且**不**处理 `Сегодня`（那是 `ru_date` 的活）。
    #[test]
    fn upload_date_only_handles_three_part_dates() {
        assert_eq!(normalize_upload_date("18 Окт 26"), "18 Oct 26");
        assert_eq!(normalize_upload_date("01 Янв 2026"), "01 Jan 2026");
        // 认不出的月份原样保留（JS: `|| parts[1]`）
        assert_eq!(normalize_upload_date("18 Foo 26"), "18 Foo 26");
        // 少于三段 → 原样返回（JS 的 `return s`）
        assert_eq!(normalize_upload_date("Сегодня"), "Сегодня");
        assert_eq!(normalize_upload_date(""), "");
        // 多段：只取前三个（JS 用 parts[0..2]）
        assert_eq!(normalize_upload_date("18 Окт 26 лишнее"), "18 Oct 26");
    }

    /// 相对路径拼 base；绝对路径原样保留（JS: `startsWith('http')`）。
    #[test]
    fn absolute_detail_urls_are_kept_verbatim() {
        let absolute = "<tr><td>18 Окт 26</td>\
             <td colspan=\"2\"><a href=\"magnet:?xt=urn:btih:cccc\"></a>\
               <a href=\"/d/\">d</a><a href=\"https://mirror.example/t/9/\">abs</a></td>\
             <td>1 MB</td><td><span class=\"green\">1</span></td></tr>";
        let out = parse(BASE, &page(&format!("{}{}", header(), absolute)));
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].detail_url.as_deref(),
            Some("https://mirror.example/t/9/")
        );
    }
}
