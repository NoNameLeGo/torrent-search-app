//! 字幕站的搜索入口 —— **只拼 URL，不抓取、不下载**。
//!
//! ## 为什么只有这么薄的一层
//!
//! 三模式（磁力 / 视频 / 网盘）本来是「找到 → 打开网页」，字幕是唯一完全空着的维度。
//! 但「内置字幕搜索 + 下载 + 解压 + 按 `片名.SxxExx` 匹配本地文件」是一条**维护成本很高**的路：
//! assrt 要 token、zimuku 有 CSRF 反爬、SubHD 返回格式怪，任一家改版都要发版
//! （本机不能编译，一轮 CI 往返 ~3 分钟）。
//!
//! 所以先做 **L1**：用清洗过的片名拼出各家自己的搜索页，交给系统浏览器。
//! 站点改版时用户自己就能用；域名换了这里改一行。
//!
//! ⚠️ **不内置爬虫，不解析字幕列表** —— 那是 L2，`docs/BACKLOG-ZHUIJU.md` §2.1 有分级理由。
//!
//! ## 关于 URL 形态的可信度
//!
//! 本机有 MITM 透明代理 + 部分 DNS 污染，**单点探活不可信**（`AGENTS.md`「网络环境经验」）。
//! 实测记录：
//!
//! | 站 | 本机结果 | 结论 |
//! |---|---|---|
//! | `subhd.tv` | `/search/<q>` **200** | ✅ 形态已确认 |
//! | `assrt.net` | 连不上（000） | ⚠️ 本机 DNS 问题，形态按站内惯例 |
//! | `zimuku.org` | `/search?q=` 与首页都 404，且重定向串畸形 | ⚠️ 疑似代理劫持，待浏览器复核 |
//!
//! ⇒ 后两条**待一次浏览器点开确认**。真发现不对，改 [`ZIMUKU`] / [`ASSRT`] 的
//! `search` 一行即可，不需要动任何调用方。

use bt_core::normalize::encode_uri_component;

/// 一个字幕站。
///
/// `search` 里的 `{q}` 会被 URL 编码后的关键词替换 —— 与 [`crate::video::MacCmsSite`]
/// 的 `{q}` 占位符同一套约定，别发明第二种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtitleSite {
    /// 稳定 id。UI 的按钮回调只传它。
    pub id: &'static str,
    /// 展示名（按钮文案）。
    pub name: &'static str,
    /// 搜索页模板，`{q}` 是关键词占位符。
    pub search: &'static str,
}

/// SubHD —— **URL 形态已实测确认**（`/search/<q>` 返回 200）。
///
/// ⚠️ 是 `const` 不是 `static`：它要被 [`SITES`] 这个 `static` 数组的初始化式引用。
/// `static` 不能出现在另一个 `static` 的初始化式里（会被当成读取运行期内存），
/// 而 `const` 是编译期展开 —— 2026-10-27 首轮 CI 就栽在这（`E0515`）。
pub const SUBHD: SubtitleSite = SubtitleSite {
    id: "subhd",
    name: "SubHD",
    search: "https://subhd.tv/search/{q}",
};

/// 射手网（伪）。形态按站内惯例（`/sub/?searchword=`），本机不可达、待复核。
pub const ASSRT: SubtitleSite = SubtitleSite {
    id: "assrt",
    name: "ASSRT",
    search: "https://assrt.net/sub/?searchword={q}",
};

/// 字幕库。形态按站内惯例（`/search?q=`），本机 404 疑似劫持、待复核。
pub const ZIMUKU: SubtitleSite = SubtitleSite {
    id: "zimuku",
    name: "字幕库",
    search: "https://zimuku.org/search?q={q}",
};

/// 静态数组本身。**不能**把 `&[...]` 直接写在 `registry()` 的返回式里 ——
/// 那是**函数里的临时值**，借不出去（`E0515: cannot return reference to temporary value`）。
static SITES: [SubtitleSite; 3] = [SUBHD, ASSRT, ZIMUKU];

/// 静态注册表。**顺序 = UI 按钮顺序**：确认过的排前面。
pub fn registry() -> &'static [SubtitleSite] {
    &SITES
}

/// 按 id 取站。`None` = 不认识的 id（UI 传错时静默忽略，不 panic）。
pub fn site(id: &str) -> Option<&'static SubtitleSite> {
    registry().iter().find(|s| s.id == id)
}

/// 拼出搜索页 URL。`query` 会先 trim，空查询返回 `None`（不打开一个空搜索页）。
pub fn search_url(site: &SubtitleSite, query: &str) -> Option<String> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    Some(site.search.replace("{q}", &encode_uri_component(q)))
}

