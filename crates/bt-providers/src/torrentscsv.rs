//! TorrentsCSV —— 公开的 JSON 搜索 API。
//!
//! 从 `src/providers/torrentscsv.js`（32 行）移植，对应上游 `TorrentsCSV.kt`。
//!
//! 结构最简单的一个：一层数组、字段全齐、没有分类信息（一律 `Other`）、
//! 没有详情页（磁力链由 `normalize` 从 infoHash 直接拼）。

use bt_core::http::{HttpClient, JsonResponse};
use bt_core::normalize::{encode_uri_component, normalize, RawResult};
use bt_core::TorrentResult;
use serde_json::Value;

use crate::value::{v2nt, v2nt_nonzero, v2string};
use crate::SearchOutcome;

/// 默认端点。
pub const API: &str = "https://torrents-csv.com/service/search";

/// 用默认端点搜索。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_at(http, API, query).await
}

/// 用**指定端点**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, api: &str, query: &str) -> SearchOutcome {
    let url = format!("{api}?q={}", encode_uri_component(query));

    let resp: JsonResponse<Value> = http.get_json(&url, None).await;
    if let Some(e) = &resp.error {
        return SearchOutcome::err(format!("TorrentsCSV unreachable ({e})"));
    }

    // ⚠️ 与 `knaben.rs` 同一条有意偏离：JS 遇到「200 + 非 JSON 正文」会静默返回空，
    // 这里交给上面的 `resp.error` 分支显式报错（见 `tests/torrentscsv.rs` 的 divergence 测试）。
    let Some(root) = resp.data.as_ref() else {
        return SearchOutcome::ok(Vec::new());
    };

    let torrents: &[Value] = match root.get("torrents").and_then(Value::as_array) {
        Some(v) => v,
        None => &[],
    };

    let results = torrents.iter().filter_map(card_from_value).collect();
    SearchOutcome::ok(results)
}

fn card_from_value(item: &Value) -> Option<TorrentResult> {
    let raw = RawResult {
        provider: "torrentscsv".to_string(),
        id: None,
        name: item.get("name").and_then(v2string),
        info_hash: item.get("infohash").and_then(v2string),
        magnet: None,
        size: item.get("size_bytes").and_then(v2nt),
        seeders: item.get("seeders").and_then(v2nt),
        leechers: item.get("leechers").and_then(v2nt),
        // JS: `it.created_unix ? Number(it.created_unix) : null` —— 0 是 falsy
        date: item.get("created_unix").and_then(v2nt_nonzero),
        // 这个站不提供分类，JS 里写死 'Other'
        category: Some("Other".to_string()),
        detail_url: None,
        files: None,
    };

    Some(normalize(&raw))
}
