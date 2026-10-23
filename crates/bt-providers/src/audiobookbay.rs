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
//! ## ✅ 上游 bug 已于 2026-10-23 修掉（JS + Rust 两版一起改）
//!
//! 原来 JS 是这么写的：
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
//! 整段挤成一行 ⇒ `startsWith('File Size:')` 为假（**size 恒 null**），
//! 而它确实以 `Posted:` 开头 ⇒ `date` 被赋成整段垃圾串（`Date.parse` → NaN → null）。
//! **两个站上真实存在的字段全丢** —— 这是数据级 bug，不是外观差异，所以现在修。
//!
//! 修法（两版同轮）：不再按 `\n` 切，改成按**字段名**切 ——
//! 取标签之后、下一个已知标签之前的那一段（见 [`field_after`]）。
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

use crate::{MagnetOutcome, SearchOutcome};

/// 镜像列表，与 JS 的 `DOMAINS` 一致（只有一个 —— 见 `AGENTS.md` 的单域名清单）。
pub const DOMAINS: &[&str] = &["https://audiobookbay.lu"];

/// 每条结果的外层容器（JS: `$('div.post')`）。
const POST: &str = "div.post";

/// 名称 + 详情页链接（JS: `div.postTitle > h2 > a`）。
const TITLE_LINK: &str = "div.postTitle > h2 > a";

/// size / date 所在的那个 `<p>`（JS: `div.postContent > p:nth-child(3)`）。
const INFO_P: &str = "div.postContent > p:nth-child(3)";

/// 用默认镜像搜索。`page` 与 JS 一样**收下但不用** —— 站点搜索页没有分页参数。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。
///
/// ⚠️ JS 传的 name 是 **`'audiobookbay'`（全小写）**，错误串必须逐字一致。
pub async fn search_with(http: &HttpClient, bases: &[&str], query: &str) -> SearchOutcome {
    crate::run_mirrors("audiobookbay", bases, |b| search_at(http, b, query)).await
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

/// 这一格文本里出现的**所有**字段标签（切分时要在这里面找「下一个标签」）。
const INFO_LABELS: [&str; 4] = ["Posted:", "Format:", "Bitrate:", "File Size:"];

/// 取 `label` 之后、**下一个已知标签**之前的那一段（trim 后；空则 `None`）。
///
/// 这是 2026-10-23 那次修复的核心：页面用 `<br />` 分隔字段，而 `.text()` 不给
/// `<br>` 补换行 ⇒ 按 `\n` 切分只能得到一条。按标签切则不受分隔符影响。
fn field_after(text: &str, label: &str) -> Option<String> {
    let start = text.find(label)? + label.len();
    let rest = &text[start..];
    let mut end = rest.len();
    for other in INFO_LABELS {
        if other == label {
            continue;
        }
        if let Some(i) = rest.find(other) {
            if i < end {
                end = i;
            }
        }
    }
    let v = rest[..end].trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// 从 `infoText` 里切出 size / date（2026-10-23 起按字段名切，见文件头）。
///
/// - size 走 JS 的 `.replace(/s$/, '')`：站上写 `19.24 GBs` ⇒ `19.24 GB`；
/// - date 形如 `1 Sep 2026`，交给 `bt_core::parse_date` 的「月名」分支（按本地时区零点）。
fn split_info(info_text: &str) -> (Option<NumOrText>, Option<NumOrText>) {
    let size = field_after(info_text, "File Size:").map(|s| NumOrText::Text(strip_trailing_s(&s)));
    let date = field_after(info_text, "Posted:").map(NumOrText::Text);
    (size, date)
}

/// JS 的 `.replace(/s$/, '')` —— 只去掉**一个**结尾的 `s`（`19.24 GBs` → `19.24 GB`）。
fn strip_trailing_s(s: &str) -> String {
    s.strip_suffix('s').unwrap_or(s).to_string()
}

/// 惰性解析磁力。对应 JS 的 `resolveMagnet(detailUrl)`。
///
/// ⚠️ 取不到 hash（含 HTTP 出错）时**只给 `no_info_hash`**，不区分原因 —— 照抄 JS。
/// （`MagnetOutcome` 现住在 `crate::lib` 里，见那里的说明。）
pub async fn resolve_magnet(http: &HttpClient, detail_url: &str) -> MagnetOutcome {
    match info_hash_at(http, detail_url).await {
        Some(h) => MagnetOutcome {
            magnet: Some(format!("magnet:?xt=urn:btih:{h}")),
            info_hash: Some(h),
            error: None,
        },
        None => MagnetOutcome::err("no_info_hash"),
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

    /// ✅ 2026-10-23 修好后：同一段**没有换行**的真实文本，两个字段都能切出来。
    #[test]
    fn the_real_pages_info_text_now_yields_both_fields() {
        let (size, date) = split_info(REAL_INFO_TEXT);
        assert_eq!(size, Some(NumOrText::Text("19.24 GB".to_string())));
        assert_eq!(date, Some(NumOrText::Text("1 Sep 2026".to_string())));
    }

    /// 经 `normalize` 之后 size/date 都真的落地了（修之前两个都是 `—`）。
    #[test]
    fn normalize_now_keeps_size_and_date() {
        let out = one("Book", "/abss/x/", REAL_INFO_TEXT);
        assert!(out[0].size.is_some(), "size_text = {}", out[0].size_text);
        assert_eq!(out[0].size_text, "19.2 GB");
        assert!(out[0].date.is_some(), "date_text = {}", out[0].date_text);
        // 磁力仍然是惰性的（列表页没有 info hash）
        assert!(out[0].needs_magnet);
    }

    /// 字段各占一行时也要能切（按标签切，与分隔符无关）。
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
