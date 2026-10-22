//! AnimeTosho —— HTML 抓取，动漫资源聚合（页面本身就是 Nyaa/AniDex 的镜像索引）。
//!
//! 从 `src/providers/animetosho.js`（106 行）移植。
//!
//! ## 条目结构（真 fixture 实测，`test/fixtures/animetosho-one-piece.html`）
//!
//! ```text
//! <div class="home_list_entry …">
//!   <div class="date" title="Date/time submitted: 06/05/2026 11:02">11:02</div>
//!   <div class="size" title="Total file size: …">1.874 GB</div>
//!   <div class="link"><a href="/view/…">One.Piece.Vol002.…</a></div>
//!   <div class="links">
//!     <span title="Seeders: 2 / Leechers: 10">[2↑/10↓]</span>
//!     <a class="dllink" href="…torrent">Torrent</a>/
//!     <a href="magnet:?…">Magnet</a>
//!   </div>
//! </div>
//! ```
//!
//! ## 三处必须照抄的地方
//!
//! 1. **日期是 `d/M/yyyy`（日在前）**，而且 JS 只取**空格前那一段**、丢掉时间，
//!    再按**本地时区零点**算（`new Date(y, m-1, d)`）。
//!    例：`06/05/2026 11:02` → **2026-05-06 00:00 本地**（不是 6 月 5 日，也不是 11:02）。
//!    ⚠️ `bt_core::parse_date` 认不出这个形状（它按 `%Y/%m/%d` 认），所以这里自己解析。
//! 2. **seeders/leechers 从 `<span title="Seeders: N / Leechers: M">` 的文本里抠**
//!    （文本是 `[2↑/10↓]`，用 JS 的 `/(\d+)\D+(\d+)/` 语义）。
//! 3. **名称/大小/日期任一缺失就丢掉这一行**（JS 的 `if (!name)` / `if (!size)` / `if (!date)`），
//!    磁力缺失也丢。
//!
//! ## 一处**丢字段**（有意）
//!
//! JS 带 `fileDownloadLink`（`a.dllink` 的 `.torrent` 直链），Rust 的 `TorrentResult`
//! 没有这个字段 ⇒ 丢弃（与 `mikan` 同一个原因，见 `mikan.rs` 文件头）。

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{
    encode_uri_component, extract_info_hash, normalize, NumOrText, RawResult, TorrentResult,
};
use chrono::{Local, TimeZone, Utc};

use crate::SearchOutcome;

/// 站点根（JS 的 `BASE`）。
pub const BASE: &str = "https://animetosho.org";

/// 唯一镜像。JS 里 `DOMAINS = [BASE]`，仍走 `runMirrors`（照抄）。
const DOMAINS: &[&str] = &[BASE];

/// 条目容器（站上还带 `_alt` / `_compl_N` 等修饰类，所以只写基础类名）。
const ENTRIES: &str = "div.home_list_entry";

const LINK_DIV: &str = "div.link > a";
const SIZE_DIV: &str = "div.size";
const LINKS_DIV: &str = "div.links";
const DATE_DIV: &str = "div.date";

/// 用默认镜像搜索。`page` 与 JS 一样**被忽略**。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。JS 传的 name 是小写 `'animetosho'`。
pub async fn search_with(http: &HttpClient, bases: &[&str], query: &str) -> SearchOutcome {
    crate::run_mirrors("animetosho", bases, |b| search_at(http, b, query)).await
}

/// `/search?q=<q>`
fn search_url(base: &str, query: &str) -> String {
    format!("{base}/search?q={}", encode_uri_component(query))
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
    let entries = dom.select(ENTRIES);
    // JS: `if (entries.length === 0) return { results: [], error: null }`
    if entries.is_empty() {
        return SearchOutcome::ok(Vec::new());
    }

    let results = entries
        .iter()
        .filter_map(|e| parse_entry(&dom, *e, base))
        .collect();

    SearchOutcome::ok(results)
}