// ---------------------------------------------------------------------------
// 片名清洗
// ---------------------------------------------------------------------------

/// 从种子标题里抠出**能拿去搜字幕的片名**。
///
/// 种子标题长这样（真实形态）：
///
/// ```text
/// [电影天堂www.dy2018.com]流浪地球.2019.HD1080P.国语中字.mkv
/// 权力的游戏.Game.of.Thrones.S01E01.1080p.BluRay.x264.mkv
/// 【高清】[某某字幕组] 沙丘2 2024 2160p WEB-DL HDR
/// ```
///
/// 字幕站是按**片名**索引的，整串拿去搜基本搜不到。处理管线：
///
/// 1. 去掉 `[]` `【】` `()` `（）` 里的内容（组名、域名、语种标记）；
/// 2. 去掉视频扩展名；
/// 3. 在**季集号**（`S01E02` / `第01集`）或**年份**（`1900..=2100`）处截断；
/// 4. `.` `_` `+` 当分隔符换成空格；
/// 5. **丢掉发布组的垃圾词**（`1080p` / `WEB-DL` / `x264` / `1920x1080` …）；
/// 6. 折叠连续空白。
///
/// ## 几个刻意的选择
///
/// - **年份判定要求前后不是数字，且后面不是 `x` / `p`** —— 否则 `1920x1080` 里的 `1920`
///   会被当成 1920 年。
/// - **第 0 位的年份不算年份** —— 片名本身就叫《2012》，从 0 位截断会把查询变成空串。
/// - **片名本身就是年份时走兜底**：清洗结果为空就回退到「去括号后的原串」，
///   而不是返回空串把按钮变成死的。
/// - **不保留季集号**：字幕站按剧名索引，带 `S01E01` 反而搜不到。
pub fn query_from_title(title: &str) -> String {
    let stripped = strip_bracket_blocks(title);
    let no_ext = strip_video_ext(&stripped);
    let cut = cut_at_marker(&no_ext);

    let cleaned = finish(&cut);
    if !cleaned.is_empty() {
        return cleaned;
    }

    // 兜底：不截断，只做清洗（《2012》这类片名走这里）。
    let fallback = finish(&no_ext);
    if !fallback.is_empty() {
        return fallback;
    }

    // 再兜底：括号里的东西可能是片名本身，取原串前 40 字。
    let raw: String = title.chars().take(40).collect();
    collapse_ws(&raw.replace('\t', " "))
}

/// 分隔符归一 + 丢垃圾词 + 折叠空白。
fn finish(s: &str) -> String {
    let spaced: String = s
        .chars()
        .map(|c| match c {
            // 分隔符统一成空格；**落单的括号**也当分隔符（未闭合时 `[` 会漏下来）
            '.' | '_' | '+' | '\t' | '[' | ']' | '【' | '】' | '(' | ')' | '（' | '）' => ' ',
            other => other,
        })
        .collect();

    let kept: Vec<&str> = spaced
        .split_whitespace()
        .filter(|t| !is_junk_token(t))
        .collect();

    collapse_ws(&kept.join(" "))
}

