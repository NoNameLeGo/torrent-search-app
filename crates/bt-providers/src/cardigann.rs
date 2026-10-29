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
use serde::{Deserialize, Serialize};
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
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawSearch {
    #[serde(default)]
    pub paths: Vec<RawPath>,
    #[serde(default)]
    pub inputs: BTreeMap<String, Yaml>,
    pub rows: Option<RawRows>,
    #[serde(default)]
    pub fields: BTreeMap<String, RawField>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawPath {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub method: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawRows {
    /// ⚠️ 从 YAML 里读出来是**含 `:has()`** 的原始串，执行前要过 [`strip_has`]。
    #[serde(default)]
    pub selector: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawField {
    pub selector: Option<String>,
    pub attribute: Option<String>,
    #[serde(default)]
    pub filters: Vec<RawFilter>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
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
///
/// ⚠️ 参数按 **key 的字典序**拼（`inputs` 是 `BTreeMap`）—— 不是 YAML 里的声明顺序。
/// 服务端不看参数顺序，但**单测要按字典序写期望值**（2026-10-27 首轮 CI 就栽在这）。
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
        // ⚠️ 替换后的值**已经编码过**，绝不能再 encode 一次 —— 否则
        // `%E6%B5%81` 会变成 `%25E6%25B5%2581`（首轮 CI 抓到的就是这个）。
        let Some(val) = fill_template(&yaml_text(v), &kw, &page_s) else {
            continue;
        };
        if val.trim().is_empty() {
            continue;
        }
        url.push(sep);
        sep = '&';
        url.push_str(&encode_uri_component(k));
        url.push('=');
        url.push_str(&val);
    }

    Some(url)
}

/// 关键词 / 页码两种占位符（Cardigann 里两种写法都见过）。
const KW_VARS: [&str; 2] = ["{{ .Keywords }}", "{{ .Query.Keywords }}"];
const PAGE_VARS: [&str; 2] = ["{{ .Page }}", "{{ .Query.Page }}"];

/// 把模板里的占位符换成**已编码**的值，其余片段就地编码。
///
/// 返回 `None` = 里面有**不认识的**占位符（分类 id / 排序模板变量）—— 调用方应丢掉这个 input。
///
/// 为什么要逐段处理而不是「先替换再整体编码」：整体编码会把已经编码好的
/// `%E6%B5%81` 再编一次（`%25E6...`）。这是 2026-10-27 首轮 CI 的真实失败。
fn fill_template(raw: &str, kw: &str, page: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len() + kw.len());
    let mut i = 0;

    while i < raw.len() {
        let lit_end = raw[i..].find("{{").map_or(raw.len(), |p| i + p);
        if lit_end > i {
            out.push_str(&encode_uri_component(&raw[i..lit_end]));
            i = lit_end;
            continue;
        }
        // 这里必然以 `{{` 开头
        let close = raw[i..].find("}}")?;
        let var = &raw[i..i + close + 2];
        let val = if KW_VARS.contains(&var) {
            kw
        } else if PAGE_VARS.contains(&var) {
            page
        } else {
            return None;
        };
        out.push_str(val);
        i += close + 2;
    }

    Some(out)
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
        // ⚠️ 参数按 key 的**字典序**：keyword < order < sort_id（不是 YAML 声明顺序）
        assert_eq!(
            url,
            "https://share.dmhy.org/topics/list?keyword=%E6%B5%81%E6%B5%AA%E5%9C%B0%E7%90%83&order=date-desc&sort_id=0"
        );
        // 回归：关键词**只能编码一次**（首轮 CI 出现过 %25E6%25B5%2581 的双重编码）
        assert!(!url.contains("%25E6"), "{url}");
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
    fn unknown_placeholders_drop_the_input_entirely() {
        let def = parse_definition(
            "links: [https://x/]\nsearch:\n  paths: [{path: q}]\n  inputs: {kw: '{{ .Keywords }}', cat: '{{ .Categories }}'}\n  rows: {selector: tr}\n  fields: {title: {selector: a}}\n",
        )
        .unwrap();
        let url = search_url(&def, "abc", 1).unwrap();
        // 认识的占位符保留，不认识的（分类 id）整条丢掉
        assert!(url.contains("kw=abc"), "{url}");
        assert!(!url.contains("cat="), "{url}");
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
        // YAML 单引号不处理转义 ⇒ 这里要传 `S(\d+)E`，Rust 里写成 `\\d`
        assert_eq!(
            apply_filters("S01E02", &[f("{name: regexp, args: 'S(\\d+)E'}")]),
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

/// 把一条用户规则包装成 `Provider`，这样它就能进聚合层。
///
/// ⚠️ **`id()` / `name()` 要求 `&'static str`** —— 与 Torznab 同一个办法：构造时
/// `Box::leak` 一次，之后一直用。代价是每次搜索每规则漏 2 个小 String
/// （`AGENTS.md` 里对 Torznab 那条注释说的就是这件事；换成 `Cow` 要改 trait 签名，
/// 不值得为一次搜索漏几十字节去动公共 API）。
pub struct RuleProvider {
    id: &'static str,
    name: &'static str,
    def: Definition,
}

impl RuleProvider {
    pub fn new(id: &str, name: &str, def: Definition) -> Self {
        Self {
            id: Box::leak(id.to_string().into_boxed_str()),
            name: Box::leak(name.to_string().into_boxed_str()),
            def,
        }
    }
}

impl crate::Provider for RuleProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn search<'a>(
        &'a self,
        http: &'a HttpClient,
        query: &'a str,
        page: u32,
    ) -> crate::BoxFuture<'a, SearchOutcome> {
        Box::pin(search_at(http, &self.def, self.id, query, page))
    }
}

// ---------------------------------------------------------------------------
// 用户规则（`sources.json` 的 `rule` 字段）
// ---------------------------------------------------------------------------

/// 我们**实现**的 filter 白名单。
///
/// 白名单外的一律拒绝导入 —— 不是"悄悄忽略"。否则用户写了 `dateparse` 却发现日期是空的，
/// 会以为是站点问题（`docs/SOURCES-DESIGN.md` D3：失败归因要清楚）。
pub const SUPPORTED_FILTERS: [&str; 7] = [
    "replace",
    "re_replace",
    "append",
    "prepend",
    "trim",
    "split",
    "regexp",
];

/// 规则里第一个**不支持**的 filter 名（`None` = 全在白名单内）。
pub fn unsupported_filter(def: &Definition) -> Option<String> {
    let search = def.search.as_ref()?;
    for field in search.fields.values() {
        for f in &field.filters {
            if !SUPPORTED_FILTERS.contains(&f.name.as_str()) {
                return Some(f.name.clone());
            }
        }
    }
    None
}

/// 用户规则 JSON → [`Definition`]。
///
/// 接受两种形态：
///
/// 1. **扁平形态**（`docs/SOURCES-DESIGN.md` §2 给用户写的那种，字段少、好写）：
///    `{ "base", "search": { "path", "method"? , "inputs"? }, "rows", "fields" }`
/// 2. **Cardigann 形态**（有 `search.paths[]`）—— 我们自己序列化出来的就是这种，
///    也方便用户直接贴 Prowlarr 的定义转成 JSON
///
/// 校验两件事：能不能跑（[`is_supported`]）、filter 是否全在白名单里。
pub fn from_user_rule(v: &serde_json::Value) -> Result<Definition, String> {
    // 形态 2：已经是 Cardigann 形状
    if v.get("search")
        .and_then(|s| s.get("paths"))
        .and_then(serde_json::Value::as_array)
        .is_some()
    {
        let def: Definition =
            serde_json::from_value(v.clone()).map_err(|e| format!("bad_rule: {e}"))?;
        return strict_validate(def);
    }

    // 形态 1：扁平
    let base = v
        .get("base")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("rule_base_invalid".to_string());
    }

    let search = v
        .get("search")
        .ok_or_else(|| "rule_no_search".to_string())?;
    let path = search
        .get("path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim();
    if path.is_empty() {
        return Err("rule_no_path".to_string());
    }
    let method = search
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("GET");
    let inputs = search
        .get("inputs")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let rows = v
        .get("rows")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| "rule_no_rows".to_string())?;
    let fields = v
        .get("fields")
        .cloned()
        .ok_or_else(|| "rule_no_fields".to_string())?;

    // 拼成 Cardigann 形状再反序列化 —— 只维护**一套**解析路径
    let shape = serde_json::json!({
        "type": "public",
        "links": [base],
        "search": {
            "paths": [{ "path": path, "method": method }],
            "inputs": inputs,
            "rows": { "selector": rows },
            "fields": fields,
        }
    });

    let def: Definition = serde_json::from_value(shape).map_err(|e| format!("bad_rule: {e}"))?;
    strict_validate(def)
}

