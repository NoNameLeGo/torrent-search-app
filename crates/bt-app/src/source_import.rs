//! **导入器**：把外部格式变成 [`SourceEntry`]，统一写进 `sources.json`。
//!
//! 这是「插口化」里**对外**的那一半 —— 用户在别处攒的源清单（TVBox / LibreTV / Prowlarr
//! / 一行一个网址）都能汇进同一份注册表，不用改代码、不用发版。
//!
//! 设计见 `docs/SOURCES-DESIGN.md` §3。四种格式的判据：
//!
//! | 输入 | 判据 | 变成什么 |
//! |---|---|---|
//! | TVBox 单仓 | JSON 有 `sites[]` | 每条 `type==1` → `maccms_json` |
//! | TVBox 多仓 | JSON 有 `urls[]` | **只提示子仓地址**，不递归（同 `bt_providers::tvbox` 口径） |
//! | LibreTV SourceList v2 | JSON 有 `sources[]` | 每条 → `maccms_json` |
//! | Prowlarr 清单 | JSON 有 `indexers[]` | 一条 `cardigann_catalog` + N 个成员 |
//! | 裸 URL 列表 | 一行一个 `http(s)` | 每条 → `maccms_json` |
//! | 单条规则 | 有 `base`+`rows`+`fields`（JSON）或 `search:`（YAML） | 一条 `html_rule` |
//!
//! ## 为什么导入结果里要带 `notes`
//!
//! 「导入成功 0 条」是最糟的反馈。多仓不展开、Spider 站用不了、条目重复 —— 这些都得说清楚，
//! 否则用户只会觉得功能坏了。

use std::collections::HashSet;

use crate::sources::{Adapter, Kind, Member, SourceEntry, SourcesFile};

/// 支持的外部格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    TvBox,
    LibreTv,
    Prowlarr,
    BareList,
    /// 单条抓取规则（Cardigann 语法；JSON 或 YAML 都认）。
    Rule,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::TvBox => "tvbox",
            Format::LibreTv => "libretv",
            Format::Prowlarr => "prowlarr",
            Format::BareList => "bare",
            Format::Rule => "rule",
        }
    }
}

/// 一次导入的结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    pub entries: Vec<SourceEntry>,
    /// 源文件里就**不支持**的条数（Spider 站、非 http 链接、缺 url …）。
    pub skipped: usize,
    /// 给人看的补充说明（多仓未展开、跳过了什么）。
    pub notes: Vec<String>,
}

/// 猜格式。判不出来就当裸列表（最宽松的那个）。
pub fn guess_format(text: &str) -> Format {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(text) else {
        // 不是 JSON：YAML 规则的两个必备段落
        return if text.contains("search:") && text.contains("fields:") {
            Format::Rule
        } else {
            Format::BareList
        };
    };

    if root.get("sites").is_some() || root.get("urls").is_some() {
        return Format::TvBox;
    }
    if root.get("indexers").is_some() {
        return Format::Prowlarr;
    }
    if root.get("sources").is_some() {
        return Format::LibreTv;
    }
    // 单条规则（扁平形态 or Cardigann 形态）
    if looks_like_rule(&root) {
        return Format::Rule;
    }
    // 裸数组：元素是对象且有 url ⇒ 当成 LibreTV 的「裸数组」形态
    if let Some(arr) = root.as_array() {
        if arr
            .first()
            .and_then(|v| v.get("url"))
            .and_then(serde_json::Value::as_str)
            .is_some()
        {
            return Format::LibreTv;
        }
    }
    Format::BareList
}

/// 按指定格式导入。**永不 panic**：失败给机器码，由调用方翻成人话。
pub fn import(text: &str, format: Format) -> Result<Imported, String> {
    match format {
        Format::TvBox => import_tvbox(text),
        Format::LibreTv => import_libretv(text),
        Format::Prowlarr => import_prowlarr(text),
        Format::BareList => Ok(import_bare(text)),
        Format::Rule => import_rule(text),
    }
}

