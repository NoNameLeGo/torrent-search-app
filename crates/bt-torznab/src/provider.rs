//! Torznab 的**请求层 + `Provider` 实现**（⑤ 的第 1、2 步）。
//!
//! 逐条对照 `src/providers/torznab.js`（133 行）：
//!   · [`normalize_api_url`] ← `normalizeApiUrl`（L19-27）
//!   · [`build_search_url`]  ← `buildSearchUrl`（L30-39）
//!   · [`TorznabProvider::search`] ← `searchOn`（L110-120）
//!   · [`TorznabProvider::new`]    ← `makeProvider`（L123-131）
//!
//! ## ⚠️ `id()` 必须是 `&'static str` —— 于是这里 `Box::leak` 一次
//!
//! `bt_providers::Provider::id()` 返回 `&'static str`（16 个站点都是编译期字面量），
//! 而 Torznab 的 id 来自用户配置（`torznab:我的索引器`）—— 运行期字符串。
//!
//! 两条路：
//!  ① 把 trait 改成 `&str`（返回借用 `self` 的串）—— **正确**，但要动 trait、
//!     `registry::list()` 的条目类型、以及所有**存下** `p.id()` 的地方；
//!  ② 把配置串 `Box::leak` 成 `&'static str` —— 零改动。
//!
//! 选了 ②。⚠️ **但"每个索引器一次"这句已经不准确了**：`provider_set()` 是
//! **每次搜索现造一份**（#4 的工厂改造，为了让 Torznab 能动态追加），
//! 所以真实代价是「**每搜索一次、每索引器 2 个短串**」，进程生命周期内不释放。
//! 量级上不用担心（3 个索引器 × 搜 100 次 ≈ 20KB），但它是**真泄漏**、不是常量 ——
//! 真到在意的规模（几十个索引器 + 长跑）就该走 ①。

use std::time::Duration;

use bt_core::http::{pick_ua, HttpClient, ReqOpts};
use bt_core::normalize::{normalize, NumOrText, RawResult, TorrentResult};
use bt_providers::{BoxFuture, Provider, SearchOutcome};

use crate::{parse_items, RawItem};

/// 用户存下来的一个索引器配置（对齐 JS `torznabStore` 里的形状）。
///
/// ⚠️ **没有 `#[derive(Default)]`** —— 见下面的手写 impl：`enabled` 必须是 `true`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexerConfig {
    /// 不填时用 `torznab:<name>`（照 JS `makeProvider`）
    pub id: String,
    pub name: String,
    /// 用户粘进来的那个 URL（可能带 query、带尾斜杠 —— [`normalize_api_url`] 会收拾）
    pub url: String,
    pub api_key: String,
    /// 对齐 JS 的 `enabled !== false` ⇒ **默认启用**
    pub enabled: bool,
}

/// ⚠️ **手写而不是 derive**：派生出来的 `bool` 默认是 `false`，
/// 那会让"没填 enabled 的索引器"**静默变哑巴** —— 而 JS 的语义是 `enabled !== false`。
///
/// 这个坑我在**配置文件**那层已经修过一次（`serde(default = "default_true")`），
/// 却漏了类型自己的 `Default` ⇒ CI 上 `metadata_defaults_match_make_provider` 红了一轮。
/// 同一个坑的两半：**序列化默认值**与**构造默认值**得一起改。
impl Default for IndexerConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            url: String::new(),
            api_key: String::new(),
            enabled: true,
        }
    }
}

/// 把用户给的 URL 收拾成 API 端点（**照 JS `normalizeApiUrl` 逐条**）：
///   1. 去掉 `?` 之后的一切
///   2. 去掉结尾的斜杠
///   3. 结尾不是 `/api`（**大小写不敏感**）就补上
///
/// 这样 Jackett/Prowlarr 里"Copy Torznab Feed"得到的整条 URL 和只填主机名都能用。
pub fn normalize_api_url(url: &str) -> String {
    // ⚠️ JS 的第一行是 `if (!url) return ''` —— 判的是**未 trim** 的原始值。
    // 所以：`''` ⇒ 返回空串；而 `'   '` 是**真值** ⇒ 会一路走到 '/api'。
    // 漏掉这个 guard 就会让空串变成 '/api'（一个会发出去的错误 URL）。
    if url.is_empty() {
        return String::new();
    }
    let mut u = url.trim().to_string();
    if let Some(q) = u.find('?') {
        u.truncate(q);
    }
    while u.ends_with('/') {
        u.pop();
    }
    /// `/\/api$/i`
    ///
    /// ⚠️ **不能裸切字节**：`u[u.len() - 4..]` 在多字节字符中间会 **panic**
    ///（用户填 `http://localhost:9117/中文` 就中招，而且每次搜索都复发）。
    let ends_with_api = u
        .get(u.len().saturating_sub(4)..)
        .is_some_and(|t| t.eq_ignore_ascii_case("/api"));
    if !ends_with_api {
        u.push_str("/api");
    }
    u
}

