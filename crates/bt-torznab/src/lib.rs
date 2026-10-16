//! Torznab 索引器的**解析层**（D17 的第一步）。
//!
//! ## 这一层只做一件事
//!
//! 一段 Torznab 的 RSS XML → 一组 [`RawItem`]（**还没** `normalize()`，也**还没**接网络）。
//! 请求与 `Provider` 实现是下一步 —— 分开是为了让"解析对不对"能单独验证。
//!
//! ## ⚠️ 为什么不能用 `bt_core::dom`（那是 HTML 解析器）
//!
//! JS 参考实现用的是 `cheerio.load(xml, { xmlMode: true })` —— **XML 模式**。
//! 而 `bt_core::dom` 底下是 `scraper`/html5ever，**没有 XML 模式**：HTML 解析器
//! 只在 foreign content（SVG/MathML）里认自闭合标签，所以
//! `<torznab:attr name="seeders" value="128" />` 的 `/` 会被**忽略** ——
//! 该元素保持"打开"，**后续兄弟节点会全部变成它的子节点**，结构就错了。
//!
//! 这一点有**实证**（不是推测）：`scripts/check-torznab-fixture.py` 用标准库的
//! `xml.etree` 解析 fixture 时，item 的直接子节点是
//! `[title, guid, link, comments, pubDate, size, enclosure, attr, attr, attr]`
//! —— 三个 attr 是 item 的**直接**子节点。
//!
//! ## 字段映射的来源
//!
//! 逐条抄自 `src/providers/torznab.js` 的 `parseItems`（那张表也在 `AGENTS.md`
//! 的「⑤ Torznab 的规格」里）。**两级/三级兜底是有意的**，别简化成"或"：
//!   · `magnet`：`enclosure@url` 是 magnet → 否则 `link` 是 magnet
//!   · `size`：`enclosure@length` → **解析不出数字**（含为空）就退回 `<size>` 元素
//!   · `infoHash`：`torznab:attr name="infohash"` → 否则从 magnet 正则 `btih:`
//!   · `detailUrl`：`comments` → `link` → `guid`（与 magnet 那条**互相独立**）

mod provider;
pub use provider::{
    build_search_url, normalize_api_url, to_result, IndexerConfig, TorznabProvider, DEFAULT_TIMEOUT,
};

use roxmltree::Node;

/// RSS 里的一个 `<item>` —— 字段名与 JS 侧的中间表示一致，方便将来对照差分。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawItem {
    pub title: String,
    /// 可能为空串（三条兜底都没命中）
    pub magnet: String,
    /// ⚠️ 用 `Option` 而不是 `0`：**"没有大小信息"与"0 字节"是两回事**
    pub size_bytes: Option<i64>,
    pub info_hash: Option<String>,
    /// ⚠️ **`Some(0)` 是有效值**（做种数为 0 的种子是真实存在的）。
    /// JS 那边踩过"把 0 当 falsy 丢掉"的坑 —— 这里用 `Option` 明确区分。
    pub seeders: Option<i64>,
    pub leechers: Option<i64>,
    pub detail_url: Option<String>,
    /// 原始 `pubDate` 串（交给 `normalize` 的日期解析，这里不解析）
    pub pub_date: Option<String>,
    pub category: Option<String>,
}

