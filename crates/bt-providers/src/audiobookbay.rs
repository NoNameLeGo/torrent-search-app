//! AudioBookBay —— 有声书站的 HTML 抓取（B 组第 4 个，也是**第一个带惰性磁力解析**的）。
//!
//! 从 `src/providers/audiobookbay.js`（92 行）移植。
//!
//! ## 页面结构（已用 cheerio 真值核实，见 `test/fixtures/html-probes.json`）
//!
//! 每条结果是一个 `div.post`（fixture 里 9 条）：
//!
//! ```text
//! div.post
//!   div.postTitle > h2 > a          ← 名称 + 详情页链接（相对路径，以 / 开头）
//!   div.postInfo                    ← Category: … / Language: …（**JS 没读它**）
//!   div.postContent
//!     ├ div.center                  ← 1: 分享者 + 封面图（图名里带 info hash）
//!     ├ p[style=center;]            ← 2: 空
//!     └ p[style=text-align:center;] ← 3: ← JS 用 `p:nth-child(3)` 命中的就是它
//! ```
//!
//! ⚠️ 那个 `<p>` 只在前两个兄弟都在时才是第 3 个孩子（`nth-child` 数的是**所有**孩子，
//! 不是只数 `p`）。cheerio 侧 `info.count = 9` 确认 9 条全都命中。
//!
//! ## ⚠️ 上游 bug：size 与 date 这一版永远是空的（照抄，未修）
//!
//! JS 是这么写的：
//!
//! ```js
//! const infoText = $(p).text();                       // ← 这里没有换行
//! const lines = infoText.split('\n').map(trim).filter(Boolean);
//! for (const line of lines) {
//!   if (line.startsWith('File Size:'))      size = …
//!   else if (line.startsWith('Posted:'))    date = …
//! }
//! ```
//!
//! 作者的意图是「一个字段一行」，但字段之间是 `<br />`，而 **cheerio 的 `.text()`
//! 不会给 `<br>` 补换行**。cheerio 1.2.0 的真值（探针 `info.first_text`）是：
//!
//! ```text
//! Posted: 1 Sep 2026Format: MP3 / Bitrate: 128 KbpsFile Size: 19.24 GBs
//! ```
//!
//! 整段挤成一行 → `startsWith('File Size:')` 为假 → **size 恒为 null**；
//! 而它确实以 `Posted:` 开头，于是 `date` 被赋成整段垃圾串（还会因为 `else if`
//! 永远走不到 File Size 那一支）。`Date.parse(…垃圾串)` = NaN → 最终 `date: null`。
//!
//! 实测 `src/lib/normalize.js` 的输出：`sizeText: "—"`、`dateText: "—"`、`needsMagnet: true`。
//! **本移植逐字照抄这个行为**（阶段一的验收点是「与 Node 版逐字段一致」，且这属于
//! 「数据缺失 / 纯显示」而不是 `linuxtracker` 那种死链功能性 bug）。
//! 真要修，两版一起改：把 `split('\n')` 换成按字段名切分即可（本文件 `<mod tests>` 里
//! 有一条 `split_info` 的正确行为测试，改的时候照它写）。
//!
//! ## 磁力是惰性的
//!
//! 列表页**没有**磁力，`info hash` 在详情页的 `<td>Info Hash:</td><td>…</td>` 里，
//! 所以每条结果都是 `magnet: null` + `needsMagnet: true`，等用户点击时走
//! [`resolve_magnet`]。JS 那边由 `server.js` 的 `/api/magnet` 调 `resolveMagnet`。

use bt_core::dom::{attr, closest_tag, nth, text, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, normalize, NumOrText, RawResult};
use bt_core::TorrentResult;

use crate::SearchOutcome;

/// 镜像列表，与 JS 的 `DOMAINS` 一致（只有一个 —— 见 `AGENTS.md` 的单域名清单）。
pub const DOMAINS: &[&str] = &["https://audiobookbay.lu"];

/// 每条结果的外层容器（JS: `$('div.post')`）。
const POST: &str = "div.post";

/// 名称 + 详情页链接（JS: `div.postTitle > h2 > a`）。
const TITLE_LINK: &str = "div.postTitle > h2 > a";

/// size / date 所在的那个 `<p>`（JS: `div.postContent > p:nth-child(3)`）。
const INFO_P: &str = "div.postContent > p:nth-child(3)";

/// `resolveMagnet` 的返回形状，字段名对齐 JS（`magnet` / `infoHash` / `error`）。
#[derive(Debug, Clone, PartialEq)]
pub struct MagnetOutcome {
    pub magnet: Option<String>,
    pub info_hash: Option<String>,
    pub error: Option<String>,
}

