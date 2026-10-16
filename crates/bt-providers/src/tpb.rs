//! The Pirate Bay —— 走公开的 `apibay` JSON API。
//!
//! 端点返回一个结果数组；**没有结果时返回 `[{"error":"no results"}]`**。
//! 从 `src/providers/tpb.js`（53 行）移植。
//!
//! ⚠️ apibay 的所有字段都是**字符串**（`"seeders": "39"`、`"size": "3654957056"`），
//! 所以这里一律按 `serde_json::Value` 收，再交给 `bt-core` 那套宽松解析，与 JS 一致。

use bt_core::http::HttpClient;
use bt_core::normalize::{encode_uri_component, normalize, to_int, NumOrText, RawResult};
use bt_core::TorrentResult;
use serde::Deserialize;
use serde_json::Value;

use crate::value::{v2nt, v2string};
use crate::SearchOutcome;

/// 默认端点。
pub const API: &str = "https://apibay.org/q.php";

/// 原始条目。字段全用 `Value` 收 —— 站点偶尔会把数字发成字符串、把字符串发成数字，
/// 用 `Option<String>` 之类会直接反序列化失败（JS 那边则是静默变 `undefined`）。
#[derive(Debug, Default, Deserialize)]
struct TpbItem {
    #[serde(default)]
    id: Value,
    #[serde(default)]
    name: Value,
    #[serde(default)]
    info_hash: Value,
    #[serde(default)]
    size: Value,
    #[serde(default)]
    seeders: Value,
    #[serde(default)]
    leechers: Value,
    #[serde(default)]
    num_files: Value,
    #[serde(default)]
    added: Value,
    #[serde(default)]
    category: Value,
}

/// TPB 用三位数字分类码，映射到我们的标准桶：
///   1xx Audio → music/books · 2xx Video → movies/series · 3xx Apps
///   4xx Games · 5xx Porn · 6xx Other → books/other
///
/// 只看首位数字；`102`（有声书）与 `205`/`208`（剧集）是例外。
pub fn tpb_category(code: &str) -> Option<String> {
    let n = code.trim();
    let out = match n.chars().next()? {
        '1' => {
            if n == "102" {
                "Books"
            } else {
                "Music"
            }
        }
        '2' => {
            if n == "205" || n == "208" {
                "Series"
            } else {
                "Movies"
            }
        }
        '3' => "Apps",
        '4' => "Games",
        '5' => "Porn",
        '6' => {
            if n == "601" || n == "602" {
                "Books"
            } else {
                "Other"
            }
        }
        _ => return None,
    };
    Some(out.to_string())
}

/// apibay 用「单元素数组 + `error` 字段」表示没有结果。
fn is_error_marker(v: &Value) -> bool {
    v.get("error")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
}

/// 用默认端点搜索。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_at(http, API, query).await
}

/// 用**指定端点**搜索。
///
/// 存在的意义是测试：测试把它指向本地一次性 HTTP 服务，于是整个 provider
/// 能在 CI 里跑完全程而不碰外网（CI 不保证有网）。
pub async fn search_at(http: &HttpClient, api: &str, query: &str) -> SearchOutcome {
    let url = format!("{api}?q={}", encode_uri_component(query));

    let resp = http.get_json::<Value>(&url, None).await;
    if let Some(e) = &resp.error {
        // 对应 JS 的两条分支：axios 把非 JSON 正文静默当字符串返回 →
        // `!Array.isArray(data)` → 'unexpected response'；其余 → 'TPB unreachable'
        return if resp.is_parse_error() {
            SearchOutcome::err(format!("unexpected response ({e})"))
        } else {
            SearchOutcome::err(format!("TPB unreachable ({e})"))
        };
    }

    let Some(Value::Array(items)) = resp.data else {
        return SearchOutcome::err("unexpected response");
    };

    if items.len() == 1 && items.first().is_some_and(is_error_marker) {
        // 无结果，但**不是错误**
        return SearchOutcome::ok(Vec::new());
    }

    let results = items.iter().filter_map(card_from_value).collect();
    SearchOutcome::ok(results)
}

fn card_from_value(item: &Value) -> Option<TorrentResult> {
    let it: TpbItem = serde_json::from_value(item.clone()).ok()?;

    // JS: `it.added ? Number(it.added) * 1000 : null` —— apibay 给的是**秒**，
    // 乘 1000 变毫秒后再交给 normalize（normalize 靠量级自己判断秒/毫秒）。
    let added_ms = v2nt(&it.added)
        .and_then(|nt| to_int(Some(&nt)))
        .map(|secs| NumOrText::Num(secs.saturating_mul(1000)));

    let raw = RawResult {
        provider: "tpb".to_string(),
        id: None,
        name: v2string(&it.name),
        info_hash: v2string(&it.info_hash),
        magnet: None,
        size: v2nt(&it.size),
        seeders: v2nt(&it.seeders),
        leechers: v2nt(&it.leechers),
        date: added_ms,
        category: v2string(&it.category).and_then(|c| tpb_category(&c)),
        detail_url: v2string(&it.id)
            .map(|id| format!("https://thepiratebay.org/description.php?id={id}")),
        files: v2nt(&it.num_files),
    };

    Some(normalize(&raw))
}
