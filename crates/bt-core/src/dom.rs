//! HTML 解析 + CSS 选择器 —— `cheerio` 的替代层。
//!
//! 底层是 [`scraper`]（Servo 的 `html5ever` + `selectors`），浏览器级解析。
//! 这里只做一件事：**把 JS 侧用到的那几个 cheerio 习惯用法，包成语义一致的 Rust API**，
//! 免得每个 provider 各自去猜"`.text()` 到底拼不拼空格"。
//!
//! ## 与 cheerio 的对应关系
//!
//! | cheerio | 这里 |
//! |---|---|
//! | `cheerio.load(html)` | [`Dom::parse`] |
//! | `$(css)` | [`Dom::select`] |
//! | `$(el).find(css)` | [`Dom::find`] |
//! | `$(el).text()` | [`text`]（**所有后代文本节点拼接**，不是只取第一个） |
//! | `$(el).text().trim()` | [`text_trim`] |
//! | `$(el).attr('href')` | [`attr`]（**实体已解码**，`&amp;` → `&`） |
//! | `$(el).closest('tr')` | [`closest_tag`]（只支持标签名，够用） |
//!
//! ## ⚠️ 已定的三条语义约定
//!
//! 1. **选择器写错 → 空结果，不 panic**。cheerio 那边会抛异常，但我们的 provider
//!    契约是「永不抛、失败进 `error` 字段」；空结果 + 上层判 `no_results_parsed` 更可控。
//! 2. **`.text()` 拼接所有后代文本**，与 cheerio 一致（不是 `textContent` 的去空白版）。
//!    空白**原样保留**，所以调用方几乎总是要 `trim()`。
//! 3. **属性值是解码后的**。linuxtracker 的 `href` 在源码里是
//!    `index.php?page=torrent-details&amp;id=...`，两侧都解码成 `&id=`，
//!    所以 `[href*="..."]` 这类子串匹配是对解码后的值做的 —— 这点有测试钉住。
//!
//! 语义等价性由 `tests/dom_probes.rs` 对着真实 fixture 与 cheerio 逐条比对
//! （探针清单 `test/fixtures/html-probes.json`，cheerio 侧实现见 `scripts/html-probes.mjs`）。

use scraper::{ElementRef, Html, Selector};
// `select` 在 Html 上可能是固有方法，也可能是 Selectable trait 提供的；
// 多导入一个 trait 最多是个未使用告警，比编译失败划算。
#[allow(unused_imports)]
use scraper::selectable::Selectable;

/// 一份解析好的文档。持有整棵树，所有选取结果都借用它。
pub struct Dom {
    doc: Html,
}

impl Dom {
    /// 解析整篇文档。对应 cheerio 的 `cheerio.load(html)`。
    ///
    /// 用 `parse_document`（不是 fragment）：两者都会补全 `html/head/body`、
    /// 给裸 `<tr>` 补 `<tbody>`，与 parse5 的行为一致 —— 这正是 `table > tbody > tr`
    /// 这类选择器能生效的原因。
    pub fn parse(html: &str) -> Self {
        Self {
            doc: Html::parse_document(html),
        }
    }

    /// 对应 `$(css)`。文档顺序，与 cheerio 相同。
    ///
    /// 选择器解析失败时返回空集合（见文件头第 1 条约定）。
    pub fn select(&self, css: &str) -> Vec<ElementRef<'_>> {
        let Ok(sel) = Selector::parse(css) else {
            return Vec::new();
        };
        self.doc.select(&sel).collect()
    }

    /// 对应 `$(scope).find(css)` —— **只看后代，不含 scope 自身**。
    pub fn find<'a>(&self, scope: ElementRef<'a>, css: &str) -> Vec<ElementRef<'a>> {
        let Ok(sel) = Selector::parse(css) else {
            return Vec::new();
        };
        scope.select(&sel).collect()
    }

    /// 对应 `$(scope).find(css)` 的别名 —— 名字更贴 JS 的习惯读法。
    pub fn find_all<'a>(&self, scope: ElementRef<'a>, css: &str) -> Vec<ElementRef<'a>> {
        self.find(scope, css)
    }
}

/// 对应 cheerio `$(el).text()`：把**所有后代文本节点**按文档顺序拼起来。
///
/// 注意 cheerio 的 `.text()` 也是这个语义（不是「第一个文本节点」），
/// 而且不折叠空白 —— 所以想拿干净的字符串请用 [`text_trim`]。
pub fn text(el: ElementRef<'_>) -> String {
    el.text().collect()
}

/// 对应 `$(el).text().trim()`。
pub fn text_trim(el: ElementRef<'_>) -> String {
    text(el).trim().to_string()
}

/// 对应 `$(el).attr(name)`。**值已解码实体**（`&amp;` → `&`、`&#39;` → `'`）。
pub fn attr(el: ElementRef<'_>, name: &str) -> Option<String> {
    el.attr(name).map(str::to_string)
}