/// 发布组/编码参数里的垃圾词。**只认拉丁字母形态**，中文标记（`国语中字`）不动 ——
/// 中文标记在字母表里分不清是片名还是标签（`国语` 可能是片名的一部分）。
fn is_junk_token(t: &str) -> bool {
    const JUNK: [&str; 33] = [
        "web", "webrip", "web-dl", "webdl", "bluray", "blu-ray", "bdrip", "brrip", "dvdrip",
        "hdtv", "remux", "uhd", "hdr", "hdr10", "dolby", "atmos", "x264", "x265", "h264", "h265",
        "hevc", "avc", "aac", "ac3", "dts", "ddp", "truehd", "10bit", "8bit", "proper", "repack",
        "internal", "complete",
    ];
    let lower = t.to_ascii_lowercase();

    // 分辨率：1080p / 2160p / 720p / 480p
    if let Some(prefix) = lower.strip_suffix('p') {
        if prefix.len() == 3 || prefix.len() == 4 {
            if prefix.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    // 分辨率：1920x1080 / 1280x720
    if let Some((w, h)) = lower.split_once('x') {
        let digits = |s: &str| (3..=4).contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit());
        if digits(w) && digits(h) {
            return true;
        }
    }
    // 4k / 8k（注意 4k 可能出现在片名里，但极罕见，且 `4k` 作为独立词几乎总是分辨率）
    if lower == "4k" || lower == "8k" {
        return true;
    }

    JUNK.iter().any(|j| *j == lower)
}

/// 去掉成对括号块（含嵌套）。**不成对的括号原样保留**（宁可留着也别吞掉片名）。
fn strip_bracket_blocks(s: &str) -> String {
    // (开, 闭) —— 半角与全角各一对
    const PAIRS: [(char, char); 4] = [('[', ']'), ('【', '】'), ('(', ')'), ('（', '）')];

    let mut out = String::with_capacity(s.len());
    let mut depth: Vec<char> = Vec::new();

    for ch in s.chars() {
        if let Some(&(_, close)) = PAIRS.iter().find(|(open, _)| *open == ch) {
            depth.push(close);
            continue;
        }
        if depth.last() == Some(&ch) {
            depth.pop();
            continue;
        }
        if !depth.is_empty() {
            // 在括号块内部，丢弃
            continue;
        }
        out.push(ch);
    }

    // 有未闭合的括号 ⇒ 上面的实现把尾部内容也吞了。退回「只删确实成对的块」。
    if !depth.is_empty() {
        return strip_paired_only(s, &PAIRS);
    }
    out
}

/// 保守版：只删**确实成对**的块，不吞未闭合的尾巴。
fn strip_paired_only(s: &str, pairs: &[(char, char); 4]) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut drop = vec![false; chars.len()];
    let mut stack: Vec<(usize, char)> = Vec::new();

    for (i, &ch) in chars.iter().enumerate() {
        if let Some(&(_, close)) = pairs.iter().find(|(open, _)| *open == ch) {
            stack.push((i, close));
        } else if let Some(pos) = stack.iter().rposition(|(_, close)| *close == ch) {
            let (start, _) = stack[pos];
            stack.truncate(pos);
            for d in drop.iter_mut().take(i + 1).skip(start) {
                *d = true;
            }
        }
    }

    chars
        .iter()
        .zip(drop)
        .filter(|(_, d)| !d)
        .map(|(c, _)| *c)
        .collect()
}

/// 去掉结尾的视频扩展名。只认白名单，别把 `.HDR` 当扩展名。
fn strip_video_ext(s: &str) -> String {
    const EXTS: [&str; 12] = [
        "mkv", "mp4", "avi", "rmvb", "rm", "ts", "m2ts", "mov", "wmv", "flv", "webm", "mpg",
    ];
    let trimmed = s.trim_end();
    let Some(dot) = trimmed.rfind('.') else {
        return trimmed.to_string();
    };
    let ext = &trimmed[dot + 1..];
    if EXTS.iter().any(|e| ext.eq_ignore_ascii_case(e)) {
        trimmed[..dot].to_string()
    } else {
        trimmed.to_string()
    }
}

/// 在第一个「季集号」或「年份」处截断。找不到就原样返回。
fn cut_at_marker(s: &str) -> String {
    let chars: Vec<(usize, char)> = s.char_indices().collect();

    for i in 0..chars.len() {
        let is_marker = (i > 0 && is_year_at(&chars, i))
            || is_season_episode_at(&chars, i)
            || is_cn_episode_at(&chars, i);
        if is_marker {
            return trim_separator_tail(&s[..chars[i].0]);
        }
    }

    s.to_string()
}

/// 截断点前的分隔符要一起剪掉（`流浪地球.` → `流浪地球`）。
fn trim_separator_tail(s: &str) -> String {
    s.trim_end_matches(|c: char| matches!(c, '.' | '_' | '+' | '-' | ' ' | '\t'))
        .to_string()
}

/// `chars[i..i+4]` 是 `1900..=2100` 的年份吗？
///
/// 前后都要卡：前面不能是数字，后面不能是数字 / `x` / `p`（避免 `1920x1080`、`2160p`）。
fn is_year_at(chars: &[(usize, char)], i: usize) -> bool {
    if i + 4 > chars.len() {
        return false;
    }
    let digits: String = chars[i..i + 4].iter().map(|(_, c)| *c).collect();
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let Ok(year) = digits.parse::<i32>() else {
        return false;
    };
    if !(1900..=2100).contains(&year) {
        return false;
    }
    if chars[i - 1].1.is_ascii_digit() {
        return false;
    }
    match chars.get(i + 4) {
        Some((_, next)) => !(next.is_ascii_digit() || matches!(next, 'x' | 'X' | 'p' | 'P')),
        None => true,
    }
}