/// TVBox：只留 `type == 1`（Spider 要 JVM，不做）。
fn import_tvbox(text: &str) -> Result<Imported, String> {
    let repo = bt_providers::tvbox::parse(text)?;
    let mut out = Imported {
        skipped: repo.skipped,
        ..Default::default()
    };

    for site in repo.as_json_sites() {
        out.entries.push(SourceEntry {
            id: site.id.clone(),
            name: site.name.clone(),
            kind: Kind::Video,
            adapter: Adapter::MaccmsJson,
            builtin: false,
            enabled: true,
            group: Some("导入".to_string()),
            url: Some(site.api.clone()),
            r#override: false,
            rule: None,
            members: Vec::new(),
        });
    }

    // 多仓：不递归。把子仓地址念出来，让用户去复制其中一个。
    if !repo.sources.is_empty() {
        let names: Vec<&str> = repo
            .sources
            .iter()
            .take(3)
            .map(|s| s.url.as_str())
            .collect();
        out.notes.push(format!(
            "这是多仓（{} 个子仓），请改填其中一个子仓地址，例如 {}",
            repo.sources.len(),
            names.join("、")
        ));
    }

    if out.entries.is_empty() && out.notes.is_empty() && out.skipped == 0 {
        return Err("empty_import".to_string());
    }
    Ok(out)
}

/// LibreTV SourceList v2 / 裸数组 `[{name,url}]`。
fn import_libretv(text: &str) -> Result<Imported, String> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("bad_json: {e}"))?;
    let arr = root
        .get("sources")
        .and_then(serde_json::Value::as_array)
        .or_else(|| root.as_array())
        .ok_or_else(|| "no_sources".to_string())?;

    let mut out = Imported::default();
    for it in arr {
        let url = it
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if !is_http(url) {
            out.skipped += 1;
            continue;
        }
        let name = it
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| host_of(url));
        out.entries.push(maccms_entry(name, url));
    }
    Ok(out)
}

/// Prowlarr 清单：一条目录 + N 个成员。
///
/// ⚠️ 成员默认 **`enabled: false`** —— 80 个 public 里境内能连的只是少数，
/// 全开会让每次搜索都在等超时（`docs/DATA-SOURCES.md` §6）。
fn import_prowlarr(text: &str) -> Result<Imported, String> {
    let list = bt_providers::cardigann::parse_catalog(text)?;
    if list.is_empty() {
        return Err("no_public_indexers".to_string());
    }

    let members: Vec<Member> = list
        .iter()
        .map(|e| Member {
            id: e.id.clone(),
            name: e.name.clone(),
            enabled: false,
        })
        .collect();

    Ok(Imported {
        entries: vec![SourceEntry {
            id: "catalog:prowlarr".to_string(),
            name: "Prowlarr 公共目录".to_string(),
            kind: Kind::Bt,
            adapter: Adapter::CardigannCatalog,
            builtin: false,
            enabled: true,
            group: Some("目录".to_string()),
            url: Some("https://indexers.prowlarr.com/master/12".to_string()),
            r#override: false,
            rule: None,
            members,
        }],
        skipped: 0,
        notes: vec![format!(
            "{} 个公共索引器，默认全部关闭 —— 先开几个能连上的",
            list.len()
        )],
    })
}

/// 一行一个 URL。
fn import_bare(text: &str) -> Imported {
    let mut out = Imported::default();
    let mut seen = HashSet::new();

    for line in text.lines() {
        let url = line
            .trim()
            .trim_end_matches(',')
            .trim_matches('"')
            .trim_matches('\'');
        if url.is_empty() || url.starts_with('#') {
            continue;
        }
        if !is_http(url) {
            out.skipped += 1;
            continue;
        }
        if !seen.insert(url.to_string()) {
            out.skipped += 1;
            continue;
        }
        out.entries.push(maccms_entry(host_of(url), url));
    }
    out
}