/// 对应 `$(el).closest(tag)` —— 从自己往上找第一个该标签名（含自己）。
///
/// 只支持**标签名**：本项目里 `closest` 只被用来找父级 `<tr>`，
/// 没必要实现完整的 CSS 匹配。哪天需要 `closest('.foo')` 再加。
pub fn closest_tag<'a>(el: ElementRef<'a>, tag: &str) -> Option<ElementRef<'a>> {
    let want = tag.to_ascii_lowercase();
    let mut cur = Some(el);
    while let Some(e) = cur {
        if e.value().name() == want {
            return Some(e);
        }
        cur = e.parent().and_then(ElementRef::wrap);
    }
    None
}

/// 取第 `i` 个（越界给 `None`），对应 JS 的 `arr[i] ? ... : ''` 习惯写法。
pub fn nth<'a>(els: &[ElementRef<'a>], i: usize) -> Option<ElementRef<'a>> {
    els.get(i).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_concatenates_all_descendant_text_like_cheerio() {
        let dom = Dom::parse("<h1 class='foo'>Hello, <i>world!</i></h1>");
        let h1 = dom.select("h1.foo");
        assert_eq!(h1.len(), 1);
        // cheerio 的 $('h1').text() 也是 "Hello, world!"（拼接所有后代文本）
        assert_eq!(text(h1[0]), "Hello, world!");
        assert_eq!(text_trim(h1[0]), "Hello, world!");
    }

    #[test]
    fn text_keeps_whitespace_verbatim() {
        let dom = Dom::parse("<td class='s'>  1.2 GB  \n </td>");
        let td = dom.select("td.s");
        assert_eq!(text(td[0]), "  1.2 GB  \n ");
        assert_eq!(text_trim(td[0]), "1.2 GB");
    }

    #[test]
    fn attrs_are_entity_decoded() {
        // linuxtracker 源码里就是 &amp; 这种写法
        let dom = Dom::parse(r#"<a href="index.php?page=torrent-details&amp;id=abc">x</a>"#);
        let a = dom.select("a");
        assert_eq!(
            attr(a[0], "href").as_deref(),
            Some("index.php?page=torrent-details&id=abc")
        );
        // 解码后子串匹配也成立
        assert_eq!(dom.select(r#"a[href*="torrent-details&id="]"#).len(), 1);
    }

    #[test]
    fn selector_matches_are_case_insensitive_for_tag_names() {
        let dom = Dom::parse("<TD class='lista'>x</TD>");
        assert_eq!(dom.select("td.lista").len(), 1);
        assert_eq!(dom.select("TD.LISTA").len(), 1, "类名也按 HTML 规则不敏感");
    }

    #[test]
    fn bare_rows_get_an_implicit_tbody() {
        let dom = Dom::parse("<table><tr><td>a</td></tr></table>");
        assert_eq!(dom.select("table > tbody > tr").len(), 1);
        assert_eq!(dom.select("table tr").len(), 1);
    }

    #[test]
    fn closest_tag_walks_up_including_self() {
        let dom = Dom::parse("<tr><td><a href='x'>n</a></td></tr>");
        let a = dom.select("a");
        let tr = closest_tag(a[0], "tr").expect("该找到 tr");
        assert_eq!(tr.value().name(), "tr");

        // 自己就是目标标签时返回自己
        let trs = dom.select("tr");
        assert_eq!(closest_tag(trs[0], "tr").map(|e| e.id()), Some(trs[0].id()));

        // 找不到就是 None，不 panic
        assert!(closest_tag(a[0], "table").is_none());
    }

    #[test]
    fn find_is_scoped_and_excludes_self() {
        let dom = Dom::parse("<tr><td>1</td><td><td>2</td></td></tr>");
        let tr = dom.select("tr");
        assert_eq!(dom.find(tr[0], "td").len(), 3);
        // scope 自身不是 td，所以不会把自己算进去
        let td = dom.select("td");
        assert_eq!(dom.find(td[0], "td").len(), 0);
    }

    #[test]
    fn nested_cells_are_returned_in_document_order() {
        let dom = Dom::parse("<tr><td>a</td><td>b</td><td>c</td></tr>");
        let tr = dom.select("tr");
        let tds = dom.find(tr[0], "td");
        let texts: Vec<String> = tds.iter().map(|e| text_trim(*e)).collect();
        assert_eq!(texts, vec!["a", "b", "c"], "顺序必须与 JS 索引一致");
        assert_eq!(nth(&tds, 1).map(|e| text_trim(e)), Some("b".to_string()));
        assert_eq!(nth(&tds, 9).map(|e| text_trim(e)), None, "越界给 None");
    }

    #[test]
    fn malformed_html_does_not_panic() {
        // linuxtracker 的侧栏就是这种没闭合 <a> 的写法
        let dom = Dom::parse("<tr><td><a href='?page=torrent-details&id=abc'>name</td></tr>");
        let a = dom.select(r#"a[href*="torrent-details"]"#);
        assert_eq!(a.len(), 1);
        assert_eq!(text_trim(a[0]), "name");
    }

    #[test]
    fn bad_selector_yields_empty_instead_of_panicking() {
        let dom = Dom::parse("<p>x</p>");
        assert!(dom.select("p >>>> broken").is_empty());
        assert!(dom.select("").is_empty());
    }
}