/// 解析一整份 Torznab 搜索响应。
///
/// ⚠️ **`Err` 只在 XML 本身不合法、或索引器显式报 `<error>` 时返回**；某一项缺字段**不是错误**
/// （那是常态 —— 见 fixture 的第 3 个 item），走各自的兜底。
pub fn parse_items(xml: &str) -> Result<Vec<RawItem>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("XML 解析失败：{e}"))?;

    // ⚠️ Torznab / Jackett / Prowlarr 报错**不复用 HTTP 状态码**：API key 错了也是
    // **200 + 一个 `<error code="100" description="Invalid API key"/>`**。
    // 不认它的话，"key 填错了"与"真的没结果"在 UI 上**长得一模一样**（都是 `✓ 0 结果`），
    // 用户会一直以为自己关键词没搜对 —— 这是这条分支存在的全部理由。
    if let Some(e) = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "error")
    {
        let code = e.attribute("code").unwrap_or("");
        let desc = e.attribute("description").unwrap_or("").trim();
        // `code="0"` 且无描述在部分索引器的**成功**响应里也出现 ⇒ 不算错
        if !(code == "0" && desc.is_empty()) {
            let msg = if desc.is_empty() {
                format!("code {code}")
            } else {
                desc.to_string()
            };
            return Err(format!("索引器报错：{msg}"));
        }
    }

    Ok(doc
        .descendants()
        // ⚠️ 取标签名一律用 `tag_name().name()`（**local name**，忽略命名空间前缀）。
        // **不能**用 `has_tag_name("item")`：那个带 `&str` 的版本把它当成
        // "**无命名空间**"的名字，而 Torznab 的元素大多带 `xmlns:torznab` ⇒ 匹配不上。
        // ⚠️ 也**不要**抽成 `fn local_name(n: Node<'_, '_>) -> &str` 这样的 helper：
        // 参数里是匿名 `'_` 时返回值没法与它关联 ⇒ `E0106: missing lifetime specifier`
        // （CI 就是这么红的）。内联反而干净。
        .filter(|n| n.is_element() && n.tag_name().name() == "item")
        .map(read_item)
        .collect())
}