/// 用默认镜像搜索。`page` 与 JS 一样**收下但不用** —— 站点搜索页没有分页参数。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。
///
/// ⚠️ JS 传的 name 是 **`'audiobookbay'`（全小写）**，错误串必须逐字一致。
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

    SearchOutcome::err(format!("audiobookbay unreachable ({})", errs.join("; ")))
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    // JS: `${base}/?s=${encodeURIComponent(query)}` —— 是 GET 的查询串，不是路径
    let url = format!("{base}/?s={}", encode_uri_component(query));

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    // JS 这里是 `if (error || !html) return { results: [], error }` —— 空 body 时
    // error 是 undefined（falsy），`runMirrors` 会把它滤掉，等价于「正常但没结果」。
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::ok(Vec::new());
    };

    let dom = Dom::parse(&html);
    // JS: `if (items.length === 0) return {…, error: 'no_results_parsed'}`
    if dom.select(POST).is_empty() {
        return SearchOutcome::err("no_results_parsed");
    }

    let results: Vec<TorrentResult> = dom
        .select(POST)
        .iter()
        .filter_map(|post| parse_post(&dom, *post, base))
        .collect();

    // JS: 抓到了容器但一条都没解析出来，也算解析失败
    if results.is_empty() {
        return SearchOutcome::err("no_results_parsed");
    }
    SearchOutcome::ok(results)
}

/// 解析一条 `div.post`。对应 JS 的 `parseListItem`（`null` 的那两条路径也照抄）。
fn parse_post(dom: &Dom, post: ElementRef<'_>, base: &str) -> Option<TorrentResult> {
    let link = dom.find(post, TITLE_LINK).into_iter().next()?;
    let href = attr(link, "href").filter(|h| !h.is_empty())?;

    let name = text_trim(link);
    if name.is_empty() {
        return None;
    }

    // JS: `href.startsWith('http') ? href : `${base}${href}`` —— 裸拼接。
    // 本站 href 一定是 `/abss/…`（fixture 9 条全以 `/` 开头），所以不会拼出
    // linuxtracker 那种「少一个斜杠」的死链；照抄不修。
    let detail_url = if href.starts_with("http") {
        href
    } else {
        format!("{base}{href}")
    };

    // JS 在这里**不 trim**（trim 是逐行做的），保持一致
    let info_text = dom
        .find(post, INFO_P)
        .into_iter()
        .next()
        .map(text)
        .unwrap_or_default();
    let (size, date) = split_info(&info_text);

    Some(normalize(&RawResult {
        provider: "audiobookbay".to_string(),
        name: Some(name),
        size,
        date,
        // 列表页不带磁力 → normalize 会自动把 needs_magnet 置 true
        magnet: None,
        // JS 硬编码 'Books'，**没有**用 postInfo 里那句 "Category: Sci-Fi"
        category: Some("Books".to_string()),
        detail_url: Some(detail_url),
        ..Default::default()
    }))
}

/// 复刻 JS 对 `infoText` 的 `split('\n')` + `if/else if` 链。
///
/// ⚠️ 真实页面上这段文本**没有换行**（见文件头），所以实际只有一条 line、
/// 且它不以 `File Size:` 开头 —— 结果就是 `(None, Some(垃圾串))`。别"顺手修好"，
/// 修了就和 Node 版对不上了；要修两版一起改。
fn split_info(info_text: &str) -> (Option<NumOrText>, Option<NumOrText>) {
    let mut size: Option<NumOrText> = None;
    let mut date: Option<NumOrText> = None;

    for raw_line in info_text.split('\n') {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        // 顺序照抄：File Size 在前，Posted 在 else if 里
        if let Some(rest) = line.strip_prefix("File Size:") {
            size = Some(NumOrText::Text(strip_trailing_s(rest.trim())));
        } else if let Some(rest) = line.strip_prefix("Posted:") {
            date = Some(NumOrText::Text(rest.trim().to_string()));
        }
    }

    (size, date)
}

/// JS 的 `.replace(/s$/, '')` —— 只去掉**一个**结尾的 `s`（`19.24 GBs` → `19.24 GB`）。
fn strip_trailing_s(s: &str) -> String {
    s.strip_suffix('s').unwrap_or(s).to_string()
}

/// 惰性解析磁力。对应 JS 的 `resolveMagnet(detailUrl)`。
///
/// ⚠️ 取不到 hash（含 HTTP 出错）时**只给 `no_info_hash`**，不区分原因 —— 照抄 JS。
pub async fn resolve_magnet(http: &HttpClient, detail_url: &str) -> MagnetOutcome {
    match info_hash_at(http, detail_url).await {
        Some(h) => MagnetOutcome {
            magnet: Some(format!("magnet:?xt=urn:btih:{h}")),
            info_hash: Some(h),
            error: None,
        },
        None => MagnetOutcome {
            magnet: None,
            info_hash: None,
            error: Some("no_info_hash".to_string()),
        },
    }
}