/// 从「Copy Torznab Feed」那种整条 URL 里抠出 `apikey`。
///
/// ## 为什么这是**应用层的便利**，而不是改 [`normalize_api_url`]
///
/// [`normalize_api_url`] 会**刻意**丢掉整个 query —— 那是忠实复刻 JS `normalizeApiUrl`
/// 的既有行为（差分口径的一部分），改它等于改口径。
/// 但用户从 Jackett/Prowlarr 复制的链接**带着** `?apikey=…`，丢了就只能手填 ⇒
/// 这个函数只负责"省一步"，由 UI 在 **key 框空着**的时候调用。
///
/// ⚠️ 只认 `apikey`（大小写不敏感）、**不做百分号解码**：那些 key 是字母数字，
/// 真遇到 `%xx` 的用户会自己填一遍。空值返回 `None` —— "有 key 参数但为空"不是有效 key。
pub fn extract_api_key(url: &str) -> Option<String> {
    // 先砍掉 fragment（`#…` 不是 query 的一部分），再取 `?` 之后
    let q = url.split('#').next()?.split_once('?')?.1;
    for pair in q.split('&') {
        // ⚠️ 用 `if let` 而不是 `split_once('=')?` —— 后者遇到一个没有 `=` 的段
        // （例如 `?apikey=K&flag`）会**整个提前返回 None**，把已经找到的 key 丢掉。
        if let Some((k, v)) = pair.split_once('=') {
            if k.eq_ignore_ascii_case("apikey") && !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// JS `URLSearchParams` 的序列化：`A-Za-z0-9*-._` 原样，**空格变 `+`**，其余 `%XX`（大写）。
///
/// ⚠️ 这与 `encodeURIComponent` **不同**（那个是空格变 `%20`、且不转义 `!'()~`）。
/// Torznab 这边用的是 `URLSearchParams`，所以是前者 —— demo 那边的 magnet 用的是后者。
fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '*' | '-' | '.' | '_') {
            out.push(c);
        } else if c == ' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 拼搜索 URL —— 参数顺序与 JS 一致（`apikey` / `extended` / `t` / `q`）。
///
/// ⚠️ JS 的 Torznab **不带分页参数**（没有 `offset`/`limit`）⇒ 这个 provider 是
/// `paginated() == false`，翻页时聚合层会把它筛掉（见 `Provider::paginated` 的说明）。
pub fn build_search_url(base_url: &str, api_key: &str, query: &str) -> String {
    let api = normalize_api_url(base_url);
    format!(
        "{api}?apikey={}&extended=1&t=search&q={}",
        urlencoded(api_key),
        urlencoded(query)
    )
}

/// `RawItem` → `TorrentResult`（走全项目统一的 `normalize`，与 JS 同一条路）。
///
/// ⚠️ 两处必须原样传下去、**不能"顺手修"**：
///   · `seeders`/`leechers` 的 `0` —— 用 `Num(0)`，别当 falsy 丢掉（JS 那边踩过）
///   · `date` 传**字符串**（`pubDate` 原文）—— 解析交给 `normalize`
pub fn to_result(provider_id: &str, item: RawItem) -> TorrentResult {
    normalize(&RawResult {
        provider: provider_id.to_string(),
        // JS 不传 id ⇒ 交给 normalize 用 `provider:infoHash` 兜底
        id: None,
        name: Some(item.title),
        info_hash: item.info_hash,
        // JS 传的是 `''`（没兜到 magnet）⇒ 这里给 None，normalize 会尝试用 infoHash 造
        magnet: Some(item.magnet).filter(|m| !m.is_empty()),
        size: item.size_bytes.map(NumOrText::Num),
        seeders: item.seeders.map(NumOrText::Num),
        leechers: item.leechers.map(NumOrText::Num),
        date: item.pub_date.map(NumOrText::Text),
        category: item.category,
        detail_url: item.detail_url,
        files: None,
    })
}

/// 一个 Torznab 索引器，当成普通 [`Provider`] 用。
#[derive(Debug, Clone)]
pub struct TorznabProvider {
    cfg: IndexerConfig,
    /// `Box::leak` 出来的静态串（见文件头的说明）
    id: &'static str,
    name: &'static str,
}

impl TorznabProvider {
    /// 照 JS `makeProvider`：id 缺省为 `torznab:<name>`，name 缺省为 `Torznab`，
    /// `enabled` 是"不是显式 false"。
    pub fn new(cfg: IndexerConfig) -> Self {
        let id: &'static str = Box::leak(
            if cfg.id.is_empty() {
                format!("torznab:{}", cfg.name)
            } else {
                cfg.id.clone()
            }
            .into_boxed_str(),
        );
        let name: &'static str = Box::leak(
            if cfg.name.is_empty() {
                "Torznab".to_string()
            } else {
                cfg.name.clone()
            }
            .into_boxed_str(),
        );
        Self { cfg, id, name }
    }

    /// 配置的只读访问（UI/持久化要用）。
    pub fn config(&self) -> &IndexerConfig {
        &self.cfg
    }

    /// **可离线测的那一半**：给定响应体，产出结果。
    /// `search()` 只是"发请求 + 调它"。
    pub fn parse_response(&self, xml: &str) -> Result<Vec<TorrentResult>, String> {
        let items = parse_items(xml)?;
        Ok(items.into_iter().map(|i| to_result(self.id, i)).collect())
    }
}