/// 是不是一条**抓取规则**（而不是一份源清单）。
///
/// 两种形态都认：扁平（`base` + `rows` + `fields`）与 Cardigann（`search.paths` + `fields`）。
fn looks_like_rule(v: &serde_json::Value) -> bool {
    let flat = v.get("base").is_some() && v.get("rows").is_some() && v.get("fields").is_some();
    let cardigann =
        v.get("search").and_then(|s| s.get("paths")).is_some() && v.get("fields").is_some();
    flat || cardigann
}

/// 单条规则：JSON（扁平或 Cardigann 形态）或 YAML。
///
/// 落库时统一**转成 Cardigann 形状的 JSON** 存进 `rule` 字段 —— 只维护一套形状，
/// 读回来直接喂 [`bt_providers::cardigann::from_user_rule`]。
fn import_rule(text: &str) -> Result<Imported, String> {
    let trimmed = text.trim_start();

    let def = if trimmed.starts_with('{') {
        let v: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("bad_json: {e}"))?;
        bt_providers::cardigann::from_user_rule(&v)?
    } else {
        bt_providers::cardigann::validate_rule(bt_providers::cardigann::parse_definition(text)?)?
    };

    let base = def.links.first().cloned().unwrap_or_default();
    let name = host_of(&base).to_string();
    let rule = serde_json::to_value(&def).map_err(|e| format!("bad_rule: {e}"))?;

    Ok(Imported {
        entries: vec![SourceEntry {
            id: format!("rule:{name}"),
            name,
            kind: Kind::Bt,
            adapter: Adapter::HtmlRule,
            builtin: false,
            enabled: true,
            group: Some("规则".to_string()),
            url: Some(base),
            r#override: false,
            rule: Some(rule),
            members: Vec::new(),
        }],
        skipped: 0,
        notes: vec!["规则已保存（BT 聚合接线在下一批）".to_string()],
    })
}

fn maccms_entry(name: &str, url: &str) -> SourceEntry {
    SourceEntry {
        id: format!("url:{}", host_of(url)),
        name: name.to_string(),
        kind: Kind::Video,
        adapter: Adapter::MaccmsJson,
        builtin: false,
        enabled: true,
        group: Some("导入".to_string()),
        url: Some(url.to_string()),
        r#override: false,
        rule: None,
        members: Vec::new(),
    }
}

fn is_http(u: &str) -> bool {
    u.starts_with("http://") || u.starts_with("https://")
}

