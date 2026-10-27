//! 苹果CMS（MacCMS / 苹果 CMS v10）**JSON API** 适配器。
//!
//! ## 为什么还要一套 JSON 适配器
//!
//! [`crate::video`] 那套是 **HTML 抓取**：一组 CSS 选择器链 + 站点配置表。它的软肋是
//! **站点换模板就全废** —— 一次改版要改 `SELECTOR_CHAINS`、补 fixture、发版
//! （本机不能编译，一轮 CI 往返 ~3 分钟）。
//!
//! 而 MacCMS 站几乎都另开了一条**结构化 JSON 接口**：
//!
//! ```text
//! GET  {api}?ac=videolist&wd=<关键词>&pg=<页>
//! → { "code": 1, "page": 1, "pagecount": 5, "total": 100,
//!     "list": [ { "vod_id": 123, "vod_name": "流浪地球", "vod_year": "2019",
//!                 "type_name": "科幻片", "vod_play_url": "第1集$https://…m3u8" } ] }
//! ```
//!
//! 这条接口的字段名十年没变过（TVBox / 影视仓整个生态都靠它），**抗改版能力强一个量级**。
//!
//! ## ⚠️ 已知短板：`detail_url` 是**推**出来的
//!
//! JSON 接口不返回详情页 URL。这里按 MacCMS 的通用路由推：
//!
//! ```text
//! https://host/api.php/provide/vod/  →  https://host/index.php/vod/detail/id/<vod_id>.html
//! ```
//!
//! 大多数建站模板支持这条路由，但**纯采集站**（只有 API、没有前台页面）会 404。
//! 在没有内置播放器的前提下（25MB 体积门，见 `docs/BACKLOG-ZHUIJU.md` §3），
//! 本模块的完成定义就是「结构化结果 + 尽力打开」——「直接播 `vod_play_url` 里的 m3u8」
//! 属于下一层，不要在这里顺手加。
//!
//! ## 契约（与 [`crate::video`] 一致）
//!
//! **永不 panic、永不返回 `Err`**：失败落在 [`VideoSearchOutcome::error`]，
//! 没有结果是 `ok(vec![])`。

use serde_json::Value;

use bt_core::http::HttpClient;
use bt_core::normalize::encode_uri_component;

use crate::video::{VideoItem, VideoSearchOutcome};

/// 一个用户自填的 JSON 源。
///
/// ⚠️ 字段是 `String` 而不是 `&'static str` —— 这些源来自**用户配置**（TVBox 单仓导入），
/// 生命周期是运行时的，跟 [`crate::video::MacCmsSite`] 那些编译期常量不是一回事。
/// 也正因为如此，它**不能**塞进 [`crate::video::VideoProvider`]（那个 trait 的
/// `id()` / `name()` 返回 `&'static str`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonApiSite {
    /// 稳定 id（TVBox 的 `key`，或 `tvbox:<key>`）。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// 接口基址，含完整路径，例如 `https://host/api.php/provide/vod/`。
    pub api: String,
}

/// 解析出来的一页结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedList {
    pub items: Vec<VideoItem>,
    /// 服务端声明的总页数（有的站不给）。
    pub page_count: Option<u32>,
    /// 服务端声明的总条数（有的站不给）。
    pub total: Option<u32>,
}

/// 拼搜索 URL。
///
/// `api` 里可能已经有 `?`（少数站的接口写成了 `.../vod/?foo=bar`），
/// 所以分隔符要看情况用 `&`。
pub fn search_url(api: &str, query: &str, page: u32) -> String {
    let sep = if api.contains('?') { '&' } else { '?' };
    let mut url = format!(
        "{api}{sep}ac=videolist&wd={}",
        encode_uri_component(query.trim())
    );
    if page > 1 {
        url.push_str("&pg=");
        url.push_str(&page.to_string());
    }
    url
}

