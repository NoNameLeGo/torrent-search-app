//! DMHY（`share.dmhy.org`，中文番剧/字幕组）—— HTML 抓取。
//!
//! 从 `src/providers/dmhy.js`（92 行）移植。
//!
//! ## 行结构（真 fixture 实测，`test/fixtures/dmhy-one-piece.html`）
//!
//! ```text
//! <tr>
//!   <td>2026/09/28 16:47<span style="display:none">2026/09/28 16:47</span></td>   ← 1 日期
//!   <td><a class="sort-2" href="/topics/list/sort_id/2"><font>動畫</font></a></td> ← 2 分类
//!   <td class="title">
//!       <span class="tag"><a href="/topics/list/team_id/767">天月動漫&發佈組</a></span>
//!       <a href="/topics/view/727909_….html">[Skymoon-Raws][<span class="keyword">One</span> …</a>
//!       <span style="color:gray">約2條評論</span>
//!   </td>                                                                          ← 3 标题
//!   <td><a class="download-arrow" href="magnet:?…">…</a> …</td>                     ← 4 磁链
//!   <td>457.9MB</td>  <td>11</td>  <td>0</td>  <td>721</td>                        ← 5/6/7/8
//! </tr>
//! ```
//!
//! ## 三处必须照抄的细节（都是 JS 的既有行为，不是这里的选择）
//!
//! 1. **名称用 `ownText`**：标题里的 `<span class="keyword">` 是高亮的关键词，
//!    `ownText` 只取**直接子文本节点** ⇒ 名字里**不含**被高亮的那几个词
//!    （`[Skymoon-Raws][ 海賊王][1180]…`，中间多一个空格）。
//!    用 `.text()` 拼全反而会与 JS 不一致。
//! 2. **名称链接必须是 `td.title > a`**（**直接子**）。`td.title` 里第一个 `<a>`
//!    其实是 `<span class="tag">` 里的**字幕组**链接；只写后代选择器会挑错。
//! 3. **磁链取 `td:nth-child(4) > a:nth-child(1)`**：同格后面还有迅雷/PikPak 两个
//!    `<a>`，第一个才是真磁力（`class="download-arrow arrow-magnet"`）。
//!
//! 分类是**数字 id**（`class="sort-2"` → `2`），不是文字 —— 按 JS 的 `CATEGORY_MAP` 映射。