/// 能不能跑 + filter 白名单，两关一起过（YAML 路径也走这里）。
pub fn validate_rule(def: Definition) -> Result<Definition, String> {
    if !is_supported(&def) {
        return Err("rule_unsupported".to_string());
    }
    Ok(def)
}

/// All filter names outside the whitelist (deduped, in first-seen order).
///
/// Used for a degraded-mode notice: do not block, but tell the caller so the UI can
/// say "N members use unimplemented filters, their date may be raw text".
pub fn unsupported_filters(def: &Definition) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(search) = def.search.as_ref() else {
        return out;
    };
    for field in search.fields.values() {
        for f in &field.filters {
            if !SUPPORTED_FILTERS.contains(&f.name.as_str()) && !out.contains(&f.name) {
                out.push(f.name.clone());
            }
        }
    }
    out
}

/// Hand-written rules only: structure AND filter whitelist.
///
/// The user wrote it, so naming the offending filter is actionable. Third-party
/// definitions (Prowlarr) go through [`validate_rule`] instead -- they cannot be fixed
/// by the user, and rejecting them would just make the feature look broken.
fn strict_validate(def: Definition) -> Result<Definition, String> {
    let def = validate_rule(def)?;
    if let Some(bad) = unsupported_filter(&def) {
        return Err(format!("unsupported_filter:{bad}"));
    }
    Ok(def)
}