/// 按 MacCMS 通用路由推出详情页。取不出主机名时返回 `None`（不猜）。
///
/// `https://host/api.php/provide/vod/` → `https://host/index.php/vod/detail/id/123.html`
pub fn detail_url_for(api: &str, vod_id: &str) -> Option<String> {
    let id = vod_id.trim();
    if id.is_empty() {
        return None;
    }
    let scheme_end = api.find("://")? + 3;
    let host_end = api[scheme_end..]
        .find('/')
        .map_or(api.len(), |i| scheme_end + i);
    if host_end <= scheme_end {
        return None;
    }
    let root = &api[..host_end];
    Some(format!("{root}/index.php/vod/detail/id/{id}.html"))
}

/// 解析一次接口响应（**纯函数**，不打网 —— 单测直接喂字符串）。
///
/// `site_id` 会写进每条 [`VideoItem::provider`]，与 BT provider 的口径一致。
pub fn parse(api: &str, site_id: &str, body: &str) -> Result<ParsedList, String> {
    let root: Value = serde_json::from_str(body).map_err(|e| format!("bad_json: {e}"))?;

    // MacCMS 的成功码是 1；`code` 可能是数字也可能是字符串。没有 `code` 字段的站照样往下走。
    // ⚠️ 写成**单层 if let + 组合 filter**：edition 2021 没有 let-chain，
    // 嵌套 if 会被 clippy 的 `collapsible_if` 指着。
    if let Some(code) = root
        .get("code")
        .map(as_text)
        .filter(|c| !c.is_empty() && c != "1")
    {
        return Err(format!("api_code_{code}"));
    }

    // `list` 有的是数组，有的站返回空对象 `{}` 表示没结果。
    let raw = match root.get("list") {
        Some(Value::Array(a)) => a.clone(),
        Some(Value::Null) | None | Some(Value::Object(_)) => Vec::new(),
        Some(other) => return Err(format!("bad_list: {}", kind_of(other))),
    };

    let mut items = Vec::with_capacity(raw.len());
    let mut seen = std::collections::HashSet::new();

    for it in raw {
        let Some(name) = it.get("vod_name").map(as_text).filter(|s| !s.is_empty()) else {
            continue;
        };
        let id = it.get("vod_id").map(as_text).unwrap_or_default();
        // 没有 vod_id 就推不出详情页 —— 这种条目留着也没法点开，直接丢。
        let Some(detail_url) = detail_url_for(api, &id) else {
            continue;
        };
        if !seen.insert(detail_url.clone()) {
            continue;
        }

        items.push(VideoItem {
            title: name,
            detail_url,
            provider: site_id.to_string(),
            year: it
                .get("vod_year")
                .map(as_text)
                .filter(|s| !s.is_empty() && s != "0"),
            // `type_name` 是主流写法；`vod_type_name` 是部分二次开发模板的写法。
            category: it
                .get("type_name")
                .or_else(|| it.get("vod_type_name"))
                .map(as_text)
                .filter(|s| !s.is_empty()),
        });
    }

    Ok(ParsedList {
        items,
        page_count: root.get("pagecount").and_then(as_u32),
        total: root.get("total").and_then(as_u32),
    })
}

/// 查一页。**永不 panic、永不返回 `Err`**。
pub async fn search_at(
    http: &HttpClient,
    site: &JsonApiSite,
    query: &str,
    page: u32,
) -> VideoSearchOutcome {
    let url = search_url(&site.api, query, page);

    let resp = http.get_text(&url, None).await;
    if let Some(e) = resp.error {
        return VideoSearchOutcome::err(e);
    }
    let Some(body) = resp.html.filter(|h| !h.is_empty()) else {
        return VideoSearchOutcome::err("no_html");
    };

    match parse(&site.api, &site.id, &body) {
        Ok(parsed) => VideoSearchOutcome::ok(parsed.items),
        Err(e) => VideoSearchOutcome::err(e),
    }
}