impl Provider for TorznabProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    /// ⚠️ **false** —— JS 的 Torznab 请求里根本没有分页参数（见 `build_search_url`）。
    /// 说 true 会让聚合层在第 2 页重复请求并拿到同一批结果，再被去重掉。
    fn paginated(&self) -> bool {
        false
    }

    fn search<'a>(
        &'a self,
        http: &'a HttpClient,
        query: &'a str,
        _page: u32,
    ) -> BoxFuture<'a, SearchOutcome> {
        Box::pin(async move {
            let url = build_search_url(&self.cfg.url, &self.cfg.api_key, query);
            // JS: `getText(url, { headers: { 'User-Agent': pickUA() } })`
            // 超时与 `http.js` 的默认值一致（客户端默认也是 10s，显式写出来是为了
            // "看得见" —— 索引器可能挂得很慢）。
            let opts = ReqOpts::default()
                .header("User-Agent", pick_ua())
                .timeout(DEFAULT_TIMEOUT);
            let resp = http.get_text(&url, Some(&opts)).await;

            // JS: `if (error || !html) return { results: [], error: error || 'empty_response' }`
            if let Some(e) = resp.error {
                return SearchOutcome::err(e);
            }
            let Some(xml) = resp.html else {
                return SearchOutcome::err("empty_response");
            };

            // JS 把 parseItems 包在 try/catch 里，抛了就返回 `e.message || 'parse_failed'`。
            // 这边 `parse_items` 返回 `Err(人话)`，语义相同（文案不同是**有意的**：
            // 我们的错误串是给用户看的）。
            match self.parse_response(&xml) {
                Ok(results) => SearchOutcome::ok(results),
                Err(e) => SearchOutcome::err(e),
            }
        })
    }
}

/// 给索引器一个默认超时（JS 用 `http.js` 的 10s 默认值）。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(test)]
mod tests {
    use super::*;

    // ---- normalize_api_url：逐条对照 JS ----

    #[test]
    fn normalize_api_url_drops_query_and_trailing_slashes() {
        // Jackett "Copy Torznab Feed" 给的就是这种带 query 的整条 URL
        assert_eq!(
            normalize_api_url(
                "http://host:9117/api/v2.0/indexers/all/results/torznab/api?apikey=abc"
            ),
            "http://host:9117/api/v2.0/indexers/all/results/torznab/api"
        );
        assert_eq!(
            normalize_api_url("http://host:9117/"),
            "http://host:9117/api"
        );
        assert_eq!(
            normalize_api_url("http://host:9117///"),
            "http://host:9117/api"
        );
        assert_eq!(
            normalize_api_url("http://host:9117"),
            "http://host:9117/api"
        );
        // ⚠️ 收尾多字节字符：旧版裸切 `u[len-4..]` 会落在这两个字符的续字节上 ⇒ **panic**
        assert_eq!(
            normalize_api_url("http://host:9117/中文"),
            "http://host:9117/中文/api"
        );
    }