#[cfg(test)]
mod rule_tests {
    use super::*;

    fn flat_rule() -> serde_json::Value {
        serde_json::json!({
            "base": "https://share.dmhy.org",
            "search": {
                "path": "/topics/list",
                "inputs": { "keyword": "{{ .Keywords }}" }
            },
            "rows": "table tbody tr:has(a[href^=\"magnet:?\"])",
            "fields": {
                "title": { "selector": "a" },
                "download": { "selector": "a[href^=\"magnet:?\"]", "attribute": "href" }
            }
        })
    }

    #[test]
    fn flat_rule_becomes_a_runnable_definition() {
        let def = from_user_rule(&flat_rule()).unwrap();
        assert!(is_supported(&def));
        assert!(unsupported_filter(&def).is_none());
        assert_eq!(
            search_url(&def, "abc", 1).unwrap(),
            "https://share.dmhy.org/topics/list?keyword=abc"
        );
    }

    #[test]
    fn cardigann_shaped_rule_also_works_and_round_trips() {
        let flat = from_user_rule(&flat_rule()).unwrap();
        let canonical = serde_json::to_value(&flat).unwrap();
        // 序列化出来的是 Cardigann 形状（search.paths[]），要能再读回去
        assert!(canonical
            .get("search")
            .and_then(|s| s.get("paths"))
            .is_some());
        let back = from_user_rule(&canonical).unwrap();
        assert_eq!(back.links, flat.links);
        assert_eq!(
            search_url(&back, "abc", 1).unwrap(),
            search_url(&flat, "abc", 1).unwrap()
        );
    }

    #[test]
    fn missing_bits_are_named_exactly() {
        let mut v = flat_rule();
        v["base"] = serde_json::json!("ftp://nope/");
        assert_eq!(from_user_rule(&v).unwrap_err(), "rule_base_invalid");

        let mut v = flat_rule();
        v.as_object_mut().unwrap().remove("rows");
        assert_eq!(from_user_rule(&v).unwrap_err(), "rule_no_rows");

        let mut v = flat_rule();
        v.as_object_mut().unwrap().remove("fields");
        assert_eq!(from_user_rule(&v).unwrap_err(), "rule_no_fields");

        let mut v = flat_rule();
        v.as_object_mut().unwrap().remove("search");
        assert_eq!(from_user_rule(&v).unwrap_err(), "rule_no_search");
    }

    #[test]
    fn unsupported_filters_are_rejected_with_the_name() {
        let mut v = flat_rule();
        v["fields"]["title"]["filters"] = serde_json::json!([{ "name": "dateparse" }]);
        // 名字要带在错误里 —— 用户才知道该删哪个
        assert_eq!(
            from_user_rule(&v).unwrap_err(),
            "unsupported_filter:dateparse"
        );

        // 白名单内的照旧通过
        let mut v = flat_rule();
        v["fields"]["title"]["filters"] = serde_json::json!([{ "name": "trim" }]);
        assert!(from_user_rule(&v).is_ok());
    }

    #[test]
    fn third_party_definitions_tolerate_unknown_filters_but_report_them() {
        // 真实情况：6 个默认中文索引器里 5 个用了 dateparse。
        // 拒绝它们 = 用户看到「成功 6/6」却一条结果都没有。
        let yaml = r#"
type: public
links:
  - https://share.dmhy.org/
search:
  paths:
    - path: topics/list
  rows:
    selector: "table tbody tr"
  fields:
    title:
      selector: a
    date:
      selector: td
      filters:
        - name: dateparse
          args: "yyyy-MM-dd"
"#;
        let def = parse_definition(yaml).unwrap();
        // 目录路径：只校验结构，filter 放行
        assert!(validate_rule(def.clone()).is_ok());
        assert_eq!(unsupported_filters(&def), vec!["dateparse"]);
        // 手写路径：仍报名字（用户能改）
        assert!(from_user_rule(&serde_json::to_value(&def).unwrap()).is_err());
    }

    #[test]
    fn post_rules_are_unsupported_for_now() {
        let mut v = flat_rule();
        v["search"]["method"] = serde_json::json!("POST");
        assert_eq!(from_user_rule(&v).unwrap_err(), "rule_unsupported");
    }
}