/// JSON 标量 → 字符串。数字（`vod_id` 常是 number）也要能取。
fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// `pagecount` / `total` 可能是数字也可能是字符串（不同 PHP 版本）。
fn as_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const API: &str = "https://www.example.com/api.php/provide/vod/";

    #[test]
    fn search_url_appends_pg_only_after_page_one() {
        assert_eq!(
            search_url(API, "流浪地球", 1),
            "https://www.example.com/api.php/provide/vod/?ac=videolist&wd=%E6%B5%81%E6%B5%AA%E5%9C%B0%E7%90%83"
        );
        assert!(search_url(API, "dune", 2).ends_with("&pg=2"));
        // 已经带 `?` 的接口不能再拼一个 `?`
        assert_eq!(
            search_url("https://h/api.php/provide/vod?x=1", "a", 1),
            "https://h/api.php/provide/vod?x=1&ac=videolist&wd=a"
        );
    }

    #[test]
    fn detail_url_uses_the_standard_maccms_route() {
        assert_eq!(
            detail_url_for(API, "12345").unwrap(),
            "https://www.example.com/index.php/vod/detail/id/12345.html"
        );
        // 没有路径的 api 也要能取到主机名
        assert_eq!(
            detail_url_for("https://h", "9").unwrap(),
            "https://h/index.php/vod/detail/id/9.html"
        );
        assert!(detail_url_for(API, "  ").is_none());
        assert!(detail_url_for("no-scheme/x", "1").is_none());
    }

    #[test]
    fn parse_maps_fields_and_dedups() {
        let body = r#"{
            "code": 1, "page": 1, "pagecount": 5, "total": 100,
            "list": [
                {"vod_id": 123, "vod_name": "流浪地球", "vod_year": "2019", "type_name": "科幻片"},
                {"vod_id": "123", "vod_name": "流浪地球（重复）", "vod_year": "", "type_name": ""},
                {"vod_id": 456, "vod_name": "沙丘2", "vod_year": 0, "vod_type_name": "动作片"},
                {"vod_id": 789, "vod_name": "   "}
            ]
        }"#;
        let parsed = parse(API, "tvbox:demo", body).unwrap();

        // 789 名字是空白 → 丢；"123" 与 123 推出的详情页相同 → 去重
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].title, "流浪地球");
        assert_eq!(parsed.items[0].year.as_deref(), Some("2019"));
        assert_eq!(parsed.items[0].category.as_deref(), Some("科幻片"));
        assert_eq!(parsed.items[0].provider, "tvbox:demo");
        assert_eq!(
            parsed.items[0].detail_url,
            "https://www.example.com/index.php/vod/detail/id/123.html"
        );
        // 数字型 vod_id + vod_type_name 的兜底 + year "0" 当没有
        assert_eq!(parsed.items[1].title, "沙丘2");
        assert_eq!(parsed.items[1].year, None);
        assert_eq!(parsed.items[1].category.as_deref(), Some("动作片"));

        assert_eq!(parsed.page_count, Some(5));
        assert_eq!(parsed.total, Some(100));
    }

    #[test]
    fn parse_accepts_string_code_and_empty_list_object() {
        // 有的站把 code 写成字符串
        assert!(parse(API, "s", r#"{"code":"1","list":[]}"#).is_ok());
        // 没结果时部分站返回 `"list":{}` —— 不算错误，算 0 条
        let parsed = parse(API, "s", r#"{"code":1,"list":{}}"#).unwrap();
        assert!(parsed.items.is_empty());
        // 连 code 都不给的站
        assert!(parse(API, "s", r#"{"list":[]}"#).is_ok());
    }

    #[test]
    fn parse_reports_errors_instead_of_panicking() {
        assert!(parse(API, "s", "not json")
            .unwrap_err()
            .starts_with("bad_json"));
        // code != 1 是错误（与「真的没结果」必须能区分）
        assert_eq!(
            parse(API, "s", r#"{"code":0,"msg":"参数错误"}"#).unwrap_err(),
            "api_code_0"
        );
        assert!(parse(API, "s", r#"{"list":"oops"}"#)
            .unwrap_err()
            .starts_with("bad_list"));
    }
}