    #[test]
    fn normalize_api_url_is_case_insensitive_about_the_api_suffix() {
        // JS 是 `/\/api$/i` —— 已经是 /API 就**不要**再补一次
        assert_eq!(normalize_api_url("http://host/API"), "http://host/API");
        assert_eq!(normalize_api_url("http://host/Api"), "http://host/Api");
        // 但它只认**结尾**
        assert_eq!(
            normalize_api_url("http://host/api/v2"),
            "http://host/api/v2/api"
        );
    }

    #[test]
    fn normalize_api_url_handles_empty() {
        assert_eq!(normalize_api_url(""), "");
        assert_eq!(
            normalize_api_url("   "),
            "/api",
            "JS: 空串 push 之后是 '/api'"
        );
    }

    // ---- build_search_url ----

    #[test]
    fn build_search_url_matches_the_js_param_order() {
        assert_eq!(
            build_search_url("http://host", "KEY", "ubuntu"),
            "http://host/api?apikey=KEY&extended=1&t=search&q=ubuntu"
        );
    }

    /// ⚠️ `URLSearchParams` 的空格是 **`+`**（不是 `%20`）—— 和 demo 的 magnet 不一样。
    #[test]
    fn build_search_url_encodes_like_url_search_params_not_encode_uri_component() {
        assert_eq!(
            build_search_url("http://h/api", "k", "ubuntu server"),
            "http://h/api?apikey=k&extended=1&t=search&q=ubuntu+server"
        );
        // 中文 ⇒ UTF-8 百分号编码（大写）
        assert!(build_search_url("http://h/api", "k", "中文").ends_with("q=%E4%B8%AD%E6%96%87"));
        // `&` / `=` 必须转义，否则会把 URL 拆坏
        assert!(build_search_url("http://h/api", "k", "a&b=c").ends_with("q=a%26b%3Dc"));
        // 空 key 也要留 `apikey=`（JS 是 `config.apiKey || ''`）
        assert!(build_search_url("http://h/api", "", "q").contains("apikey=&extended=1"));
        // ⚠️ 这条是 **URLSearchParams 与 encodeURIComponent 的分水岭**：
        // `*` / `-` / `.` / `_` 保留，而 `~` **要**编码成 %7E（encodeURIComponent 不编码 ~）。
        // 真值来自 `scripts/torznab-url-truth.cjs`。
        assert!(build_search_url("http://h/api", "k", "a*b~c-d.e_f").ends_with("q=a*b%7Ec-d.e_f"));
    }

    // ---- RawItem → TorrentResult（用 torznab-search.xml 里那三条） ----

