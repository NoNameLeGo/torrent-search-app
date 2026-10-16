//! NYAA —— 动漫 / 亚洲种子的 HTML 抓取（B 组第 9 个，也是**第一个 fixture 由 CI 抓的**）。
//!
//! 从 `src/providers/nyaa.js`（64 行）移植。
//!
//! ## 为什么 fixture 是 CI 抓的
//!
//! `nyaa.si` 在**本机探不通**（代理 + 站点封锁），而 CI 的干净机房 IP 能通。
//! 于是新增了 `.github/workflows/fetch-fixture.yml`：手动触发、带上 url + path，
//! CI 抓完自己 commit 回分支（与 `html-probes.yml` 同一套路）。
//! 这也是「本机能抓的都搬完了」之后继续往下搬的标准动作。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 结果表 `table.torrent-list tbody tr`（fixture 里 **1 行**），表头是 `<thead>` 里的，
//! 不参与选择器。⚠️ **名字那格带 `colspan="2"`**（评论列被合并），所以一行只有
//! **8 个 td**，而 JS 是照 9 列的布局写的：
//!
//! ```text
//! [0] 分类   <a href="/?c=6_1" title="Software - Applications">
//! [1] 名字   <a href="/view/96659" title="…">Koha Live CD Release 3 (3.0.4 Ubuntu 9.10…)</a>
//! [2] Link   <a href="/download/96659.torrent">…</a> + <a href="magnet:?xt=urn:btih:45008e…">
//! [3] 624.0 MiB
//! [4] 2009-11-03 07:03
//! [5] 0   ← 真 seeders
//! [6] 0   ← 真 leechers
//! [7] 0   ← downloads
//! ```
//!
//! ## ⚠️ 上游 bug（照抄，未修）—— 这一站一次踩了三个
//!
//! 1. **列错位**：JS 读 `tds.eq(2)/eq(3)/eq(4)/eq(5)`，以为它们是 Size/Date/Seeders/Leechers，
//!    但 `colspan` 把评论列合掉之后它们实际是 **Link/Size/Date/Seeders**：
//!    - `size` 拿到 **Link 格（空串）** → `sizeText: "—"`
//!    - `date` 拿到 **`"624.0 MiB"`** → `parseDate` 解析不出 → `dateText: "—"`
//!    - `seeders` 拿到 **`"2009-11-03 07:03"`** → `parseInt` 给出 **2009**（实测 JS 就是这个数）
//!    - `leechers` 拿到的是**真 seeders**（这一行两格都是 0，所以没露馅）
//! 2. **磁力找错地方**：`nameCell.find('a[href^="magnet:"]')` 在 `tds.eq(1)`（名字格）里找，
//!    而磁力在 `tds.eq(2)`（Link 格）里（fixture 里实测全页只有那一条）→ **magnet 恒为 null**、
//!    `needsMagnet` 恒为 true。
//! 3. 由 2 派生：`nyaa.js` **没有 `resolveMagnet`**，于是前端那个「获取磁力」按钮会打到
//!    `/api/magnet` → 该 provider 无 resolver → 回退到 1337x 的（已死）→ 报错。
//!    **也就是说 nyaa 的结果目前拿不到磁力。**
//!
//! 三条都照抄（阶段一的验收点是「与 Node 版逐字段一致」）；测试里把「seeders=2009」这种
//! 荒诞值也钉住了，免得以后有人以为是 Rust 侧写错。
//!
//! ## 分页
//!
//! URL 里有 `page`，而且 **JS 没过 `coercePage`** —— 传 0 就是 `page=0`，照抄。