/// `https://a.b/c?d` → `a.b`（拿不到就原样返回，不 panic）。
pub(crate) fn host_of(url: &str) -> &str {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// 把导入结果并进文件：**按 id 与 url 双重去重**（同 url 换个写法不该算新源）。
///
/// 返回新增条数（0 = 全是已有的）。
pub fn apply(file: &mut SourcesFile, imported: &Imported) -> usize {
    let mut ids: HashSet<String> = file.sources.iter().map(|s| s.id.clone()).collect();
    let mut urls: HashSet<String> = file
        .sources
        .iter()
        .filter_map(|s| s.url.as_deref())
        .map(|u| u.trim_end_matches('/').to_string())
        .collect();

    let mut added = 0;
    for e in &imported.entries {
        let url_key = e
            .url
            .as_deref()
            .unwrap_or("")
            .trim_end_matches('/')
            .to_string();
        if ids.contains(&e.id) || (!url_key.is_empty() && urls.contains(&url_key)) {
            continue;
        }
        ids.insert(e.id.clone());
        if !url_key.is_empty() {
            urls.insert(url_key);
        }
        file.sources.push(e.clone());
        added += 1;
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    const TVBOX_SINGLE: &str = r#"{
        "sites": [
            {"key":"a","name":"甲","type":1,"api":"https://a.example/api.php/provide/vod/"},
            {"key":"b","name":"乙","type":3,"api":"csp_B"}
        ]
    }"#;

    const TVBOX_MULTI: &str = r#"{"urls":[{"name":"某多仓","url":"http://m.example/dc.txt"}]}"#;

    const LIBRETV: &str = r#"{
        "name":"我的源列表","version":2,
        "sources":[{"name":"示例","url":"https://x.example/api.php/provide/vod"}]
    }"#;

    const PROWLARR: &str = r#"{"indexers":[
        {"id":"dmhy","name":"dmhy","type":"public","language":"zh-TW","links":["https://share.dmhy.org/"]},
        {"id":"priv","name":"私有","type":"private","links":["https://p/"]}
    ]}"#;

    #[test]
    fn guesses_each_format() {
        assert_eq!(guess_format(TVBOX_SINGLE), Format::TvBox);
        assert_eq!(guess_format(TVBOX_MULTI), Format::TvBox);
        assert_eq!(guess_format(LIBRETV), Format::LibreTv);
        assert_eq!(guess_format(PROWLARR), Format::Prowlarr);
        // 裸数组（LibreTV 的另一种形态）
        assert_eq!(
            guess_format(r#"[{"name":"a","url":"https://a/"}]"#),
            Format::LibreTv
        );
        // 纯文本
        assert_eq!(guess_format("https://a/\nhttps://b/"), Format::BareList);
        assert_eq!(guess_format(""), Format::BareList);
    }

    #[test]
    fn tvbox_keeps_only_json_sites_and_reports_skipped() {
        let out = import(TVBOX_SINGLE, Format::TvBox).unwrap();
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].id, "tvbox:a");
        assert_eq!(out.entries[0].adapter, Adapter::MaccmsJson);
        assert_eq!(out.entries[0].kind, Kind::Video);
        assert_eq!(out.skipped, 1, "type=3 的 Spider 站要计进跳过");
    }

    #[test]
    fn tvbox_multi_repo_only_explains_instead_of_recursing() {
        let out = import(TVBOX_MULTI, Format::TvBox).unwrap();
        assert!(out.entries.is_empty());
        assert_eq!(out.notes.len(), 1);
        assert!(out.notes[0].contains("多仓"), "{:?}", out.notes);
        assert!(out.notes[0].contains("http://m.example/dc.txt"));
    }

    #[test]
    fn libretv_reads_name_and_url() {
        let out = import(LIBRETV, Format::LibreTv).unwrap();
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].name, "示例");
        assert_eq!(
            out.entries[0].url.as_deref(),
            Some("https://x.example/api.php/provide/vod")
        );
        assert_eq!(out.entries[0].adapter, Adapter::MaccmsJson);
    }

    #[test]
    fn prowlarr_becomes_one_catalog_with_disabled_members() {
        let out = import(PROWLARR, Format::Prowlarr).unwrap();
        assert_eq!(out.entries.len(), 1);
        let cat = &out.entries[0];
        assert_eq!(cat.adapter, Adapter::CardigannCatalog);
        assert_eq!(cat.kind, Kind::Bt);
        assert_eq!(cat.members.len(), 1, "private 不该进成员");
        assert_eq!(cat.members[0].id, "dmhy");
        assert!(
            !cat.members[0].enabled,
            "成员默认关闭（80 个全开就是一片超时）"
        );
    }

    #[test]
    fn bare_list_skips_junk_and_duplicates() {
        let text = "\n# 注释\nhttps://a.example/vod/\nnot-a-url\nhttps://a.example/vod/\n";
        let out = import(text, Format::BareList).unwrap();
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].id, "url:a.example");
        assert_eq!(out.skipped, 2);
    }

    #[test]
    fn apply_dedups_by_id_and_by_normalized_url() {
        let mut file = SourcesFile::default();
        let a = import("https://a.example/vod/", Format::BareList).unwrap();
        assert_eq!(apply(&mut file, &a), 1);

        // 同一 URL 换个尾斜杠不该算新源
        let b = import("https://a.example/vod", Format::BareList).unwrap();
        assert_eq!(apply(&mut file, &b), 0);
        assert_eq!(file.sources.len(), 1);

        // 不同 URL 才新增
        let c = import("https://b.example/vod/", Format::BareList).unwrap();
        assert_eq!(apply(&mut file, &c), 1);
        assert_eq!(file.sources.len(), 2);
    }

    #[test]
    fn host_of_handles_ports_paths_and_junk() {
        assert_eq!(host_of("https://a.example:8080/x?y=1"), "a.example:8080");
        assert_eq!(host_of("http://b.example"), "b.example");
        assert_eq!(host_of("junk"), "junk");
    }
}

