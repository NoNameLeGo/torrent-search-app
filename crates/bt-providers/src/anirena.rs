//! AniRena（`anirena.com`，动漫/漫画/同人）—— HTML 抓取。
//!
//! 从 `src/providers/anirena.js`（123 行）移植。
//!
//! ## 磁力是**惰性**的（搜索不碰它）
//!
//! 站上的磁力链接是个**会 302 到 `magnet:` 的地址**，JS 在 `resolveMagnet()` 里
//! 手动关掉 axios 的自动跳转、读 `Location` 头（[`follow_to_magnet`]）。
//! 搜索阶段完全不请求它 ⇒ 每条结果 `magnet: null` + `needsMagnet: true`。
//!
//! ## 页面结构（真 fixture 实测，`test/fixtures/anirena-one-piece.html`）
//!
//! ```text
//! <tr data-created-ts=1790543557>            ← 属性**不带引号**（scraper 照样解析）
//!   <td class="col-name"><div class="tl-name-wrap"><a class="tl-torrent-name" href="…">
//!   <td class="col-actions"><div class="tl-actions"><a>…（源站链接，缺了就跳过整行）
//!   <td class="col-size">…   <td class="col-se"><span class="tl-se">…  <td class="col-le"><span class="tl-le">
//!   <td class="col-cat" title="Anime/…">   ← 分类在 **title** 上，取 `/` 前那段再映射
//! ```
//!
//! ## 两处照抄的细节
//!
//! 1. **日期来自行属性** `data-created-ts`（unix **秒**），不是格子文本；
//!    非纯数字就当没有（JS 的 `/^\d+$/`）。
//! 2. **`fileDownloadLink` 丢弃** —— Rust 的 `TorrentResult` 没有这个字段（同 mikan / animetosho）。
//!
//! ## 测试强度的说明
//!
//! 本机没有 cheerio ⇒ 算不出"应几行"，所以只断言**非空 + 每行结构完整**；
//! 精确的逐字段一致性交给 M6 差分门（否则选择器写错会「两边都 0 行」假通过）。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, normalize, NumOrText, RawResult, TorrentResult};

use crate::{MagnetOutcome, SearchOutcome};

/// 站点根（JS 的 `BASE`）。
pub const BASE: &str = "https://anirena.com";

/// 唯一镜像。JS 里 `DOMAINS = [BASE]`，仍走 `runMirrors`（照抄）。
const DOMAINS: &[&str] = &[BASE];

const ROWS: &str = "table.tl-table > tbody > tr";
const NAME_LINK: &str = "td.col-name > div.tl-name-wrap > a.tl-torrent-name";
/// 源站链接（缺了整行跳过）。
const SOURCE_LINK: &str = "td.col-actions > div.tl-actions > a:nth-child(1)";
const SIZE_CELL: &str = "td.col-size";
const SEEDERS_CELL: &str = "td.col-se > span.tl-se";
const LEECHERS_CELL: &str = "td.col-le > span.tl-le";
const CATEGORY_CELL: &str = "td.col-cat";

/// 站点分类（`title` 里 `/` 之前那段）→ 项目标准分类。逐字照抄 JS 的 `CATEGORY_MAP`。
fn category_from_raw(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let key = raw.split('/').next().unwrap_or("").trim();
    Some(
        match key {
            "Anime" => "Anime",
            "Manga" | "Literature" => "Books",
            "Audio" => "Music",
            "Live Action" => "Series",
            "Software" => "Apps",
            "Hentai" => "Porn",
            "Other" => "Other",
            _ => "Other",
        }
        .to_string(),
    )
}

/// 用默认镜像搜索。`page` 与 JS 一样**被忽略**（URL 里写死 `page=1`）。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。JS 传的 name 是小写 `'anirena'`。
pub async fn search_with(http: &HttpClient, bases: &[&str], query: &str) -> SearchOutcome {
    crate::run_mirrors("anirena", bases, |b| search_at(http, b, query)).await
}

/// `/?q=<q>&page=1`
fn search_url(base: &str, query: &str) -> String {
    format!("{base}/?q={}&page=1", encode_uri_component(query))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    let url = search_url(base, query);

    let resp = http.get_text(&url, None).await;
    // JS: `if (error || !html) return { results: [], error: error || 'no_html' }`
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::err("no_html");
    };

    let dom = Dom::parse(&html);
    let rows = dom.select(ROWS);
    if rows.is_empty() {
        return SearchOutcome::ok(Vec::new());
    }

    let results = rows
        .iter()
        .filter_map(|row| parse_row(&dom, *row, base))
        .collect();

    SearchOutcome::ok(results)
}