use bt_core::dom::{attr, text, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{
    encode_uri_component, extract_info_hash, normalize, NumOrText, RawResult,
};
use bt_core::TorrentResult;

use crate::SearchOutcome;

/// 唯一域名（JS 里没有 `DOMAINS` 数组，也没有 `runMirrors`）。
pub const BASE: &str = "https://nyaa.si";

/// 结果行。
const ROWS: &str = "table.torrent-list tbody tr";

/// 名字格里的详情页链接。
const VIEW_LINK: &str = r#"a[href^="/view/"]"#;

/// 名字格里的磁力（⚠️ JS 就在这里找，所以恒找不到 —— 真磁力在 Link 格里）。
const MAGNET_LINK: &str = r#"a[href^="magnet:"]"#;

/// 用默认域名搜索。`page` **原样进 URL**（JS 没做 `coercePage`）。
pub async fn search(http: &HttpClient, query: &str, page: u32) -> SearchOutcome {
    search_at(http, BASE, query, page).await
}

/// `/` + 固定查询参数 + `page`（**不做 `coercePage`**，与 JS 一致）。
fn search_url(base: &str, query: &str, page: u32) -> String {
    format!(
        "{base}/?f=0&c=0_0&q={}&page={page}",
        encode_uri_component(query)
    )
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
///
/// ⚠️ JS 里详情链接拼的是模块级 `BASE`（不是参数），本移植改用传入的 `base`
/// —— 生产环境两者相同（nyaa 没有镜像），测试里才需要指向本地服务。
pub async fn search_at(http: &HttpClient, base: &str, query: &str, page: u32) -> SearchOutcome {
    let url = search_url(base, query, page);

    let resp = http.get_text(&url, None).await;
    if resp.error.is_some() {
        // JS: `error: \`NYAA unreachable (${error})\``
        let e = resp.error.unwrap_or_default();
        return SearchOutcome::err(format!("NYAA unreachable ({e})"));
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        // 正文为空、但接口没报错 —— JS 的模板会把 `undefined` **原样变成字面量**，照抄
        return SearchOutcome::err("NYAA unreachable (undefined)");
    };

    let dom = Dom::parse(&html);
    // JS: `if (rows.length === 0) return { results: [] }` —— **不是错误**
    if dom.select(ROWS).is_empty() {
        return SearchOutcome::ok(Vec::new());
    }

    let results: Vec<TorrentResult> = dom
        .select(ROWS)
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    SearchOutcome::ok(results)
}

/// 解析一行。所有索引都**逐字照抄 JS**（含那三处 bug，见文件头）。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let tds = dom.find(row, "td");
    // JS: `if (tds.length < 6) return;`
    if tds.len() < 6 {
        return None;
    }

    let name_cell = tds.get(1).copied()?;

    let view = dom.find(name_cell, VIEW_LINK).into_iter().next();
    // JS: `(($view.text() || nameCell.text())).trim()`
    let name = {
        let from_view = view.map(text_trim).unwrap_or_default();
        if from_view.is_empty() {
            text_trim(name_cell)
        } else {
            from_view
        }
    };
    if name.is_empty() {
        return None;
    }

    // 分类：先找 `a[href*=c=]` 的 title，再退到格子里第一个 `<a>` 的 title（JS 的 `||`）
    let category = tds
        .first()
        .and_then(|cat_cell| {
            let titled = dom
                .find(*cat_cell, r#"a[href*="c="]"#)
                .into_iter()
                .next()
                .and_then(|a| attr(a, "title"));
            titled.filter(|t| !t.is_empty()).or_else(|| {
                dom.find(*cat_cell, "a")
                    .into_iter()
                    .next()
                    .and_then(|a| attr(a, "title"))
                    .filter(|t| !t.is_empty())
            })
        })
        .unwrap_or_default();

    // ⚠️ JS 在**名字格**里找磁力 —— 实测那里没有（磁力在 Link 格）→ 恒为 None。
    // 照抄，别"顺手修好"，修了就和 Node 版对不上。
    let magnet = dom
        .find(name_cell, MAGNET_LINK)
        .into_iter()
        .next()
        .and_then(|a| attr(a, "href"))
        .filter(|h| !h.is_empty());

    let detail_url = view
        .and_then(|a| attr(a, "href"))
        .filter(|h| !h.is_empty())
        .map(|href| format!("{base}{href}"));

    // ⚠️ 下面四个索引是 JS 的 `tds.eq(2..5)`，因为 `colspan` 而**全部错位**（见文件头）
    let size = cell_text(&tds, 2);
    let date = cell_text(&tds, 3);
    let seeders = cell_text(&tds, 4);
    let leechers = cell_text(&tds, 5);

    Some(normalize(&RawResult {
        provider: "nyaa".to_string(),
        name: Some(name),
        size: Some(NumOrText::Text(size)),
        date: Some(NumOrText::Text(date)),
        seeders: Some(NumOrText::Text(seeders)),
        leechers: Some(NumOrText::Text(leechers)),
        magnet: magnet.clone(),
        info_hash: extract_info_hash(magnet.as_deref()),
        detail_url,
        category: Some(category),
        ..Default::default()
    }))
}

/// 取第 `i` 个 td 的文本（越界给空串，等价于 jQuery 空集合上的 `.text()`）。
fn cell_text(tds: &[ElementRef<'_>], i: usize) -> String {
    tds.get(i).map(|e| text(*e)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_URL: &str = "https://nyaa.si";

    /// 造一行：真实 fixture 的 8 格布局（名字格 `colspan="2"`）。
    fn row(
        name: &str,
        view: &str,
        magnet: Option<&str>,
        size: &str,
        date: &str,
        seeds: &str,
        leech: &str,
    ) -> String {
        let magnet_link = match magnet {
            Some(m) => format!("<a href=\"{m}\"><i class=\"fa fa-magnet\"></i></a>"),
            None => String::new(),
        };
        format!(
            r##"<tr class="default">
                 <td><a href="/?c=6_1" title="Software - Applications"><img src="/i.png" alt="x"></a></td>
                 <td colspan="2"><a href="{view}" title="{name}">{name}</a></td>
                 <td><a href="/download/1.torrent"><i class="fa fa-download"></i></a>{magnet_link}</td>
                 <td>{size}</td>
                 <td>{date}</td>
                 <td>{seeds}</td>
                 <td>{leech}</td>
                 <td>0</td>
               </tr>"##
        )
    }

    fn page(rows: &str) -> String {
        format!(
            "<html><body><table class=\"table torrent-list\"><thead><tr><th>Category</th></tr></thead>\
             <tbody>{rows}</tbody></table></body></html>"
        )
    }

    fn parse_page(rows: &str) -> Vec<TorrentResult> {
        let dom = Dom::parse(&page(rows));
        dom.select(ROWS)
            .iter()
            .filter_map(|r| parse_row(&dom, *r, BASE_URL))
            .collect()
    }

    const MAGNET: &str = "magnet:?xt=urn:btih:45008e48c8800b7d7643337b2e70a634e4c69f6a&dn=x";

    #[test]
    fn parses_a_row_with_the_same_off_by_two_indices_as_the_js() {
        // 名字格里**故意不放**磁力（和真实页面一样），磁力放在 Link 格
        let rows = row(
            "Koha Live CD",
            "/view/96659",
            None,
            "624.0 MiB",
            "2009-11-03 07:03",
            "0",
            "0",
        );
        let out = parse_page(&rows);

        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.provider, "nyaa");
        assert_eq!(r.name, "Koha Live CD");
        assert_eq!(r.detail_url.as_deref(), Some("https://nyaa.si/view/96659"));
        assert_eq!(r.category.as_deref(), Some("Software - Applications"));

        // ⚠️ 列错位的三个后果（照抄 JS）
        assert_eq!(r.size, None, "读到的是空串的 Link 格");
        assert_eq!(r.size_text, "—");
        assert_eq!(r.date, None, "读到的是 \"624.0 MiB\"");
        assert_eq!(r.date_text, "—");
        assert_eq!(r.seeders, Some(2009), "读到的是日期串 \"2009-11-03 07:03\"");
        assert_eq!(r.leechers, Some(0), "读到的其实是真 seeders（这行也是 0）");

        // ⚠️ 磁力在名字格找不到 → 恒 null
        assert_eq!(r.magnet, None);
        assert_eq!(r.info_hash, None);
        assert!(r.needs_magnet);
    }

    /// 磁力即使**就放在名字格里**（真实页面不是这样），JS 的索引错位也照样影响别的字段。
    #[test]
    fn a_magnet_in_the_name_cell_would_be_found() {
        let rows = format!(
            r##"<tr><td><a href="/?c=6_1" title="Anime">c</a></td>
                 <td colspan="2"><a href="/view/1" title="n">n</a>
                   <a href="{MAGNET}"><i class="fa fa-magnet"></i></a></td>
                 <td></td><td>1 MiB</td><td>2020-01-01</td><td>7</td><td>1</td><td>0</td></tr>"##
        );
        let out = parse_page(&rows);

        assert_eq!(out[0].magnet.as_deref(), Some(MAGNET));
        assert_eq!(
            out[0].info_hash.as_deref(),
            Some("45008e48c8800b7d7643337b2e70a634e4c69f6a")
        );
        assert!(!out[0].needs_magnet, "有磁力就不需要惰性解析");
    }

    #[test]
    fn rows_with_fewer_than_six_cells_are_skipped() {
        let rows = "<tr><td>a</td><td>b</td><td>c</td></tr>";
        assert!(parse_page(rows).is_empty(), "JS: tds.length < 6 → return");
    }

    #[test]
    fn name_falls_back_to_the_whole_cell_when_the_view_link_has_no_text() {
        let rows = format!(
            r##"<tr><td><a href="/?c=6_1" title="Anime">c</a></td>
                 <td colspan="2"><a href="/view/1" title="x"></a>Text from the cell</td>
                 <td></td><td>1 MiB</td><td>2020-01-01</td><td>1</td><td>0</td><td>0</td></tr>"##
        );
        let out = parse_page(&rows);

        assert_eq!(
            out[0].name, "Text from the cell",
            "JS 的 `|| nameCell.text()`"
        );
    }

    #[test]
    fn category_falls_back_to_the_first_anchor_title() {
        // 没有 `a[href*=c=]` 时退到第一个 <a> 的 title（JS 的 `||`）
        let rows = r##"<tr><td><a href="/x" title="Anime - English-translated">i</a></td>
             <td colspan="2"><a href="/view/1" title="n">n</a></td>
             <td></td><td>1 MiB</td><td>2020-01-01</td><td>1</td><td>0</td><td>0</td></tr>"##;
        let out = parse_page(rows);

        assert_eq!(
            out[0].category.as_deref(),
            Some("Anime - English-translated")
        );
    }

    /// URL 里的 `page` 是**原样拼进去**的（JS 没过 `coercePage`）—— 0 就是 0。
    #[test]
    fn the_page_number_is_not_coerced() {
        assert_eq!(
            search_url(BASE_URL, "ubuntu", 0),
            "https://nyaa.si/?f=0&c=0_0&q=ubuntu&page=0"
        );
        assert_eq!(
            search_url(BASE_URL, "ubuntu 22", 7),
            "https://nyaa.si/?f=0&c=0_0&q=ubuntu%2022&page=7"
        );
    }
}