#[cfg(test)]
mod rule_import_tests {
    use super::*;

    const FLAT_RULE: &str = r#"{
        "base": "https://share.dmhy.org",
        "search": { "path": "/topics/list", "inputs": { "keyword": "{{ .Keywords }}" } },
        "rows": "table tbody tr:has(a[href^=\"magnet:?\"])",
        "fields": {
            "title": { "selector": "a" },
            "download": { "selector": "a[href^=\"magnet:?\"]", "attribute": "href" }
        }
    }"#;

    // ⚠️ 用**原始串 + block 风格**：flow 风格（`[a]` / `{k: v}`）在 serde_yaml 里
    // 踩过「did not find expected key」（2026-10-27 首轮 CI），别再改回去。
    const YAML_RULE: &str = r#"
type: public
links:
  - https://share.dmhy.org/
search:
  paths:
    - path: topics/list
  inputs:
    keyword: "{{ .Keywords }}"
  rows:
    selector: "table tbody tr"
  fields:
    title:
      selector: a
"#;

    #[test]
    fn a_rule_is_not_mistaken_for_a_source_list() {
        assert_eq!(guess_format(FLAT_RULE), Format::Rule);
        assert_eq!(guess_format(YAML_RULE), Format::Rule);
    }

    #[test]
    fn flat_json_rule_becomes_an_html_rule_entry() {
        let out = import(FLAT_RULE, Format::Rule).unwrap();
        assert_eq!(out.entries.len(), 1);
        let e = &out.entries[0];
        assert_eq!(e.adapter, Adapter::HtmlRule);
        assert_eq!(e.kind, Kind::Bt);
        assert_eq!(e.id, "rule:share.dmhy.org");
        assert_eq!(e.url.as_deref(), Some("https://share.dmhy.org"));
        // 存下来的是 Cardigann 形状（能再读回去）
        let stored = e.rule.as_ref().unwrap();
        assert!(stored.get("search").and_then(|s| s.get("paths")).is_some());
        assert!(bt_providers::cardigann::from_user_rule(stored).is_ok());
    }

    #[test]
    fn yaml_rule_is_accepted_too() {
        let out = import(YAML_RULE, Format::Rule).unwrap();
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].adapter, Adapter::HtmlRule);
    }

    #[test]
    fn unsupported_filters_fail_with_the_name_so_the_user_can_fix_it() {
        let bad = FLAT_RULE.replace(
            r#""title": { "selector": "a" }"#,
            r#""title": { "selector": "a", "filters": [{ "name": "dateparse" }] }"#,
        );
        let err = import(&bad, Format::Rule).unwrap_err();
        assert_eq!(err, "unsupported_filter:dateparse");
    }

    #[test]
    fn a_broken_rule_says_which_part_is_missing() {
        let no_rows =
            r#"{"base":"https://x/","search":{"path":"/s"},"fields":{"title":{"selector":"a"}}}"#;
        let err = import(no_rows, Format::Rule).unwrap_err();
        assert_eq!(err, "rule_no_rows");
    }
}