/// 详情页里的 info hash。对应 JS 的 `getInfoHash`。
///
/// 页面形状：`<tr><td>Info Hash:</td><td>f759c8…</td></tr>`，
/// JS 是「扫所有 `td`，找到自己的文本正好是 `Info Hash:` 的那个，取它的**下一个元素兄弟**」。
///
/// 本移植取的是「同一行里它后面那个 `td`」：用 [`closest_tag`] 回到 `tr` 再按位置取下一个，
/// 语义与 `.next()` 等价（这一行只有这两个 `td`），而且只用 `dom.rs` 里已被探针钉住的 API
/// —— 本机没有 cargo，少赌一个 `next_siblings()` 的签名。
pub async fn info_hash_at(http: &HttpClient, detail_url: &str) -> Option<String> {
    let resp = http.get_text(detail_url, None).await;
    if resp.error.is_some() {
        return None;
    }
    let html = resp.html.filter(|h| !h.is_empty())?;
    let dom = Dom::parse(&html);

    let label = dom
        .select("td")
        .into_iter()
        .find(|td| text_trim(*td) == "Info Hash:")?;

    let row = closest_tag(label, "tr")?;
    let tds = dom.find(row, "td");
    let idx = tds.iter().position(|td| td.id() == label.id())?;
    let value = nth(&tds, idx + 1).map(text_trim).unwrap_or_default();

    // JS: `/^[a-f0-9]{40}$/i.test(hash) ? hash : null` —— 原样返回（**不小写**）
    is_info_hash(&value).then_some(value)
}

