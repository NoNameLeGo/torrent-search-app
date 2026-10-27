//! **Cardigann**（Prowlarr / Jackett 的站点定义 YAML）解析与执行。
//!
//! ## 为什么值得做
//!
//! Prowlarr 维护着几百个索引器的**声明式定义**：一个 YAML 就说清了「搜索 URL 怎么拼、
//! 结果行怎么取、每列用哪个选择器」。本项目现在是**一站点一 `.rs` 文件**（28 家），
//! 接上这套定义等于**一个解析器覆盖上百个站**。
//!
//! 数据来源（详见 `docs/DATA-SOURCES.md` §2）：
//!
//! ```text
//! 清单：https://indexers.prowlarr.com/master/<VER>       ← JSON，577 条
//! 单站：https://indexers.prowlarr.com/master/<VER>/<id>  ← 该站的 YAML
//! ```
//!
//! ## ⚠️ 这是**子集**实现，不是完整 Cardigann
//!
//! 只做 `type == public` + `search` 段。**不做**：登录（`login`）、分类映射（`caps`）、
//! 分页模板、filter 全集、`download` 的 `before`/`after`、站点自定义 `headers`。
//!
//! 支持的 filter 白名单（实测出现频次最高的几个）：
//! `replace` / `re_replace` / `append` / `prepend` / `trim` / `split` / `regexp`。
//! 其余 filter（`dateparse` / `timeparse` / `andmatch` / `sort` / `type` …）
//! **原样跳过** —— 宁可留下脏文本，也不要因为一个不认识的 filter 丢掉整站。
//!
//! ## `:has()` 的降级
//!
//! 真实定义里 `rows.selector` 会写成 `table tbody tr:has(a[href^="magnet:?"])`。
//! `bt_core::dom` 底层是 `scraper`，`:has` 支持情况不确定（本机不能编译，无法实测）。
//! 所以这里**自己把它拆掉**：`:has(X)` 从选择器里移除，改成「取到行之后，再用 X 在本行内
//! 查一次，非空才留下」。语义等价，且不依赖 `scraper` 的能力。
//!
//! ## 契约（与其它 provider 一致）
//!
//! **永不 panic、永不返回 `Err`**：失败落在 [`SearchOutcome::error`]。

use std::collections::{BTreeMap, HashSet};

use bt_core::dom::{attr, text_trim, Dom, ElementRef};
use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, normalize, NumOrText, RawResult};
use bt_core::TorrentResult;
use serde::Deserialize;
use serde_yaml::Value as Yaml;

use crate::SearchOutcome;

// ---------------------------------------------------------------------------
// 清单
// ---------------------------------------------------------------------------

/// 清单里的一条（只保留 `type == "public"`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    /// 稳定 id（也是取 YAML 的文件名）。
    pub id: String,
    pub name: String,
    /// 语言标签，如 `zh-CN` / `en-US`。
    pub language: String,
    /// 站点主页（`links[]`）。搜索 URL 以它做 base。
    pub links: Vec<String>,
}

/// 解析 Prowlarr 的清单 JSON。
///
/// 只留 `type == "public"` —— `private` 要账号、`semi-private` 要 API key，
/// 两者都得先做凭据体系（`docs/DATA-SOURCES.md` §2.5 已记）。
pub fn parse_catalog(json: &str) -> Result<Vec<CatalogEntry>, String> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("bad_json: {e}"))?;
    let arr = root
        .get("indexers")
        .and_then(serde_json::Value::as_array)
        // 清单也可能是裸数组（旧格式）—— 两种都认
        .or_else(|| root.as_array())
        .ok_or_else(|| "no_indexers".to_string())?;

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for it in arr {
        if it.get("type").and_then(serde_json::Value::as_str) != Some("public") {
            continue;
        }
        let id = it
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let name = it
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&id)
            .to_string();
        let language = it
            .get("language")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let links: Vec<String> = it
            .get("links")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        // 没有可用 base 的条目推不出搜索 URL，不留。
        if links.is_empty() {
            continue;
        }

        out.push(CatalogEntry {
            id,
            name,
            language,
            links,
        });
    }

    Ok(out)
}