/// `S01E02` / `s1e2` 形态。
fn is_season_episode_at(chars: &[(usize, char)], i: usize) -> bool {
    let get = |k: usize| chars.get(k).map(|(_, c)| *c);
    if !matches!(get(i), Some('S') | Some('s')) {
        return false;
    }
    let mut k = i + 1;
    let start = k;
    while matches!(get(k), Some(c) if c.is_ascii_digit()) {
        k += 1;
    }
    if k == start || k - start > 2 {
        return false;
    }
    if !matches!(get(k), Some('E') | Some('e')) {
        return false;
    }
    k += 1;
    let start = k;
    while matches!(get(k), Some(c) if c.is_ascii_digit()) {
        k += 1;
    }
    k > start && k - start <= 3
}

/// `第01集` / `第01话` 形态。
fn is_cn_episode_at(chars: &[(usize, char)], i: usize) -> bool {
    if chars[i].1 != '第' {
        return false;
    }
    let mut k = i + 1;
    let start = k;
    while matches!(chars.get(k), Some((_, c)) if c.is_ascii_digit()) {
        k += 1;
    }
    if k == start {
        return false;
    }
    matches!(
        chars.get(k),
        Some((_, '集')) | Some((_, '话')) | Some((_, '話'))
    )
}

/// 折叠空白（含全角空格），并 trim。
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for ch in s.chars() {
        let is_ws = ch.is_whitespace() || ch == '\u{3000}';
        if is_ws {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique_and_resolvable() {
        let reg = registry();
        assert_eq!(reg.len(), 3);
        for (i, s) in reg.iter().enumerate() {
            assert!(site(s.id).is_some(), "{} 取不到", s.id);
            assert!(!s.name.is_empty());
            assert!(s.search.contains("{q}"), "{} 模板缺 {{q}}", s.id);
            for other in &reg[i + 1..] {
                assert_ne!(s.id, other.id);
            }
        }
        assert!(site("nope").is_none());
    }

    #[test]
    fn search_url_encodes_and_rejects_empty() {
        let url = search_url(&SUBHD, "流浪地球").expect("非空");
        // `encode_uri_component` 是 UTF-8 百分号编码
        assert_eq!(
            url,
            "https://subhd.tv/search/%E6%B5%81%E6%B5%AA%E5%9C%B0%E7%90%83"
        );

        assert!(search_url(&SUBHD, "   ").is_none());
        assert!(search_url(&ZIMUKU, "").is_none());
    }

    #[test]
    fn search_url_keeps_the_template_shape() {
        assert_eq!(
            search_url(&ASSRT, "dune").unwrap(),
            "https://assrt.net/sub/?searchword=dune"
        );
        assert_eq!(
            search_url(&ZIMUKU, "dune").unwrap(),
            "https://zimuku.org/search?q=dune"
        );
    }

    #[test]
    fn query_strips_site_tags_and_extension() {
        assert_eq!(
            query_from_title("[电影天堂www.dy2018.com]流浪地球.2019.HD1080P.国语中字.mkv"),
            "流浪地球"
        );
        assert_eq!(
            query_from_title("【高清】[某某字幕组] 沙丘2 2024 2160p WEB-DL HDR"),
            "沙丘2"
        );
    }

    #[test]
    fn query_cuts_at_season_episode() {
        assert_eq!(
            query_from_title("权力的游戏.Game.of.Thrones.S01E01.1080p.BluRay.x264.mkv"),
            "权力的游戏 Game of Thrones"
        );
        assert_eq!(query_from_title("繁花.第01集.2160p.WEB-DL.mkv"), "繁花");
    }

    #[test]
    fn query_drops_resolution_and_codec_tokens() {
        // 1920x1080 里的 1920 不能被当成年份（否则整串变空）
        assert_eq!(query_from_title("Some.Movie.1920x1080.mkv"), "Some Movie");
        // 2160p 超出年份范围，但也该被当垃圾词丢掉
        assert_eq!(query_from_title("Dune.2160p.WEB-DL.mkv"), "Dune");
        assert_eq!(
            query_from_title("Dune.2021.1080p.BluRay.x265.10bit.HDR.mkv"),
            "Dune"
        );
    }

    #[test]
    fn query_falls_back_when_title_is_just_a_year() {
        // 片名就叫《2012》：0 位的年份不算标记，不能截成空串
        assert_eq!(query_from_title("2012"), "2012");
        assert_eq!(query_from_title("2012.mkv"), "2012");
    }

    #[test]
    fn query_keeps_unpaired_brackets() {
        // 括号没闭合时不能把后面的片名一起吞掉
        assert_eq!(
            query_from_title("[字幕组 流浪地球.2019.mkv"),
            "字幕组 流浪地球"
        );
    }

    #[test]
    fn query_collapses_separators() {
        assert_eq!(query_from_title("The_Matrix.1999.mkv"), "The Matrix");
        assert_eq!(query_from_title("  流浪地球  "), "流浪地球");
    }
}
