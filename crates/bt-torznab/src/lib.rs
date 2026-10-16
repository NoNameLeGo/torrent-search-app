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
/// ⚠️ **`Err` 只在 XML 本身不合法时返回**；某一项缺字段**不是错误**
/// （那是常态 —— 见 fixture 的第 3 个 item），走各自的兜底。
pub fn parse_items(xml: &str) -> Result<Vec<RawItem>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("XML 解析失败：{e}"))?;
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
    let attr = |name: &str| -> Option<String> {
        item.children()
            .filter(|c| c.is_element() && c.tag_name().name() == "attr")
            .find(|c| c.attribute("name") == Some(name))
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
/// ⚠️ 这里用 `to_lowercase()` 找位置、再回原串按**同一个字节偏移**截取。
/// 这在 `to_lowercase` 改变字节长度时（个别 Unicode 字符会，如 U+0130）会有偏移错位 ——
/// 但 magnet 是 RFC 规定的 ASCII URI，而且 `xt=urn:btih:` 在**最前面**，
/// 后面才可能出现中文（`&dn=中文`），所以偏移不受影响。
/// 真要较真就换成手写的 ASCII 大小写不敏感查找（不经 `to_lowercase`）。
fn btih(magnet: &str) -> Option<String> {
    let lower = magnet.to_lowercase();
    let idx = lower.find("btih:")? + "btih:".len();
    let hex: String = magnet[idx..]
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
        // link 里的 &amp; 也要解码
        assert!(it.magnet.contains("&dn=Rock"), "{}", it.magnet);

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
}