use bt_core::dom::{attr, own_text, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{
    encode_uri_component, extract_info_hash, normalize, NumOrText, RawResult, TorrentResult,
};

use crate::SearchOutcome;

/// 站点根（JS 的 `BASE`）。
pub const BASE: &str = "https://share.dmhy.org";

/// 唯一镜像。JS 里 `DOMAINS = [BASE]`，仍走 `runMirrors`（照抄）。
const DOMAINS: &[&str] = &[BASE];

/// 结果表：`table#topic_list > tbody > tr`。
const ROWS: &str = "table#topic_list > tbody > tr";

/// 名称链接 —— **直接子** `<a>`，跳过 `<span class="tag">` 里的字幕组链接。
const NAME_LINK: &str = "td.title > a";

/// 磁力：第 4 格里的**第 1 个** `<a>`。
const MAGNET_LINK: &str = "td:nth-child(4) > a:nth-child(1)";

const CATEGORY_LINK: &str = "td:nth-child(2) > a";
const DATE_SPAN: &str = "td:nth-child(1) > span";
const SIZE_CELL: &str = "td:nth-child(5)";
const SEEDERS_CELL: &str = "td:nth-child(6)";
const LEECHERS_CELL: &str = "td:nth-child(7)";

/// 站点分类 id → 项目标准分类。逐字照抄 JS 的 `CATEGORY_MAP`（未命中 ⇒ `Other`）。
fn category_from_id(id: &str) -> String {
    match id {
        "2" | "7" | "31" => "Anime",
        "3" => "Books",
        "6" | "41" | "42" => "Series",
        "4" | "15" | "43" | "44" => "Music",
        "9" | "17" | "18" | "19" | "20" | "21" => "Games",
        _ => "Other",
    }
    .to_string()
}

/// 用默认镜像搜索。`page` 直接进 URL（JS 也是）。
pub async fn search(http: &HttpClient, query: &str, page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query, page).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。JS 传的 name 是小写 `'dmhy'`。
pub async fn search_with(
    http: &HttpClient,
    bases: &[&str],
    query: &str,
    page: u32,
) -> SearchOutcome {
    crate::run_mirrors("dmhy", bases, |b| search_at(http, b, query, page)).await
}

/// `?keyword=<q>&sort_id=0&team_id=0&order=date-desc&page=<n>`
fn search_url(base: &str, query: &str, page: u32) -> String {
    format!(
        "{base}/topics/list?keyword={}&sort_id=0&team_id=0&order=date-desc&page={page}",
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
        return SearchOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    let rows = dom.select(ROWS);
    // JS: `if (rows.length === 0) return { results: [], error: null }`
    // —— 没有结果表不是错误（搜索词没命中时站上就是这么回）
    if rows.is_empty() {
        return SearchOutcome::ok(Vec::new());
    }

    let results = rows
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    SearchOutcome::ok(results)
}

/// 解析一行；缺名称或缺磁力就整行丢掉（JS 的 `return null`）。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let name_link = dom.find(row, NAME_LINK).into_iter().next()?;
    // ⚠️ ownText，不是 text —— 见文件头第 1 条
    let name = own_text(name_link);
    if name.is_empty() {
        return None;
    }

    let magnet = dom
        .find(row, MAGNET_LINK)
        .into_iter()
        .next()
        .and_then(|a| attr(a, "href"))
        .filter(|h| !h.is_empty())?;

    let size = own_text_cell(dom, row, SIZE_CELL);
    let seeders = text_cell(dom, row, SEEDERS_CELL);
    let leechers = text_cell(dom, row, LEECHERS_CELL);
    let date = own_text_cell(dom, row, DATE_SPAN);

    // JS: `($row.find('td:nth-child(2) > a').attr('class') || '').replace('sort-','').trim()`
    let cat_id = dom
        .find(row, CATEGORY_LINK)
        .into_iter()
        .next()
        .and_then(|a| attr(a, "class"))
        .unwrap_or_default()
        // JS 是 `.replace('sort-','')`（只换第一处），这里同口径
        .replacen("sort-", "", 1);
    let category = category_from_id(cat_id.trim());

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
        provider: "dmhy".to_string(),
        name: Some(name),
        // JS 是 `size || null` —— 空串当没有
        size: non_empty_nt(size),
        // JS 是 `seeders || null`
        seeders: non_empty_nt(seeders),
        leechers: non_empty_nt(leechers),
        date: non_empty_nt(date),
        category: Some(category),
        // JS 直接把磁力原串塞进去（含全部 tracker），不重组
        magnet: Some(magnet.clone()),
        info_hash: extract_info_hash(Some(&magnet)),
        detail_url,
        ..Default::default()
    }))
}

/// 取格子的 `ownText`（JS 的 `ownText($row.find(cell))`）。
fn own_text_cell(dom: &Dom, row: ElementRef<'_>, cell: &str) -> String {
    dom.find(row, cell)
        .into_iter()
        .next()
        .map(own_text)
        .unwrap_or_default()
}

/// 取格子的 `.text().trim()`（JS 的 `$row.find(cell).text().trim()`）。
fn text_cell(dom: &Dom, row: ElementRef<'_>, cell: &str) -> String {
    dom.find(row, cell)
        .into_iter()
        .next()
        .map(text_trim)
        .unwrap_or_default()
}

/// 空串 → `None`（对齐 JS 的 `x || null`）。
fn non_empty_nt(s: String) -> Option<NumOrText> {
    if s.trim().is_empty() {
        None
    } else {
        Some(NumOrText::Text(s))
    }
}

#[cfg(test)]
mod tests {
    use super::category_from_id;

    #[test]
    fn category_ids_follow_the_js_map() {
        assert_eq!(category_from_id("2"), "Anime");
        assert_eq!(category_from_id("31"), "Anime");
        assert_eq!(category_from_id("3"), "Books");
        assert_eq!(category_from_id("41"), "Series");
        assert_eq!(category_from_id("44"), "Music");
        assert_eq!(category_from_id("21"), "Games");
        // 未命中 ⇒ Other（JS 的 `|| 'Other'`）
        assert_eq!(category_from_id("999"), "Other");
        assert_eq!(category_from_id(""), "Other");
    }
}
