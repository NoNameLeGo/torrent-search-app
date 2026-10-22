//! 离线 demo 引擎 —— 从 `src/providers/demo.js`（77 行）逐条移植。
//!
//! ## 它是干什么的
//!
//! 没有网络、或者所有站点都挂的时候，仍然把**整条 UI 链路**走通：搜索 → 排序 →
//! 分类筛选 → 分页 / 无限滚动。数据是**确定性**的（同一个 query 永远给出同一批结果），
//! 所以它也是「UI 出问题 vs 站点出问题」的判别器。
//!
//! ## 确定性从哪来
//!
//! 种子 = **FNV-1a(query)**，随机数 = **mulberry32(种子)**。两者都是逐位复刻 JS 的：
//!   · `hashStr` 走的是 **UTF-16 code unit**（JS `charCodeAt`），所以 Rust 这边必须
//!     用 `encode_utf16()` —— 用 `chars()` 的话中文 query 会算出**不同的种子**，
//!     整个池子就都不一样了（这是最容易看不出、也最容易写错的一处）。
//!   · `mulberry32` 的每一步都是 32 位环绕运算（JS `|0` / `>>>` / `Math.imul`），
//!     所以在 Rust 里用 `u32` + `wrapping_*` 就能逐位对上。
//!
//! ## ⚠️ 与 JS 的一处**有意**差异：日期
//!
//! JS 在生成池子时直接读 `Date.now()`。那让 `search()` 的输出依赖**调用时刻**，
//! 测试就没法钉死。这里把"现在"提成参数：
//!   · [`pool_at(query, now_ms)`](pool_at) —— 纯函数，给定 (query, now_ms) 输出完全确定
//!   · [`Demo::search`] 传真实时间 ⇒ **线上表现与 JS 完全一致**
//! 测试传一个固定值，于是可以断言到毫秒。
//!
//! ## 元数据（照 `src/providers/index.js:120` 的注册项）
//!
//! `enabled: true`、`browseable: true` —— 注意 JS **没有**给它 `paginated: true`，
//! 虽然它确实消费 `page`。`index.js:155` 那条是 `page > 1 && !p.paginated && !p.demo`，
//! 即 **`demo` 是单独的一个豁免分支**。所以这里的 `paginated()` 保持 `false`（默认），
//! 靠 `demo() = true` 走豁免 —— 照抄 JS，别"顺手修正"。

use bt_core::http::HttpClient;
use bt_core::normalize::{normalize, NumOrText, RawResult, TorrentResult};

use crate::{BoxFuture, Provider, SearchOutcome};

/// 每页条数（JS `PER_PAGE`）。
pub const PER_PAGE: usize = 20;
/// 池子总条数（JS `buildPool` 里的 `total`）。
pub const POOL_SIZE: usize = 120;

/// JS `CATEGORIES`
const CATEGORIES: [&str; 6] = ["Movies", "TV", "Anime", "Music", "Software", "Games"];
/// JS `SUFFIXES`
const SUFFIXES: [&str; 10] = [
    "[1080p]",
    "[720p]",
    "[2160p 4K]",
    "EXTENDED",
    "REMASTERED",
    "COMPLETE",
    "[WEB-DL]",
    "[BluRay]",
    "SEASON 1",
    "COLLECTION",
];

/// JS `hashStr` —— FNV-1a，**按 UTF-16 code unit 迭代**。
///
/// ⚠️ 别改成 `chars()`：中文 query 在 JS 里每个汉字是 1 个 code unit、
/// 而 Rust 的 `chars()` 也是 1 个 scalar（这里恰好一样），但**补充平面**的字符
/// （emoji 等）在 JS 里是 **2 个** code unit、在 `chars()` 里是 1 个 ⇒ 种子不同。
/// `encode_utf16()` 两边都对。
fn hash_str(s: &str) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for unit in s.encode_utf16() {
        h ^= u32::from(unit);
        // JS `Math.imul(h, 16777619)` —— 只保留低 32 位
        h = h.wrapping_mul(16_777_619);
    }
    h
}