/// 解析一行；缺名字或缺源站链接就丢掉（JS 的两个 `return`）。
fn parse_row(dom: &Dom, row: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let name_link = dom.find(row, NAME_LINK).into_iter().next()?;
    let name = text_trim(name_link);
    if name.is_empty() {
        return None;
    }

    // 源站链接是**必需**的（JS 在取不到时整行跳过）
    dom.find(row, SOURCE_LINK)
        .into_iter()
        .next()
        .and_then(|a| attr(a, "href"))
        .filter(|h| !h.is_empty())?;

    let size = cell_text(dom, row, SIZE_CELL);
    let seeders = cell_text(dom, row, SEEDERS_CELL);
    let leechers = cell_text(dom, row, LEECHERS_CELL);

    // 日期：行属性 `data-created-ts`（unix 秒）；非纯数字当没有
    let date = attr(row, "data-created-ts")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        .and_then(|s| s.parse::<i64>().ok())
        .map(NumOrText::Num);

    let category = category_from_raw(
        dom.find(row, CATEGORY_CELL)
            .into_iter()
            .next()
            .and_then(|c| attr(c, "title"))
            .as_deref(),
    );

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
        provider: "anirena".to_string(),
        name: Some(name),
        // JS: `size: size || null` —— 空串当没有
        size: non_empty_nt(size),
        seeders: non_empty_nt(seeders),
        leechers: non_empty_nt(leechers),
        date,
        category,
        detail_url,
        // 磁力惰性：搜索阶段不请求（JS 也是 `needsMagnet: true`）
        magnet: None,
        ..Default::default()
    }))
}

/// 取格子的 `.text().trim()`。
fn cell_text(dom: &Dom, row: ElementRef<'_>, cell: &str) -> String {
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

/// 惰性解析磁力。对应 JS 的 `resolveMagnet(detailUrl)`：
/// 详情页里 `div.td-actions > a:nth-child(1)` 的 href 是个**会跳转**的地址，
/// 手动跟一次、读 `Location`（[`follow_to_magnet`]）。
pub async fn resolve_magnet(http: &HttpClient, detail_url: &str) -> MagnetOutcome {
    let resp = http.get_text(detail_url, None).await;
    if let Some(e) = resp.error {
        return MagnetOutcome {
            magnet: None,
            info_hash: None,
            error: Some(e),
        };
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return MagnetOutcome {
            magnet: None,
            info_hash: None,
            error: Some("no_html".to_string()),
        };
    };

    let dom = Dom::parse(&html);
    let Some(href) = dom
        .select("div.td-actions > a:nth-child(1)")
        .into_iter()
        .next()
        .and_then(|a| attr(a, "href"))
        .filter(|h| !h.is_empty())
    else {
        return MagnetOutcome {
            magnet: None,
            info_hash: None,
            error: Some("no_magnet_link".to_string()),
        };
    };

    let abs = if href.starts_with("http") {
        href
    } else {
        format!("{BASE}{href}")
    };

    match follow_to_magnet(&abs).await {
        Some(m) => MagnetOutcome {
            info_hash: bt_core::normalize::extract_info_hash(Some(&m)),
            magnet: Some(m),
            error: None,
        },
        None => MagnetOutcome {
            magnet: None,
            info_hash: None,
            error: Some("no_magnet".to_string()),
        },
    }
}

/// 跟一次跳转，从 `Location` 里读 `magnet:`（对应 JS 的 `followToMagnet`）。
///
/// ⚠️ 必须**关掉自动跳转**（`ReqOpts::no_redirect()`），否则 reqwest 会跟着跳到
/// `magnet:` 然后报「不支持的协议」—— 那正是 JS 用 `maxRedirects: 0` 避开的事。
async fn follow_to_magnet(source_url: &str) -> Option<String> {
    // ⚠️ reqwest 的跳转策略只能设在 Client 上，所以这里用一个「不跟随跳转」的实例
    let no_redirect = HttpClient::without_redirects();
    let resp = no_redirect.get_text(source_url, None).await;
    // 3xx 时 `error` 也是 Some，但 `location` 照样有值 —— 所以这里不看 error
    resp.location.filter(|l| l.starts_with("magnet:"))
}