    fn fixture() -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/torznab-search.xml");
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()))
    }

    fn provider() -> TorznabProvider {
        TorznabProvider::new(IndexerConfig {
            id: "torznab:test".to_string(),
            name: "Test".to_string(),
            url: "http://host:9117".to_string(),
            api_key: "KEY".to_string(),
            enabled: true,
        })
    }

    #[test]
    fn maps_items_through_normalize() {
        let out = provider().parse_response(&fixture()).expect("应该能解析");
        assert_eq!(out.len(), 3);

        // item 0：主路径
        assert_eq!(out[0].name, "Ubuntu 24.04 LTS Desktop amd64");
        assert_eq!(out[0].size, Some(5_200_936_960));
        assert_eq!(out[0].seeders, Some(128));
        assert_eq!(out[0].leechers, Some(31), "leechers 取 attr 里的 peers");
        assert_eq!(
            out[0].info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(out[0].provider, "torznab:test");
        assert_eq!(
            out[0].id, "torznab:test:0123456789abcdef0123456789abcdef01234567",
            "JS 不传 id ⇒ normalize 用 provider:infoHash 兜底"
        );
    }

    /// ⚠️ **`seeders = 0` 是有效值** —— 用 Option 传，别在链路上当 falsy 丢掉。
    #[test]
    fn zero_seeders_survives_the_mapping() {
        let out = provider().parse_response(&fixture()).unwrap();
        assert_eq!(
            out[1].seeders,
            Some(0),
            "0 个做种是真实存在的，不是'没有数据'"
        );
        assert_eq!(out[1].leechers, Some(0));
        assert_eq!(out[1].name, "Rock & Roll \"Remastered\" 1975 FLAC");
        assert_eq!(out[1].size, Some(734_003_200));
    }

    /// 全缺字段那一条：不 panic，各项为 None，`detailUrl` 走 guid ⇒ 触发 needs_magnet。
    #[test]
    fn a_sparse_item_does_not_panic() {
        let out = provider().parse_response(&fixture()).unwrap();
        assert_eq!(out[2].name, "Edge Case No Size");
        assert_eq!(out[2].size, None);
        assert_eq!(out[2].seeders, None);
        assert_eq!(
            out[2].detail_url.as_deref(),
            Some("https://tracker.example/download/1")
        );
    }

    /// 坏 XML 是 `Err`，不是 panic —— `search()` 会把它变成 `SearchOutcome::err`。
    #[test]
    fn broken_xml_becomes_an_error() {
        assert!(provider().parse_response("<rss><channel><item>").is_err());
        // 没有 item 的响应是**合法**的（索引器就是没结果）⇒ 空 Vec，不是错误
        assert!(provider()
            .parse_response("<rss><channel></channel></rss>")
            .unwrap()
            .is_empty());
    }

    // ---- 元数据照 JS `makeProvider` ----

    #[test]
    fn metadata_defaults_match_make_provider() {
        let p = TorznabProvider::new(IndexerConfig {
            name: "My Indexer".to_string(),
            ..Default::default()
        });
        assert_eq!(p.id(), "torznab:My Indexer", "id 缺省 = torznab:<name>");
        assert_eq!(p.name(), "My Indexer");
        assert!(p.enabled(), "⚠️ `enabled !== false` ⇒ 默认是 true");

        let unnamed = TorznabProvider::new(IndexerConfig::default());
        assert_eq!(unnamed.name(), "Torznab", "name 缺省 = Torznab");
    }

    #[test]
    fn an_explicitly_disabled_indexer_stays_disabled() {
        let p = TorznabProvider::new(IndexerConfig {
            name: "off".to_string(),
            enabled: false,
            ..Default::default()
        });
        assert!(!p.enabled());
    }

    /// ⚠️ JS 的 Torznab 请求里没有分页参数 ⇒ 必须说 false，否则第 2 页会重复请求。
    #[test]
    fn torznab_is_not_paginated() {
        assert!(!provider().paginated());
        assert!(!provider().browseable(), "JS 没给 torznab 设 browseable");
        assert!(!provider().demo());
    }

    // ---- 从粘贴的 feed URL 抠 apikey（UI 的便利函数，不是 port 行为）----

    #[test]
    fn extracts_the_api_key_from_a_pasted_feed_url() {
        // Jackett 的 "Copy Torznab Feed" 就长这样（整条带 apikey）
        assert_eq!(
            extract_api_key(
                "http://host:9117/api/v2.0/indexers/all/results/torznab/api?apikey=abc123&extended=1"
            ),
            Some("abc123".to_string())
        );
        // 顺序无关、后面还跟着别的参数
        assert_eq!(
            extract_api_key("http://h/api?t=search&apikey=K&q=x"),
            Some("K".to_string())
        );
        // 参数名大小写不敏感
        assert_eq!(
            extract_api_key("http://h/api?ApiKey=K"),
            Some("K".to_string())
        );
        // 值里带 `=` 也要取全（`split_once('=')` 只切第一个）
        assert_eq!(
            extract_api_key("http://h/api?apikey=a=b"),
            Some("a=b".to_string())
        );
    }

    /// ⚠️ 三种"没有有效 key"都要返回 `None` —— **别把空串当 key**：
    /// 空 key 会让请求变成 `apikey=`，站点回 401，用户看到的是"key 无效"而不是"你没填"。
    #[test]
    fn there_is_no_api_key_when_there_is_none() {
        assert_eq!(extract_api_key("http://h/api"), None, "没有 query");
        assert_eq!(
            extract_api_key("http://h/api?extended=1"),
            None,
            "没有 apikey"
        );
        assert_eq!(extract_api_key("http://h/api?apikey="), None, "key 是空的");
        // ⚠️ 这一条钉的是实现细节：**遇到没有 `=` 的段不能提前放弃**
        //（第一版写成 `split_once('=')?` 就会在这里返回 None，把前面找到的 key 丢掉）
        assert_eq!(
            extract_api_key("http://h/api?apikey=K&flag"),
            Some("K".to_string())
        );
        // fragment 不算 query
        assert_eq!(
            extract_api_key("http://h/api?apikey=K#frag"),
            Some("K".to_string())
        );
    }
}