/// JS `mulberry32` —— 逐位复刻（`|0` = 环绕、`>>>` = 无符号右移）。
///
/// 返回一个闭包，每调一次前进一格；值域 `[0, 1)`。
fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
    let mut s = seed;
    move || {
        s = s.wrapping_add(0x6D2B_79F5);
        let mut t = (s ^ (s >> 15)).wrapping_mul(1 | s);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// JS `Math.floor(rng() * n)` —— 值域 `[0, n)`。
fn pick(rng: &mut impl FnMut() -> f64, n: usize) -> usize {
    let v = (rng() * n as f64).floor();
    // rng() 落在 [0,1)，所以这里不可能越界；仍夹一下，避免浮点边界上 panic
    (v as usize).min(n.saturating_sub(1))
}

/// JS `randHex(rng, n)`
fn rand_hex(rng: &mut impl FnMut() -> f64, n: usize) -> String {
    let mut out = String::with_capacity(n);
    for _ in 0..n {
        let d = (rng() * 16.0).floor() as u32;
        out.push(char::from_digit(d.min(15), 16).unwrap_or('0'));
    }
    out
}

/// 照着 `src/providers/demo.js` 的 `buildPool` 生成整个池子。
///
/// `now_ms` 就是 JS 里的 `Date.now()`（毫秒）—— 传进来是为了让这个函数**纯**。
pub fn pool_at(query: &str, now_ms: i64) -> Vec<TorrentResult> {
    // JS: `mulberry32(hashStr(query || 'demo'))` —— **空 query 用 'demo' 当种子**，
    // 但名字里仍然用**原始**（空的）query。所以空 query 的第一条叫
    // `" [720p] - S01E02 [Games]"`（前导空格不是 bug）。
    let seed_key = if query.is_empty() { "demo" } else { query };
    let mut rng = mulberry32(hash_str(seed_key));

    let mut out = Vec::with_capacity(POOL_SIZE);
    for _ in 0..POOL_SIZE {
        let cat = CATEGORIES[pick(&mut rng, CATEGORIES.len())];
        let suffix = SUFFIXES[pick(&mut rng, SUFFIXES.len())];
        let ep = 1 + pick(&mut rng, 24);
        let name = format!("{query} {suffix} - S01E{ep:02} [{cat}]");
        let size = ((rng() * 48.0 + 0.2) * 1_073_741_824.0).floor() as i64; // 0.2GB–48GB
        let seeders = (rng() * 5000.0).floor() as i64;
        let leechers = (rng() * 1500.0).floor() as i64;
        let age_days = (rng() * 730.0).floor() as i64;
        let date_ms = now_ms - age_days * 86_400_000;
        let info_hash = rand_hex(&mut rng, 40);

        out.push(normalize(&RawResult {
            provider: "demo".to_string(),
            name: Some(name.clone()),
            size: Some(NumOrText::Num(size)),
            seeders: Some(NumOrText::Num(seeders)),
            leechers: Some(NumOrText::Num(leechers)),
            // ⚠️ 毫秒：JS 也是毫秒，而 `parse_date` 的判据是 `> 1e11` ⇒ 两边同一条路
            date: Some(NumOrText::Num(date_ms)),
            category: Some(cat.to_string()),
            info_hash: Some(info_hash.clone()),
            magnet: Some(format!(
                "magnet:?xt=urn:btih:{info_hash}&dn={}",
                encode_uri_component(&name)
            )),
            ..Default::default()
        }));
    }
    out
}

/// JS `encodeURIComponent` —— magnet 的 `dn=` 用它。
///
/// 只实现"不转义"的那一半（`A-Za-z0-9-_.!~*'()`），其余按 UTF-8 逐字节 `%XX`。
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
        {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 分页切片 —— **纯函数**，所以分页行为能离线测（`search` 只是"它 + 一个 now"）。
///
/// 返回 `(这一页的结果, has_more)`。⚠️ `has_more` 是 demo **显式表态**的那个值
/// （JS 里只有 demo 会），所以调用方要把它放进 `Some(..)`。
pub fn slice_page(pool: &[TorrentResult], page: u32) -> (Vec<TorrentResult>, bool) {
    let start = (page.max(1) as usize - 1) * PER_PAGE;
    let slice = if start >= pool.len() {
        Vec::new()
    } else {
        pool[start..(start + PER_PAGE).min(pool.len())].to_vec()
    };
    (slice, start + PER_PAGE < pool.len())
}

/// 现在的毫秒时间戳（`std` 就够，不用引 chrono）。
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 离线 demo 引擎。
#[derive(Debug, Clone, Copy, Default)]
pub struct Demo;

impl Provider for Demo {
    fn id(&self) -> &'static str {
        "demo"
    }

    fn name(&self) -> &'static str {
        "Demo (offline)"
    }

    /// JS 的注册项里有 `browseable: true`（`index.js:120`）—— 空 query 也能查。
    fn browseable(&self) -> bool {
        true
    }

    /// ⚠️ 它**消费** `page`，但 JS 并没有给它 `paginated: true` ——
    /// `index.js:155` 是 `page > 1 && !p.paginated && !p.demo`，`demo` 是**单独**的豁免分支。
    /// 所以这里保持默认的 `false`，靠 `demo() = true`。照抄 JS，别顺手"修正"。
    fn demo(&self) -> bool {
        true
    }

    /// 不碰网络 —— `http` 用不到（参数保持 `_` 前缀以免 unused 警告）。
    fn search<'a>(
        &'a self,
        _http: &'a HttpClient,
        query: &'a str,
        page: u32,
    ) -> BoxFuture<'a, SearchOutcome> {
        Box::pin(async move {
            let pool = pool_at(query, now_ms());
            let (results, has_more) = slice_page(&pool, page);
            // ⚠️ demo **会**显式表态 hasMore（JS 里只有它会）—— 所以是 Some，不是 None
            SearchOutcome {
                results,
                error: None,
                has_more: Some(has_more),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 钉死的"现在" ⇒ 日期可以断言到毫秒。
    const NOW: i64 = 1_800_000_000_000;

    /// ⚠️ 这些期望值是**用 node 跑 JS 的 demo.js 得到的**（`scripts/demo-truth.cjs`），
    /// 不是手写的。改 `pool_at` 之前先重跑那个脚本。
    fn names(q: &str) -> Vec<String> {
        pool_at(q, NOW).into_iter().map(|r| r.name).collect()
    }

    #[test]
    fn pool_has_the_js_size_and_the_first_items_match_js() {
        let p = pool_at("ubuntu", NOW);
        assert_eq!(p.len(), POOL_SIZE);

        assert_eq!(p[0].name, "ubuntu [720p] - S01E01 [TV]");
        assert_eq!(p[0].size, Some(50_263_070_484));
        assert_eq!(p[0].seeders, Some(1983));
        assert_eq!(p[0].leechers, Some(1246));
        assert_eq!(p[0].category.as_deref(), Some("TV"));
        assert_eq!(
            p[0].info_hash.as_deref(),
            Some("ffb92c9167cb2c4a3bd6896fe88b0afcbbc298c9")
        );

        assert_eq!(p[1].name, "ubuntu COMPLETE - S01E16 [Movies]");
        assert_eq!(p[1].size, Some(27_669_761_040));
        assert_eq!(p[1].seeders, Some(1327));

        assert_eq!(p[19].name, "ubuntu COLLECTION - S01E06 [Games]");
        assert_eq!(p[19].size, Some(12_170_077_176));
        assert_eq!(p[19].leechers, Some(60));
    }

    /// 确定性 = 它的全部价值所在（UI 出问题 vs 站点出问题 的判别器）。
    #[test]
    fn the_pool_is_deterministic_for_a_query() {
        assert_eq!(names("ubuntu"), names("ubuntu"));
        // 不同 query ⇒ 不同池子（种子不同）
        assert_ne!(names("ubuntu"), names("demo"));
    }

    /// ⚠️ 中文 query 走的是 **UTF-16 code unit** 那条路 —— 这是最容易写错的一处。
    /// 期望值同样来自 node（`hashStr` 对 '中文关键词' 的结果）。
    #[test]
    fn a_cjk_query_matches_the_js_hash() {
        let p = pool_at("中文关键词", NOW);
        assert_eq!(p[0].name, "中文关键词 [WEB-DL] - S01E15 [Games]");
        assert_eq!(p[0].size, Some(8_854_606_596));
        assert_eq!(p[0].seeders, Some(929));
        assert_eq!(
            p[0].info_hash.as_deref(),
            Some("83ddb3201b19e650e514e72868850728bbd39f9d")
        );
    }

    /// 空 query：**种子取 'demo'**，但名字里用原始的（空）query ⇒ 前导空格。
    /// 这是照抄 JS 的行为（`query || 'demo'` 只影响种子，不影响名字模板）。
    #[test]
    fn an_empty_query_seeds_like_demo_but_keeps_the_raw_name() {
        assert_eq!(
            names("")[0],
            " [720p] - S01E02 [Games]",
            "种子取 'demo' ⇒ 与 query='demo' 同一批数字，但名字用空 query"
        );
        // 同一批随机数 ⇒ 第 0 条的 infoHash 与 'demo' 的相同
        assert_eq!(
            pool_at("", NOW)[0].info_hash,
            pool_at("demo", NOW)[0].info_hash
        );
    }

    /// 日期：`now_ms - age_days * 一天`，毫秒。JS 的 demo 第 0 条 age_days = 686。
    #[test]
    fn dates_are_milliseconds_derived_from_the_injected_now() {
        let p = pool_at("ubuntu", NOW);
        assert_eq!(p[0].date, Some(1_740_729_600_000), "NOW - 686 天");
        assert!(
            !p[0].date_text.is_empty(),
            "date_text 要有东西（不断言具体值：受时区影响）"
        );
        // 都在"现在"之前
        assert!(p.iter().all(|r| r.date.unwrap_or(0) <= NOW));
    }

    /// magnet 的 `dn=` 走 `encodeURIComponent` 语义（空格 = %20，不是 +）。
    #[test]
    fn the_magnet_dn_is_uri_encoded_like_js() {
        let p = pool_at("ubuntu", NOW);
        let m = p[0].magnet.as_deref().unwrap();
        assert!(m.starts_with("magnet:?xt=urn:btih:ffb92c9167cb2c4a3bd6896fe88b0afcbbc298c9&dn="));
        assert!(
            m.contains("ubuntu%20%5B720p%5D%20-%20S01E01%20%5BTV%5D"),
            "空格⇒%20、[⇒%5B、]⇒%5D：{m}"
        );
        // id 走 normalize 的兜底：`demo:<infoHash>`
        assert_eq!(p[0].id, "demo:ffb92c9167cb2c4a3bd6896fe88b0afcbbc298c9");
    }

    /// 分页：每页 20，共 6 页；第 6 页之后 `hasMore = false`。
    ///
    /// ⚠️ 期望值来自 node 的**实测**（page6 是最后一页 ⇒ hasMore=false；page7 是 0 条）。
    #[test]
    fn pagination_matches_js() {
        let pool = pool_at("ubuntu", NOW);

        let (p1, more1) = slice_page(&pool, 1);
        assert_eq!(p1.len(), PER_PAGE);
        assert!(more1);
        assert_eq!(p1[0].name, "ubuntu [720p] - S01E01 [TV]");

        let (p2, more2) = slice_page(&pool, 2);
        assert_eq!(p2.len(), PER_PAGE);
        assert!(more2);
        assert_eq!(p2[0].name, "ubuntu EXTENDED - S01E06 [Movies]");
        assert_eq!(
            p2[0].size,
            Some(13_882_762_656),
            "第 2 页第 1 条（node 实测）"
        );

        // 120 条 ÷ 20 = 6 页；第 6 页是最后一页（100+20 == 120 ⇒ 不再有下一页）
        let (p6, more6) = slice_page(&pool, 6);
        assert_eq!(p6.len(), PER_PAGE);
        assert!(!more6, "第 6 页是最后一页");

        let (p7, more7) = slice_page(&pool, 7);
        assert!(p7.is_empty(), "第 7 页没有东西");
        assert!(!more7);

        // page = 0 与 page = 1 同义（JS 的默认值是 1，不会传 0，但别 panic）
        assert_eq!(slice_page(&pool, 0).0.len(), PER_PAGE);
    }

    #[test]
    fn metadata_matches_the_js_registry_entry() {
        let d = Demo;
        assert_eq!(d.id(), "demo");
        assert_eq!(d.name(), "Demo (offline)");
        assert!(d.enabled(), "index.js:120 的 enabled: true");
        assert!(d.browseable(), "index.js:120 的 browseable: true");
        assert!(d.demo(), "demo 是 page>1 那个豁免分支");
        assert!(
            !d.paginated(),
            "⚠️ JS 没有给 demo 设 paginated —— 照抄，别顺手'修正'"
        );
    }
}