fn read_item(item: Node<'_, '_>) -> RawItem {
    // 直接子节点里按 local name 找一个元素（Torznab 的 item 字段都是 item 的直接子节点）
    let child = |tag: &str| -> Option<Node<'_, '_>> {
        item.children()
            .find(|c| c.is_element() && c.tag_name().name() == tag)
    };
    // 元素的文本，trim 后为空就算"没有"
    let text = |tag: &str| -> Option<String> {
        child(tag)
            .and_then(|c| c.text())
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
    };
    // <torznab:attr name="X" value="Y"> → Y
    //
    // ⚠️ 两处刻意对齐 JS（`src/providers/torznab.js:59-63`）：
    //   ① `$el.find('torznab\\:attr')` 是**全后代**，不是只看直接子节点；
    //   ② 名字按 `toLowerCase()` 比 —— 把属性写成 `Seeders` 的索引器也得认。
    let attr = |name: &str| -> Option<String> {
        item.descendants()
            .filter(|c| c.is_element() && c.tag_name().name() == "attr")
            .find(|c| {
                c.attribute("name")
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .and_then(|c| c.attribute("value"))
            .map(|v| v.to_string())
    };

    // magnet 的两级兜底：enclosure@url 是 magnet → 否则 link 是 magnet
    let enc_url = child("enclosure")
        .and_then(|e| e.attribute("url"))
        .unwrap_or("");
    let mut magnet = if enc_url.starts_with("magnet:") {
        enc_url.to_string()
    } else {
        String::new()
    };
    if magnet.is_empty() {
        if let Some(link) = text("link").filter(|l| l.starts_with("magnet:")) {
            magnet = link;
        }
    }

    // size 的两级兜底：enclosure@length → **解析不出数字**就退回 <size> 元素
    let size_bytes = child("enclosure")
        .and_then(|e| e.attribute("length"))
        .and_then(parse_int)
        .or_else(|| text("size").and_then(|s| parse_int(&s)));

    // infoHash 的两级兜底：torznab:attr → magnet 里的 btih:
    let info_hash = attr("infohash").or_else(|| btih(&magnet));

    RawItem {
        title: text("title").unwrap_or_default(),
        magnet,
        size_bytes,
        info_hash,
        seeders: attr("seeders").and_then(|s| parse_int(&s)),
        // ⚠️ Torznab 把做种/下载对叫 seeders / **peers**（不叫 leechers）
        leechers: attr("peers").and_then(|s| parse_int(&s)),
        // 三级兜底，且与 magnet 那条互相独立
        detail_url: text("comments")
            .or_else(|| text("link"))
            .or_else(|| text("guid")),
        pub_date: text("pubDate"),
        category: attr("category"),
    }
}

/// JS 侧是 `parseInt(s, 10)`（宽松：`"12abc"` → 12），这里对齐。
fn parse_int(s: &str) -> Option<i64> {
    let t = s.trim();
    let mut end = 0usize;
    let bytes = t.as_bytes();
    let mut start = 0usize;
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
        start = 1;
    }
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == start {
        return None;
    }
    t[..end].parse::<i64>().ok()
}

/// 从 magnet 里抠 `btih:<hex>`（JS 用 `/btih:([a-f0-9]+)/i`，大小写不敏感）。
///
/// ⚠️ **不许用 `to_lowercase()` 找位置再回原串切**：`to_lowercase` 会改变字节长度
///（U+0130 'İ' → 3 字节），于是"副本里的下标"拿去切原串就会错位，
/// 轻则取错、重则 `magnet[idx..]` 落在字符中间 **panic**。
///（旧版就是这样，靠"`btih:` 总在最前面"的假设免祸 —— 但 `&dn=İ…&xt=urn:btih:`
/// 这种顺序完全合法，假设不成立。）手写 ASCII 大小写不敏感查找就没这个问题。
fn btih(magnet: &str) -> Option<String> {
    let needle = b"btih:";
    let at = magnet
        .as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))?
        + needle.len();
    let hex: String = magnet[at..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    if hex.is_empty() {
        None
    } else {
        Some(hex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `env!("CARGO_MANIFEST_DIR")` = `crates/bt-torznab`，fixture 在仓库根的 `test/fixtures/`。
    ///
    /// ⚠️ 与 `dump_rust` 同一个坑：`cargo test` 的 CWD 是**包根**，不是仓库根 ——
    /// 所以路径必须从 `CARGO_MANIFEST_DIR` 推，不能写相对的。
    fn fixture() -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures/torznab-search.xml");
        std::fs::read_to_string(&p)
            .unwrap_or_else(|e| panic!("读不到 fixture {}：{e}", p.display()))
    }

    fn items() -> Vec<RawItem> {
        parse_items(&fixture()).expect("fixture 应该能解析")
    }

    #[test]
    fn parses_every_item() {
        assert_eq!(items().len(), 3, "fixture 有 3 个 item");
    }

    /// item 0：标准形态 —— 每一项都走"主路径"。
    #[test]
    fn standard_item_uses_the_primary_paths() {
        let it = &items()[0];
        assert_eq!(it.title, "Ubuntu 24.04 LTS Desktop amd64");
        assert_eq!(
            it.size_bytes,
            Some(5_200_936_960),
            "size 走 enclosure@length"
        );
        assert_eq!(
            it.info_hash.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567"),
            "infoHash 走 torznab:attr"
        );
        assert_eq!(it.seeders, Some(128));
        assert_eq!(it.leechers, Some(31), "leechers 取的是 attr 里的 peers");
        assert_eq!(
            it.detail_url.as_deref(),
            Some("https://tracker.example/details/12345"),
            "detailUrl 走 comments"
        );
        assert_eq!(
            it.pub_date.as_deref(),
            Some("Wed, 15 Oct 2026 09:12:00 +0000")
        );
        assert_eq!(
            it.magnet, "",
            "enclosure 是 .torrent 不是 magnet ⇒ magnet 为空"
        );
    }

    /// item 1：实体解码 + 两级兜底 + **0 是有效值**。
    #[test]
    fn entity_decoding_and_the_fallback_levels() {
        let it = &items()[1];

        // XML 实体要解码 —— 否则标题里会带字面的 &amp;
        assert_eq!(it.title, "Rock & Roll \"Remastered\" 1975 FLAC");

        // magnet 来自 enclosure@url（第一级）
        assert!(it.magnet.starts_with("magnet:"), "{}", it.magnet);
        // ⚠️ 但它**不带** `&dn=Rock` —— 那一段在 link 上，而 magnet 走的是 enclosure。
        // 第一版我在这里断言了 contains("&dn=Rock")，CI 打回来（打印出的正是这条 magnet）。
        assert!(
            !it.magnet.contains("&dn=Rock"),
            "enclosure 那一级不带 dn：{}",
            it.magnet
        );

        // ⚠️ fixture 故意**不给** infohash 属性 ⇒ 必须从 magnet 的 btih: 兜底
        assert_eq!(
            it.info_hash.as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef01")
        );

        // ⚠️ **0 是有效值**：不能因为"0 是假的"就丢掉（JS 那边踩过）
        assert_eq!(it.seeders, Some(0));
        assert_eq!(it.leechers, Some(0));

        // ⚠️ `detailUrl` 与 `magnet` 是**两条独立的兜底链**，这里正好是证据：
        //   magnet 走第一级（enclosure@url），detailUrl 走第二级（link）
        //   ⇒ 两者**不相等** —— link 上还带着 `&dn=Rock`，enclosure 上没有。
        // 第一版我断言了 `detail_url == magnet`，CI 直接打回来。
        assert_eq!(
            it.magnet, "magnet:?xt=urn:btih:abcdef0123456789abcdef0123456789abcdef01",
            "magnet 走 enclosure（第一级）"
        );
        assert_eq!(
            it.detail_url.as_deref(),
            Some("magnet:?xt=urn:btih:abcdef0123456789abcdef0123456789abcdef01&dn=Rock"),
            "detailUrl 走 link（第二级），且 &amp; 已解码"
        );
    }

    /// item 2：全缺字段 —— **每项都是 None/空，而且不 panic**。
    #[test]
    fn a_sparse_item_is_all_none_not_an_error() {
        let it = &items()[2];
        assert_eq!(it.title, "Edge Case No Size");
        assert_eq!(it.magnet, "");
        assert_eq!(it.size_bytes, None, "没有 enclosure 也没有 size 元素");
        assert_eq!(it.info_hash, None);
        assert_eq!(it.seeders, None, "缺席 ≠ 0");
        assert_eq!(it.leechers, None);
        assert_eq!(it.pub_date, None);
        // ⚠️ 这一项**同时有 link 和 guid** ⇒ detailUrl 落在 **link**（第二级）上。
        // 第一版我写成"只能走 guid 兜底"（expected guid），CI 打回来。
        // guid 到底走不走得到，见下面那条专门的用例。
        assert_eq!(
            it.detail_url.as_deref(),
            Some("https://tracker.example/download/1"),
            "link 在 guid 之前，所以落在这里"
        );
    }

    /// **第三级兜底（guid）单独验** —— fixture 里那一项走不到它，
    /// 而"没被覆盖的那一级"正是最容易写错的地方（也是我第一版写错的地方）。
    #[test]
    fn detail_url_falls_back_to_guid_when_comments_and_link_are_absent() {
        let xml = r#"<rss><channel><item>
            <title>t</title>
            <guid isPermaLink="false">https://tracker.example/details/7</guid>
        </item></channel></rss>"#;
        assert_eq!(
            parse_items(xml).unwrap()[0].detail_url.as_deref(),
            Some("https://tracker.example/details/7")
        );
    }

    #[test]
    fn size_falls_back_to_the_size_element_when_length_is_not_a_number() {
        // enclosure@length 不是数字 ⇒ 按 JS 的 `isNaN(parseInt(...))` 退回 <size>
        let xml = r#"<rss><channel><item>
            <title>t</title><size>123456</size>
            <enclosure url="http://x/a.torrent" length="not-a-number" />
        </item></channel></rss>"#;
        let it = &parse_items(xml).unwrap()[0];
        assert_eq!(it.size_bytes, Some(123456));
    }

    /// JS 是 `parseInt(s, 10)`（宽松）—— `"12abc"` 得 12、`"-5"` 得 -5。
    #[test]
    fn parse_int_matches_js_parse_int() {
        assert_eq!(parse_int("12abc"), Some(12));
        assert_eq!(parse_int("42"), Some(42));
        assert_eq!(parse_int(" -5"), Some(-5));
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("abc"), None, "一个数字都没有 ⇒ None，不是 0");
    }

    /// ⚠️ 命名空间前缀不能把标签匹配搞坏：`<torznab:attr>` 是有命名空间的元素。
    /// 这就是不能用 `has_tag_name("attr")` 的原因（那个版本当成"无命名空间"）。
    #[test]
    fn namespaced_attr_elements_are_found_by_local_name() {
        let xml = r#"<rss xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel><item>
            <title>t</title>
            <torznab:attr name="seeders" value="7" />
        </item></channel></rss>"#;
        assert_eq!(parse_items(xml).unwrap()[0].seeders, Some(7));
    }

    #[test]
    fn broken_xml_is_an_error_not_a_panic() {
        assert!(parse_items("<rss><channel><item>").is_err());
        assert!(parse_items("").is_err());
    }

    /// 没有 item 的响应是**合法的**（索引器就是没结果）⇒ 空 Vec，不是错误。
    #[test]
    fn a_response_with_no_items_is_ok() {
        assert_eq!(
            parse_items("<rss><channel></channel></rss>").unwrap().len(),
            0
        );
    }

    /// ⚠️ 这就是那个最阴的分支：Torznab 报错也是 **HTTP 200**。
    /// 不认 `<error>` 的话，`parse_items` 会当"合法但没结果" ⇒ 空 Vec，
    /// 于是 UI 上"API key 填错了"与"真的搜不到"显示完全一样（都是 `✓ 0 结果`）。
    #[test]
    fn an_error_response_is_an_error_not_an_empty_ok() {
        let err = parse_items(r#"<error code="100" description="Invalid API key" />"#)
            .expect_err("错 key 必须是 Err，不能是空 Vec");
        assert!(
            err.contains("Invalid API key"),
            "错误串要带上索引器原话：{err}"
        );

        // 嵌套在 RSS 里的形态（部分索引器）同样要认
        let nested = parse_items(
            r#"<rss><channel><error code="201" description="Insufficient credits" /></channel></rss>"#,
        )
        .expect_err("RSS 里嵌的 <error> 也要认");
        assert!(nested.contains("Insufficient credits"), "{nested}");
    }

    /// 反过来：`code="0"` + 空描述在部分索引器的**成功**响应里也会出现 ⇒ 不能当错。
    #[test]
    fn a_zero_code_without_description_is_not_an_error() {
        let xml = r#"<rss><channel><error code="0" description="" /><item><title>t</title></item></channel></rss>"#;
        assert_eq!(parse_items(xml).unwrap().len(), 1);
    }

    /// ⚠️ 旧版用 `to_lowercase()` 找下标、再拿它切**原串** —— `to_lowercase` 会改变字节
    /// 长度（`İ` U+0130 → 3 字节），于是下标错位、`magnet[idx..]` 可落在字符中间 panic。
    /// `btih:` 不在最前面时也得找得到、且大小写不敏感。
    #[test]
    fn btih_survives_multibyte_prefix_and_uppercase_needle() {
        let m = "magnet:?dn=İstanbul&xt=URN:BTIH:ABCDEF0123456789ABCDEF0123456789ABCDEF01";
        assert_eq!(
            btih(m).as_deref(),
            Some("ABCDEF0123456789ABCDEF0123456789ABCDEF01"),
            "大小写与位置都不能影响（JS 的正则带 /i，且不锚定）"
        );
        assert_eq!(btih("magnet:?dn=nohash"), None);
        assert_eq!(btih("magnet:?xt=urn:btih:zz"), None, "非 hex ⇒ None");
    }
}