/// 清单 URL。`ver` 从 `VERSIONS` 文件读，别硬编码（`docs/DATA-SOURCES.md` §2.1）。
pub fn catalog_url(ver: &str) -> String {
    format!("https://indexers.prowlarr.com/master/{ver}")
}

/// 单个定义的 URL。
pub fn definition_url(ver: &str, id: &str) -> String {
    format!("https://indexers.prowlarr.com/master/{ver}/{id}")
}

// ---------------------------------------------------------------------------
// 定义（YAML）
// ---------------------------------------------------------------------------

/// 一个站点的定义。字段几乎全部可选 —— 定义文件的版本差异很大，缺什么在执行时降级。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Definition {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub links: Vec<String>,
    pub search: Option<RawSearch>,
    /// `login` 段存在 = 要账号，直接不支持。
    #[serde(default)]
    pub login: Option<Yaml>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawSearch {
    #[serde(default)]
    pub paths: Vec<RawPath>,
    #[serde(default)]
    pub inputs: BTreeMap<String, Yaml>,
    pub rows: Option<RawRows>,
    #[serde(default)]
    pub fields: BTreeMap<String, RawField>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawPath {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub method: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawRows {
    /// ⚠️ 从 YAML 里读出来是**含 `:has()`** 的原始串，执行前要过 [`strip_has`]。
    #[serde(default)]
    pub selector: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawField {
    pub selector: Option<String>,
    pub attribute: Option<String>,
    #[serde(default)]
    pub filters: Vec<RawFilter>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawFilter {
    pub name: String,
    #[serde(default)]
    pub args: Option<Yaml>,
}

/// 解析一份定义 YAML。
pub fn parse_definition(yaml: &str) -> Result<Definition, String> {
    serde_yaml::from_str::<Definition>(yaml).map_err(|e| format!("bad_yaml: {e}"))
}

/// 这份定义能不能跑（不能跑就别拉它，省一次请求）。
pub fn is_supported(def: &Definition) -> bool {
    if def.login.is_some() {
        return false;
    }
    let Some(search) = &def.search else {
        return false;
    };
    let Some(path) = search.paths.first() else {
        return false;
    };
    if path.path.trim().is_empty() {
        return false;
    }
    if path.method.as_deref().unwrap_or("get").to_ascii_lowercase() != "get" {
        return false;
    }
    search
        .rows
        .as_ref()
        .is_some_and(|r| !r.selector.trim().is_empty())
        && !search.fields.is_empty()
}

// ---------------------------------------------------------------------------
// 选择器
// ---------------------------------------------------------------------------

/// 把 `:has(...)` 从选择器里剥掉，返回 `(基础选择器, 行内必须命中的选择器列表)`。
///
/// 括号要**配对扫描**：`tr:has(a[href^="magnet:?"])` 里的 `)` 才是结尾，
/// 用 `find(')')` 会在 `:not(:has(x))` 这类嵌套上截错。
pub fn strip_has(selector: &str) -> (String, Vec<String>) {
    let chars: Vec<char> = selector.chars().collect();
    let mut base = String::with_capacity(selector.len());
    let mut inners = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let is_has = chars[i] == ':'
            && i + 4 < chars.len()
            && chars[i + 1] == 'h'
            && chars[i + 2] == 'a'
            && chars[i + 3] == 's'
            && chars[i + 4] == '(';

        if is_has {
            let start = i + 5;
            let mut depth = 1usize;
            let mut j = start;
            while j < chars.len() {
                match chars[j] {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if depth == 0 {
                let inner: String = chars[start..j].iter().collect();
                if !inner.trim().is_empty() {
                    inners.push(inner.trim().to_string());
                }
                i = j + 1;
                continue;
            }
            // 括号没闭合：原样保留，别把选择器弄坏
        }

        base.push(chars[i]);
        i += 1;
    }

    (base.trim().to_string(), inners)
}

// ---------------------------------------------------------------------------
// URL 拼装
// ---------------------------------------------------------------------------

/// 拼搜索 URL。`inputs` 里的 `{{ .Keywords }}` / `{{ .Query.Keywords }}` 用编码后的关键词替换。
///
/// 含**其它**未解析占位符的 input 直接丢掉：那些是分类 id / 排序之类的站点专属参数，
/// 猜一个值反而可能让结果变空。
pub fn search_url(def: &Definition, query: &str, page: u32) -> Option<String> {
    let base = def.links.first()?.trim_end_matches('/').to_string();
    let search = def.search.as_ref()?;
    let path = search.paths.first()?.path.trim();
    if path.is_empty() {
        return None;
    }

    let mut url = if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        format!("{base}/{}", path.trim_start_matches('/'))
    };

    let kw = encode_uri_component(query.trim());
    let page_s = page.to_string();
    let mut sep = if url.contains('?') { '&' } else { '?' };

    for (k, v) in &search.inputs {
        let raw = yaml_text(v);
        let replaced = raw
            .replace("{{ .Query.Keywords }}", &kw)
            .replace("{{ .Keywords }}", &kw)
            .replace("{{ .Query.Page }}", &page_s)
            .replace("{{ .Page }}", &page_s);
        if replaced.contains("{{") || replaced.trim().is_empty() {
            continue;
        }
        url.push(sep);
        sep = '&';
        url.push_str(&encode_uri_component(k));
        url.push('=');
        url.push_str(&encode_uri_component(replaced.trim()));
    }

    Some(url)
}

// ---------------------------------------------------------------------------
// 结果解析
// ---------------------------------------------------------------------------

/// 从一页 HTML 里取结果。**纯函数**，所以能离线单测。
pub fn parse_rows(def: &Definition, provider: &str, html: &str, base: &str) -> Vec<TorrentResult> {
    let Some(search) = &def.search else {
        return Vec::new();
    };
    let Some(rows_def) = &search.rows else {
        return Vec::new();
    };

    let (row_sel, has_filters) = strip_has(&rows_def.selector);
    if row_sel.is_empty() {
        return Vec::new();
    }

    let dom = Dom::parse(html);
    let rows = dom.select(&row_sel);

    let mut out = Vec::new();
    for row in rows {
        // `:has(...)` 的降级判定：行内必须能查到那些选择器
        if has_filters
            .iter()
            .any(|inner| dom.find(row, inner).is_empty())
        {
            continue;
        }

        let Some(title) =
            field_value(&dom, row, search.fields.get("title")).filter(|t| !t.trim().is_empty())
        else {
            continue;
        };

        let download = field_value(&dom, row, search.fields.get("download"));
        let (magnet, info_hash) = split_magnet(download.as_deref());

        out.push(normalize(&RawResult {
            provider: provider.to_string(),
            name: Some(title),
            magnet,
            info_hash,
            size: field_value(&dom, row, search.fields.get("size")).map(NumOrText::Text),
            seeders: field_value(&dom, row, search.fields.get("seeders")).map(NumOrText::Text),
            leechers: field_value(&dom, row, search.fields.get("leechers")).map(NumOrText::Text),
            date: field_value(&dom, row, search.fields.get("date")).map(NumOrText::Text),
            detail_url: field_value(&dom, row, search.fields.get("details"))
                .map(|u| absolute(base, &u)),
            category: field_value(&dom, row, search.fields.get("category")),
            ..Default::default()
        }));
    }

    out
}

/// 取一个字段的值：选择器 → 属性或文本 → filters。
fn field_value(dom: &Dom, row: ElementRef<'_>, field: Option<&RawField>) -> Option<String> {
    let field = field?;

    // 没有 selector ⇒ 用行元素本身（Cardigann 允许，`download` 常这么写）
    let el = match field.selector.as_deref().map(str::trim) {
        None | Some("") => row,
        Some(sel) => match dom.find(row, sel).into_iter().next() {
            Some(e) => e,
            None => return None,
        },
    };

    let raw = match field.attribute.as_deref().map(str::trim) {
        Some(a) if !a.is_empty() => attr(el, a)?,
        _ => text_trim(el),
    };

    let filtered = apply_filters(&raw, &field.filters);
    if filtered.trim().is_empty() {
        return None;
    }
    Some(filtered)
}

/// 磁力 → `(magnet, info_hash)`。`magnet:?xt=urn:btih:<hash>` 里的 hash 抠出来。
fn split_magnet(v: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(v) = v.map(str::trim).filter(|s| s.starts_with("magnet:")) else {
        return (None, None);
    };
    let hash = v
        .split(|c| c == '?' || c == '&')
        .find_map(|part| part.strip_prefix("xt=urn:btih:"))
        .map(|h| h.split('&').next().unwrap_or(h).to_string());
    (Some(v.to_string()), hash)
}

/// 相对链接转绝对。
fn absolute(base: &str, href: &str) -> String {
    let base = base.trim_end_matches('/');
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if href.starts_with('/') {
        format!("{base}{href}")
    } else {
        format!("{base}/{href}")
    }
}

/// filter 白名单。不认识的**原样跳过**（宁可脏一点，也别丢整站）。
pub fn apply_filters(input: &str, filters: &[RawFilter]) -> String {
    let mut cur = input.to_string();
    for f in filters {
        cur = match f.name.as_str() {
            "trim" => cur.trim().to_string(),
            "replace" => match pair(&f.args) {
                Some((from, to)) => cur.replace(&from, &to),
                None => cur,
            },
            "re_replace" => match pair(&f.args) {
                Some((from, to)) => match regex::Regex::new(&from) {
                    Ok(re) => re.replace_all(&cur, to.as_str()).to_string(),
                    Err(_) => cur,
                },
                None => cur,
            },
            "regexp" => match one(&f.args).and_then(|e| regex::Regex::new(&e).ok()) {
                Some(re) => re
                    .captures(&cur)
                    .and_then(|c| {
                        c.get(1)
                            .or_else(|| c.get(0))
                            .map(|m| m.as_str().to_string())
                    })
                    .unwrap_or_default(),
                None => cur,
            },
            "append" => match one(&f.args) {
                Some(s) => format!("{cur}{s}"),
                None => cur,
            },
            "prepend" => match one(&f.args) {
                Some(s) => format!("{s}{cur}"),
                None => cur,
            },
            "split" => match pair(&f.args) {
                Some((sep, idx)) => {
                    let want: usize = idx.trim().parse().unwrap_or(0);
                    cur.split(sep.as_str()).nth(want).unwrap_or("").to_string()
                }
                None => cur,
            },
            // 其余（dateparse / timeparse / andmatch / sort / type …）原样放行
            _ => cur,
        };
    }
    cur
}

/// filters 的「两参数」形态：`["a","b"]` 或 `{from: a, to: b}`。
fn pair(args: &Option<Yaml>) -> Option<(String, String)> {
    match args.as_ref()? {
        Yaml::Sequence(seq) if seq.len() >= 2 => Some((yaml_text(&seq[0]), yaml_text(&seq[1]))),
        Yaml::Mapping(m) => Some((
            m.get(Yaml::String("from".into())).map(yaml_text)?,
            m.get(Yaml::String("to".into())).map(yaml_text)?,
        )),
        _ => None,
    }
}

/// filters 的「单参数」形态：`"x"` / `["x"]` / `{s: x}` / `{expr: x}`。
fn one(args: &Option<Yaml>) -> Option<String> {
    match args.as_ref()? {
        Yaml::String(_) | Yaml::Number(_) | Yaml::Bool(_) => Some(yaml_text(args.as_ref()?)),
        Yaml::Sequence(seq) => seq.first().map(yaml_text),
        Yaml::Mapping(m) => m
            .get(Yaml::String("s".into()))
            .or_else(|| m.get(Yaml::String("expr".into())))
            .map(yaml_text),
        _ => None,
    }
}

fn yaml_text(v: &Yaml) -> String {
    match v {
        Yaml::String(s) => s.clone(),
        Yaml::Number(n) => n.to_string(),
        Yaml::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// 拉一次并解析。**永不 panic、永不返回 `Err`**。
pub async fn search_at(
    http: &HttpClient,
    def: &Definition,
    provider: &str,
    query: &str,
    page: u32,
) -> SearchOutcome {
    if !is_supported(def) {
        return SearchOutcome::err("unsupported_definition");
    }
    let Some(url) = search_url(def, query, page) else {
        return SearchOutcome::err("no_search_path");
    };

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return SearchOutcome::err(e);
    }
    let Some(html) = resp.html.filter(|h| !h.is_empty()) else {
        return SearchOutcome::err("no_html");
    };

    let base = def.links.first().map(String::as_str).unwrap_or("");
    SearchOutcome::ok(parse_rows(def, provider, &html, base))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DMHY: &str = r#"
type: public
links: [https://share.dmhy.org/]
search:
  paths: [{ path: topics/list }]
  inputs: { keyword: "{{ .Keywords }}", sort_id: 0, order: date-desc }
  rows: { selector: 'table tbody tr:has(a[href^="magnet:?"])' }
  fields:
    title:    { selector: 'a[href^="/topics/view/"]' }
    download: { selector: 'a[href^="magnet:?"]', attribute: href }
    size:     { selector: 'td:nth-child(5)' }
    seeders:  { selector: 'td:nth-child(6)' }
    details:  { selector: 'a[href^="/topics/view/"]', attribute: href }
"#;

    #[test]
    fn catalog_keeps_only_public_with_links() {
        let json = r#"{"indexers":[
            {"id":"dmhy","name":"dmhy","type":"public","language":"zh-TW","links":["https://share.dmhy.org/"]},
            {"id":"priv","name":"私有","type":"private","links":["https://p.example/"]},
            {"id":"nolink","name":"没链接","type":"public","links":[]},
            {"id":"dmhy","name":"重复","type":"public","links":["https://share.dmhy.org/"]},
            {"id":"bad","name":"坏协议","type":"public","links":["ftp://x/"]}
        ]}"#;
        let list = parse_catalog(json).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "dmhy");
        assert_eq!(list[0].language, "zh-TW");
        assert_eq!(list[0].links, vec!["https://share.dmhy.org/"]);

        // 裸数组也认
        assert_eq!(
            parse_catalog(r#"[{"id":"a","type":"public","links":["https://a/"]}]"#)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(parse_catalog("{}").unwrap_err(), "no_indexers");
        assert!(parse_catalog("<html>").unwrap_err().starts_with("bad_json"));
    }

    #[test]
    fn definition_parses_the_real_dmhy_shape() {
        let def = parse_definition(DMHY).unwrap();
        assert_eq!(def.kind.as_deref(), Some("public"));
        assert_eq!(def.links, vec!["https://share.dmhy.org/"]);
        let search = def.search.as_ref().unwrap();
        assert_eq!(search.paths.len(), 1);
        assert_eq!(search.paths[0].path, "topics/list");
        assert!(search.rows.as_ref().unwrap().selector.contains(":has("));
        assert_eq!(search.fields.len(), 5);
        assert!(is_supported(&def));
    }

    #[test]
    fn unsupported_definitions_are_rejected() {
        // POST 不支持
        let post = parse_definition(
            "links: [https://x/]\nsearch:\n  paths: [{path: q, method: post}]\n  rows: {selector: tr}\n  fields: {title: {selector: a}}\n",
        )
        .unwrap();
        assert!(!is_supported(&post));

        // 要登录
        let login = parse_definition(
            "login: {path: login.php}\nlinks: [https://x/]\nsearch:\n  paths: [{path: q}]\n  rows: {selector: tr}\n  fields: {title: {selector: a}}\n",
        )
        .unwrap();
        assert!(!is_supported(&login));

        // 没有 search
        let empty = parse_definition("links: [https://x/]\n").unwrap();
        assert!(!is_supported(&empty));
    }

    #[test]
    fn strip_has_removes_all_levels() {
        assert_eq!(
            strip_has(r#"table tbody tr:has(a[href^="magnet:?"])"#),
            (
                "table tbody tr".to_string(),
                vec![r#"a[href^="magnet:?"]"#.to_string()]
            )
        );
        // 嵌套括号不能提前截断
        let (base, inners) = strip_has("tr:has(td:not(:has(a)))");
        assert_eq!(base, "tr");
        assert_eq!(inners, vec!["td:not(:has(a))".to_string()]);
        // 没有 :has 就原样返回
        assert_eq!(strip_has("ul li"), ("ul li".to_string(), Vec::new()));
        // 括号没闭合时不能把选择器吃坏
        assert_eq!(strip_has("tr:has(a"), ("tr:has(a".to_string(), Vec::new()));
    }

    #[test]
    fn search_url_substitutes_keywords_and_drops_other_placeholders() {
        let def = parse_definition(DMHY).unwrap();
        let url = search_url(&def, "流浪地球", 1).unwrap();
        assert_eq!(
            url,
            "https://share.dmhy.org/topics/list?keyword=%E6%B5%81%E6%B5%AA%E5%9C%B0%E7%90%83&sort_id=0&order=date-desc"
        );
    }

    #[test]
    fn search_url_keeps_page_and_absolute_paths() {
        let def = parse_definition(
            "links: [https://x/]\nsearch:\n  paths: [{path: 'https://x/api?q=1'}]\n  inputs: {k: '{{ .Keywords }}', p: '{{ .Page }}'}\n  rows: {selector: tr}\n  fields: {title: {selector: a}}\n",
        )
        .unwrap();
        assert_eq!(
            search_url(&def, "a", 3).unwrap(),
            "https://x/api?q=1&k=a&p=3"
        );
    }

    #[test]
    fn filters_apply_the_whitelist() {
        let f = |yaml: &str| -> RawFilter { serde_yaml::from_str(yaml).unwrap() };

        assert_eq!(
            apply_filters("a-b-c", &[f("{name: replace, args: ['-', ' ']}")]),
            "a b c"
        );
        assert_eq!(apply_filters("  x  ", &[f("{name: trim}")]), "x");
        assert_eq!(
            apply_filters("720p", &[f("{name: append, args: ' HD'}")]),
            "720p HD"
        );
        assert_eq!(
            apply_filters("S01E02", &[f("{name: regexp, args: 'S(\\\\d+)E'}")]),
            "01"
        );
        assert_eq!(
            apply_filters("a1b2", &[f("{name: re_replace, args: ['[0-9]', '#']}")]),
            "a#b#"
        );
        assert_eq!(
            apply_filters("x|y|z", &[f("{name: split, args: ['|', '1']}")]),
            "y"
        );
        // 不认识的 filter 原样放行（不能丢站）
        assert_eq!(
            apply_filters("keep", &[f("{name: dateparse, args: 'yyyy-MM-dd'}")]),
            "keep"
        );
    }

    #[test]
    fn parse_rows_extracts_fields_and_respects_has_filter() {
        let def = parse_definition(DMHY).unwrap();
        let html = r#"
            <table><tbody>
              <tr>
                <td><a href="/topics/view/1">片名 A</a></td>
                <td><a href="magnet:?xt=urn:btih:aaaa&amp;dn=x">磁力</a></td>
                <td>x</td><td>y</td>
                <td>1.5 GB</td><td>42</td>
              </tr>
              <tr><td><a href="/topics/view/2">没有磁力的行</a></td></tr>
            </tbody></table>
        "#;

        let rows = parse_rows(&def, "cardigann:dmhy", html, "https://share.dmhy.org");
        // 第二行没有磁力锚点 ⇒ 被 `:has(...)` 的降级判定筛掉
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r.name, "片名 A");
        assert_eq!(r.provider, "cardigann:dmhy");
        assert_eq!(r.magnet.as_deref(), Some("magnet:?xt=urn:btih:aaaa&dn=x"));
        assert_eq!(r.info_hash.as_deref(), Some("aaaa"));
        assert_eq!(r.seeders, Some(42));
        assert_eq!(
            r.detail_url.as_deref(),
            Some("https://share.dmhy.org/topics/view/1")
        );
    }
}
