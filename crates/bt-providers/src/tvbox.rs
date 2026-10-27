//! TVBox / 影视仓「单仓」配置解析 —— **只取能当 JSON API 用的站点**。
//!
//! ## 为什么只取 `type == 1`
//!
//! TVBox 生态的 `sites[]` 里，`type` 决定这条源怎么取数：
//!
//! | type | 含义 | 本项目能用吗 |
//! |---|---|---|
//! | `0` | XML / 优酷等旧接口 | ❌ 要另写一套 XML 适配 |
//! | `1` | **苹果CMS JSON**（`?ac=videolist&wd=`） | ✅ 走 [`crate::maccms_json`] |
//! | `3` | Spider（要先下 `xxx.jar` 再跑 JVM） | ❌ 等于内嵌一个 Java 运行时 |
//! | `4` | 特殊/定制 | ❌ 无通用协议 |
//!
//! ⇒ 只有 `type == 1` 是「一份 HTTP 请求 + 一次 JSON 解析」就能吃下的。
//! 其余类型**不是不能做，是做了要拖进一整个运行时**，与 25MB 体积门直接冲突
//! （`docs/BACKLOG-ZHUIJU.md` §3）。
//!
//! ## 两种配置形态
//!
//! ```text
//! 单仓：{"sites": [ { "key": "a", "name": "A", "type": 1, "api": "https://…/provide/vod/" } ]}
//! 多仓：{"urls":  [ { "name": "某某多仓", "url": "http://…/sub.json" } ]}
//! ```
//!
//! 多仓只**列出来**，不自动递归抓取 —— 递归会变成一个不受控的抓取器
//! （一个文件里嵌几十个文件、每个文件里几百个站），失败面太大。
//!
//! ## 为什么不做内置
//!
//! 这些地址失效极快、内容良莠不齐（含灰产/成人源），**只支持用户自填**。
//! 本项目不预置、不内置、不分发任何 TVBox 配置 —— 与「自定义 Torznab 索引器」
//! （`config_store::indexers.json`）同一套产品口径。

use serde_json::Value;

use crate::maccms_json::JsonApiSite;

/// 一条可用的 JSON 源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TvBoxSite {
    /// TVBox 的 `key`（站内唯一标识）。可能就是 URL 本身。
    pub key: String,
    pub name: String,
    /// `api`，形如 `https://host/api.php/provide/vod/`。
    pub api: String,
}

/// 多仓里的一个子仓引用。**只用于展示**，不自动抓取。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TvBoxSourceRef {
    pub name: String,
    pub url: String,
}

/// 解析结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TvBoxRepo {
    /// `type == 1` 的可用站点。
    pub sites: Vec<TvBoxSite>,
    /// 多仓里的子仓引用（单仓时为空）。
    pub sources: Vec<TvBoxSourceRef>,
    /// 被丢掉的站点条数（非 type=1 / 无 api / 重复）—— UI 拿去提示「导入 N 条，跳过 M 条」。
    pub skipped: usize,
}

impl TvBoxRepo {
    pub fn is_empty(&self) -> bool {
        self.sites.is_empty() && self.sources.is_empty()
    }

    /// 转成 [`JsonApiSite`]，`id` 加 `tvbox:` 前缀 —— 与用户手填的源区分开，
    /// UI 一眼能看出这条是从哪来的。
    pub fn as_json_sites(&self) -> Vec<JsonApiSite> {
        self.sites
            .iter()
            .map(|s| JsonApiSite {
                id: format!("tvbox:{}", s.key),
                name: s.name.clone(),
                api: s.api.clone(),
            })
            .collect()
    }
}

/// 解析一份 TVBox 配置（**纯函数**，不打网）。
///
/// - 有 `sites` → 单仓，按 [`is_usable_site`] 过滤；
/// - 有 `urls` → 多仓，只收集子仓引用；
/// - 两个都没有 → `Err`（**不能**当成「0 条」—— 那会让用户以为导入成功了）。
pub fn parse(body: &str) -> Result<TvBoxRepo, String> {
    let root: Value = serde_json::from_str(body).map_err(|e| format!("bad_json: {e}"))?;

    let sites = root.get("sites").and_then(Value::as_array);
    let urls = root.get("urls").and_then(Value::as_array);

    let mut out = TvBoxRepo::default();

    if let Some(list) = sites {
        let mut seen = std::collections::HashSet::new();
        for it in list {
            match usable_site(it) {
                Some(site) if seen.insert(site.api.clone()) => out.sites.push(site),
                _ => out.skipped += 1,
            }
        }
    }

    if let Some(list) = urls {
        for it in list {
            let url = it.get("url").and_then(Value::as_str).unwrap_or("").trim();
            if !is_http(url) {
                out.skipped += 1;
                continue;
            }
            let name = it
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .unwrap_or(url);
            out.sources.push(TvBoxSourceRef {
                name: name.to_string(),
                url: url.to_string(),
            });
        }
    }

    if out.is_empty() && out.skipped == 0 {
        return Err("no_sites_or_urls".to_string());
    }
    Ok(out)
}