/// 解析一个条目；JS 的四个 `return null` 都照抄。
fn parse_entry(dom: &Dom, entry: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let anchor = dom.find(entry, LINK_DIV).into_iter().next()?;
    let name = text_trim(anchor);
    if name.is_empty() {
        return None;
    }

    let detail_url = attr(anchor, "href").filter(|h| !h.is_empty()).map(|href| {
        if href.starts_with("http") {
            href
        } else {
            format!("{base}{href}")
        }
    });

    // JS: `const size = …find('div.size').text().trim(); if (!size) return null;`
    let size = dom
        .find(entry, SIZE_DIV)
        .into_iter()
        .next()
        .map(text_trim)
        .unwrap_or_default();
    if size.is_empty() {
        return None;
    }

    let links = dom.find(entry, LINKS_DIV).into_iter().next();

    // seeders/leechers：`div.links` 里**第一个带 title 属性的 span**
    let (seeders, leechers) = links
        .and_then(|l| {
            dom.find(l, "span")
                .into_iter()
                .find(|s| attr(*s, "title").is_some())
        })
        .map(|s| seeders_leechers(&text_trim(s)))
        .unwrap_or((None, None));

    // 日期：`div.date` 的 title，去掉前缀后按 `d/M/yyyy` 解析
    let date_raw = dom
        .find(entry, DATE_DIV)
        .into_iter()
        .next()
        .and_then(|d| attr(d, "title"))
        .map(|t| t.replace("Date/time submitted:", ""))
        .map(|t| t.trim().to_string());
    let date = parse_date(date_raw.as_deref())?;

    let magnet = links
        .and_then(|l| dom.find(l, "a[href^=\"magnet:\"]").into_iter().next())
        .and_then(|a| attr(a, "href"))
        .filter(|m| !m.is_empty())?;

    Some(normalize(&RawResult {
        provider: "animetosho".to_string(),
        name: Some(name),
        size: Some(NumOrText::Text(size)),
        seeders: seeders.map(NumOrText::Text),
        leechers: leechers.map(NumOrText::Text),
        date: Some(NumOrText::Num(date)),
        info_hash: extract_info_hash(Some(&magnet)),
        magnet: Some(magnet),
        category: Some("Anime".to_string()),
        detail_url,
        ..Default::default()
    }))
}

/// 复刻 JS 的 `parseDate`。
///
/// ```js
/// if (/^today/i) return Date.now();
/// if (/^yesterday/i) return Date.now() - 86400000;
/// const first = s.split(/\s+/)[0];            // 只取第一段（丢掉时间）
/// const parts = first.split('/');             // d/M/yyyy
/// if (parts.length === 3) return new Date(y, m-1, d).getTime();  // 本地零点
/// const t = Date.parse(s); return isNaN(t) ? null : t;
/// ```
fn parse_date(raw: Option<&str>) -> Option<i64> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("today") {
        return Some(Utc::now().timestamp_millis());
    }
    if lower.starts_with("yesterday") {
        return Some(Utc::now().timestamp_millis() - 86_400_000);
    }

    let first = s.split_whitespace().next().unwrap_or("");
    let parts: Vec<&str> = first.split('/').collect();
    if parts.len() == 3 {
        let d: u32 = parts[0].parse().ok()?;
        let m: u32 = parts[1].parse().ok()?;
        let y: i32 = parts[2].parse().ok()?;
        // JS 的 `new Date(y, m-1, d)` —— **本地时区零点**
        return Local
            .with_ymd_and_hms(y, m, d, 0, 0, 0)
            .single()
            .map(|dt| dt.timestamp_millis());
    }

    // JS 最后还有 `Date.parse(s)` 兜底
    bt_core::normalize::parse_date(Some(&NumOrText::Text(s.to_string())))
}

/// 复刻 JS 的 `/(\d+)\D+(\d+)/`：第一个数字串、至少一个非数字、第二个数字串。
fn seeders_leechers(text: &str) -> (Option<String>, Option<String>) {
    let mut nums: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            nums.push(std::mem::take(&mut cur));
            if nums.len() == 2 {
                break;
            }
        }
    }
    // 文本以数字结尾时，最后一个数字串还在 `cur` 里
    if nums.len() < 2 && !cur.is_empty() {
        nums.push(cur);
    }
    if nums.len() == 2 {
        (Some(nums[0].clone()), Some(nums[1].clone()))
    } else {
        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_date, seeders_leechers};
    use chrono::{Local, TimeZone};

    #[test]
    fn seeders_leechers_mirror_the_js_regex() {
        let (s, l) = seeders_leechers("[2↑/10↓]");
        assert_eq!((s.as_deref(), l.as_deref()), (Some("2"), Some("10")));
        let (s, l) = seeders_leechers("[515↑/59↓]");
        assert_eq!((s.as_deref(), l.as_deref()), (Some("515"), Some("59")));
        // 只有一个数字 ⇒ JS 的 `(\d+)\D+(\d+)` 匹配不上 ⇒ 两个都是 null
        assert_eq!(seeders_leechers("[42]"), (None, None));
        assert_eq!(seeders_leechers("—"), (None, None));
    }

    #[test]
    fn date_is_day_first_and_drops_the_time() {
        // `06/05/2026 11:02` → 2026-05-06 00:00 **本地**（不是 6 月 5 日）
        let expected = Local
            .with_ymd_and_hms(2026, 5, 6, 0, 0, 0)
            .single()
            .expect("构造本地时间")
            .timestamp_millis();
        assert_eq!(parse_date(Some("06/05/2026 11:02")), Some(expected));
        assert_eq!(parse_date(Some("06/05/2026")), Some(expected));
        assert_eq!(parse_date(None), None);
        assert_eq!(parse_date(Some("")), None);
    }
}