/// 40 位十六进制（大小写都收）。对应 JS 的 `/^[a-f0-9]{40}$/i`。
fn is_info_hash(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "f759c8ef86a9ff389e7b965cf5037288aa5ec896";

    /// cheerio 1.2.0 在真 fixture 上给出的 `div.postContent > p:nth-child(3)` 的完整文本
    /// （探针 `info.first_text`）。
    const REAL_INFO_TEXT: &str =
        "Posted: 1 Sep 2026Format: MP3 / Bitrate: 128 KbpsFile Size: 19.24 GBs";

    fn search_page(posts: &str) -> String {
        format!("<div class=\"posts\">{posts}</div>")
    }

    fn post(title: &str, href: &str, info: &str) -> String {
        format!(
            r#"<div class="post">
                 <div class="postTitle"><h2><a href="{href}" rel="bookmark">{title}</a></h2></div>
                 <div class="postInfo">Category: Sci-Fi&nbsp;<br />Language: Polish</div>
                 <div class="postContent"><div class="center">cover</div>
                   <p style="center;"> </p>
                   <p style='text-align:center;'>{info}</p></div>
               </div>"#
        )
    }

    fn one(title: &str, href: &str, info: &str) -> Vec<TorrentResult> {
        let html = search_page(&post(title, href, info));
        let dom = Dom::parse(&html);
        let posts = dom.select(POST);
        assert_eq!(posts.len(), 1, "自造页面应该正好一条");
        parse_post(&dom, *posts.first().unwrap(), "https://audiobookbay.lu")
            .into_iter()
            .collect()
    }

    #[test]
    fn parses_a_post_end_to_end() {
        let out = one("Some Book", "/abss/some-book/", "Posted: 1 Sep 2026");
        assert_eq!(out.len(), 1);
        let r = &out[0];
        assert_eq!(r.provider, "audiobookbay");
        assert_eq!(r.name, "Some Book");
        assert_eq!(
            r.detail_url.as_deref(),
            Some("https://audiobookbay.lu/abss/some-book/")
        );
        assert_eq!(r.category.as_deref(), Some("Books"), "JS 硬编码 Books");
        assert!(r.needs_magnet, "列表页没有磁力");
        assert_eq!(r.magnet, None);
        assert_eq!(r.info_hash, None);
    }

    /// 详情页链接是绝对 URL 时原样保留（JS 的 `href.startsWith('http')` 分支）。
    #[test]
    fn absolute_detail_urls_are_kept_verbatim() {
        let out = one("Book", "https://mirror.example/abss/x/", "");
        assert_eq!(
            out[0].detail_url.as_deref(),
            Some("https://mirror.example/abss/x/")
        );
    }

    #[test]
    fn posts_without_href_or_name_are_skipped() {
        let html = search_page(&format!(
            "{}{}{}",
            post("no href", "", "Posted: 1 Sep 2026"),
            post("", "/abss/no-name/", "Posted: 1 Sep 2026"),
            post("ok", "/abss/ok/", "Posted: 1 Sep 2026")
        ));
        let dom = Dom::parse(&html);
        let results: Vec<TorrentResult> = dom
            .select(POST)
            .iter()
            .filter_map(|p| parse_post(&dom, *p, "https://audiobookbay.lu"))
            .collect();
        assert_eq!(results.len(), 1, "缺 href / 缺名字的整条丢掉");
        assert_eq!(results[0].name, "ok");
    }

    /// ⚠️ 上游行为钉住：真页面那段文本没有换行 → size 恒空、date 是整段垃圾串。
    #[test]
    fn the_real_pages_info_text_loses_both_size_and_date() {
        let (size, date) = split_info(REAL_INFO_TEXT);
        assert_eq!(size, None, "JS 的 else if 让 File Size 那一支永远走不到");
        // JS: `line.substring('Posted:'.length).trim()` —— 只切掉 "Posted:" 五个字，
        // 后面的 Format / File Size 全被当成日期
        let expected_date = REAL_INFO_TEXT
            .strip_prefix("Posted: ")
            .expect("REAL_INFO_TEXT 该以 Posted: 开头");
        assert_eq!(date, Some(NumOrText::Text(expected_date.to_string())));
    }

    /// 上面那条垃圾 date 经 `normalize` 之后确实是空的（与 JS 实测一致）。
    #[test]
    fn normalize_turns_the_garbage_date_into_nothing() {
        let out = one("Book", "/abss/x/", REAL_INFO_TEXT);
        assert_eq!(out[0].size, None);
        assert_eq!(out[0].size_text, "—");
        assert_eq!(out[0].date, None);
        assert_eq!(out[0].date_text, "—");
    }

    /// 若字段真的各占一行（`split_info` 被修好后的目标行为），两个字段都该能解析出来。
    /// 这条测试是给「两版一起修」那天用的参照。
    #[test]
    fn split_info_parses_one_field_per_line() {
        let (size, date) = split_info("Posted: 1 Sep 2026\nFormat: MP3\nFile Size: 19.24 GBs");
        assert_eq!(size, Some(NumOrText::Text("19.24 GB".to_string())));
        assert_eq!(date, Some(NumOrText::Text("1 Sep 2026".to_string())));
    }

    #[test]
    fn split_info_ignores_blank_lines_and_unknown_fields() {
        let (size, date) = split_info("\n\n  Language: Polish \n \n");
        assert_eq!(size, None);
        assert_eq!(date, None);
    }

    #[test]
    fn split_info_checks_file_size_before_posted() {
        // 单行以 File Size 开头 → 进 size 分支，date 保持空（JS 的 if 在前）
        let (size, date) = split_info("File Size: 1 GBs");
        assert_eq!(size, Some(NumOrText::Text("1 GB".to_string())));
        assert_eq!(date, None);
    }

    #[test]
    fn strip_trailing_s_removes_only_one_s() {
        assert_eq!(strip_trailing_s("19.24 GBs"), "19.24 GB");
        assert_eq!(strip_trailing_s("19.24 GBBs"), "19.24 GBB");
        assert_eq!(strip_trailing_s(""), "");
        assert_eq!(strip_trailing_s("MB"), "MB");
    }

    #[test]
    fn is_info_hash_accepts_40_hex_and_rejects_everything_else() {
        assert!(is_info_hash(HASH));
        assert!(is_info_hash(&HASH.to_uppercase()), "大小写都收");
        assert!(!is_info_hash(""), "空串不行");
        assert!(!is_info_hash(&HASH[..39]), "少一位不行");
        assert!(!is_info_hash(&format!("{HASH}0")), "多一位不行");
        assert!(
            !is_info_hash(&"z".repeat(40)),
            "非十六进制不行（'z' 不在 [a-f0-9] 里）"
        );
    }

    /// 详情页里那个「标签 td → 下一个 td」的取值逻辑，用合成页面钉住。
    ///
    /// 注意 `</td>` 与 `<td>` 之间**真的有换行**（fixture 里就是这样），
    /// 所以这条顺带验证了「不用管中间那个文本节点」。
    #[test]
    fn info_hash_comes_from_the_cell_after_the_label() {
        let html = format!(
            "<table><tbody>\n<tr>\n<td>Piece Size:</td>\n<td>2 MBs</td>\n</tr>\n\
             <tr>\n<td>Info Hash:</td>\n<td>{HASH}</td>\n</tr>\n</tbody></table>"
        );
        let dom = Dom::parse(&html);
        let label = dom
            .select("td")
            .into_iter()
            .find(|td| text_trim(*td) == "Info Hash:")
            .expect("该找到标签");
        let row = closest_tag(label, "tr").unwrap();
        let tds = dom.find(row, "td");
        let idx = tds.iter().position(|td| td.id() == label.id()).unwrap();
        let value = nth(&tds, idx + 1).map(text_trim).unwrap_or_default();
        assert_eq!(value, HASH);
        assert!(is_info_hash(&value));
    }
}