/// 一条 `sites[]` 能不能用。
///
/// 判定顺序（任何一条不过就 `None`，调用方计为 skipped）：
/// 1. `type` 必须是 `1`（数字或字符串都认）—— 缺 `type` 视作 `0`，不用；
/// 2. `api` 必须是 `http(s)://`；
/// 3. `searchable` 显式为 `0` 的跳过（TVBox 里这种站只能点开分类，不能搜）；
/// 4. `name` 空就退回 `key`，两者都空才丢。
fn usable_site(v: &Value) -> Option<TvBoxSite> {
    let ty = v.get("type").map(|t| match t {
        Value::Number(n) => n.as_i64().map(|n| n.to_string()),
        Value::String(s) => Some(s.trim().to_string()),
        _ => None,
    });
    // `Some(Some("1"))` 才算 type=1
    if ty.flatten().as_deref() != Some("1") {
        return None;
    }

    let api = v.get("api").and_then(Value::as_str).unwrap_or("").trim();
    if !is_http(api) {
        return None;
    }

    if let Some(s) = v.get("searchable") {
        let off = match s {
            Value::Bool(b) => !*b,
            Value::Number(n) => n.as_i64().unwrap_or(0) == 0,
            Value::String(t) => t.trim() == "0",
            _ => false,
        };
        if off {
            return None;
        }
    }

    let key = v.get("key").and_then(Value::as_str).unwrap_or("").trim();
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(key);
    if name.is_empty() {
        return None;
    }

    Some(TvBoxSite {
        key: if key.is_empty() { api } else { key }.to_string(),
        name: name.to_string(),
        api: api.to_string(),
    })
}

fn is_http(u: &str) -> bool {
    u.starts_with("http://") || u.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_repo_keeps_only_type_1_with_http_api() {
        let body = r#"{
            "sites": [
                {"key":"a","name":"甲","type":1,"api":"https://a.example/api.php/provide/vod/"},
                {"key":"b","name":"乙","type":"1","api":"https://b.example/api.php/provide/vod/"},
                {"key":"c","name":"丙","type":0,"api":"https://c.example/api.php/provide/vod/"},
                {"key":"d","name":"丁","type":3,"api":"csp_Ding"},
                {"key":"e","name":"戊","type":1,"api":"ftp://e.example/"},
                {"key":"f","name":"己","type":1,"api":"https://a.example/api.php/provide/vod/"},
                {"key":"g","name":"庚","type":1,"api":"https://g.example/api.php/provide/vod/","searchable":0}
            ]
        }"#;
        let repo = parse(body).unwrap();

        assert_eq!(repo.sites.len(), 2, "{:?}", repo.sites);
        assert_eq!(repo.sites[0].name, "甲");
        assert_eq!(repo.sites[1].name, "乙");
        // c/d/e/f/g 五个全被跳过（type≠1 ×2、非 http、重复、searchable=0）
        assert_eq!(repo.skipped, 5);
        assert!(repo.sources.is_empty());
    }

    #[test]
    fn missing_type_counts_as_zero() {
        let body = r#"{"sites":[{"key":"x","name":"无 type","api":"https://x.example/vod/"}]}"#;
        let repo = parse(body).unwrap();
        assert!(repo.sites.is_empty());
        assert_eq!(repo.skipped, 1);
    }

    #[test]
    fn name_falls_back_to_key() {
        let body = r#"{"sites":[{"key":"https://k.example/vod/","type":1,"api":"https://k.example/vod/"}]}"#;
        let repo = parse(body).unwrap();
        assert_eq!(repo.sites.len(), 1);
        assert_eq!(repo.sites[0].name, "https://k.example/vod/");
    }

    #[test]
    fn multi_repo_lists_sources_without_generating_sites() {
        let body = r#"{
            "urls": [
                {"name":"某多仓","url":"http://m.example/dc.txt"},
                {"url":"https://m2.example/sub.json"},
                {"name":"坏的","url":"file:///etc/passwd"}
            ]
        }"#;
        let repo = parse(body).unwrap();
        assert!(repo.sites.is_empty());
        assert_eq!(repo.sources.len(), 2);
        assert_eq!(repo.sources[0].name, "某多仓");
        // 没有 name 时用 url 兜底
        assert_eq!(repo.sources[1].name, "https://m2.example/sub.json");
        assert_eq!(repo.skipped, 1);
    }

    #[test]
    fn empty_or_broken_input_is_an_error_not_an_empty_import() {
        // 「什么都没解析到」必须报错，否则 UI 会显示「导入成功 0 条」
        assert_eq!(parse(r#"{"sites":[]}"#).unwrap_err(), "no_sites_or_urls");
        assert_eq!(parse("{}").unwrap_err(), "no_sites_or_urls");
        assert!(parse("<html>不是 json</html>")
            .unwrap_err()
            .starts_with("bad_json"));
    }

    #[test]
    fn as_json_sites_prefixes_the_id() {
        let repo = parse(
            r#"{"sites":[{"key":"abc","name":"甲","type":1,"api":"https://a.example/vod/"}]}"#,
        )
        .unwrap();
        let sites = repo.as_json_sites();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].id, "tvbox:abc");
        assert_eq!(sites[0].name, "甲");
        assert_eq!(sites[0].api, "https://a.example/vod/");
    }
}
