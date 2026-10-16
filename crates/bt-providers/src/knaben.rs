//! Knaben —— 多站点聚合索引，走官方 JSON API（POST）。
//!
//! 从 `src/providers/knaben.js`（70 行）移植。
//!
//! 它其实是「聚合器的聚合器」：一条结果可能来自 TPB、1337x、Rutracker…
//! 原始站点名在 `tracker` / `cachedOrigin` 字段里，但我们只用 `provider: "knaben"`，
//! 与 JS 版一致（引擎维度由 provider 决定，不由来源决定）。

use bt_core::http::{HttpClient, JsonResponse};
use bt_core::normalize::{extract_info_hash, normalize, RawResult};
use bt_core::TorrentResult;
use serde::Serialize;
use serde_json::Value;

use crate::value::{min_number, v2nt, v2string};
use crate::SearchOutcome;

/// 默认端点。
pub const API: &str = "https://api.knaben.org/v1";

/// 请求体。字段名必须与 JS 一致 —— 这些是服务端的 snake_case 约定。
#[derive(Debug, Serialize)]
struct KnabenRequest<'a> {
    query: &'a str,
    size: u32,
    order_by: &'a str,
    order_direction: &'a str,
    hide_unsafe: bool,
    hide_xxx: bool,
}

/// 把 Knaben 的数值分类码映射到我们的标准桶。
///
/// ⚠️ 注意 `9000000..10000000` 才是 Books —— **`8` 开头没有映射**（JS 里也是）。
/// 曾经因为这个区间不连续而漏掉过 Books，所以下面的测试单列了这条。
pub fn knaben_category(category_id: i64) -> Option<String> {
    let out = match category_id {
        n if (1_000_000..2_000_000).contains(&n) => "Music",
        n if (2_000_000..3_000_000).contains(&n) => "Series",
        n if (3_000_000..4_000_000).contains(&n) => "Movies",
        n if (4_000_000..5_000_000).contains(&n) => "Apps",
        n if (5_000_000..6_000_000).contains(&n) => "Porn",
        n if (6_000_000..7_000_000).contains(&n) => "Anime",
        n if (7_000_000..8_000_000).contains(&n) => "Games",
        n if (9_000_000..10_000_000).contains(&n) => "Books",
        n if (10_000_000..11_000_000).contains(&n) => "Other",
        _ => return None,
    };
    Some(out.to_string())
}

/// 用默认端点搜索。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_at(http, API, query).await
}

/// 用**指定端点**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, api: &str, query: &str) -> SearchOutcome {
    let payload = KnabenRequest {
        query,
        size: 300,
        order_by: "seeders",
        order_direction: "desc",
        hide_unsafe: true,
        hide_xxx: false,
    };

    // `post_json` 是 `post_json<T, B>`，T 无法从参数推出 → 标注在绑定上
    let resp: JsonResponse<Value> = http.post_json(api, &payload, None).await;
    if let Some(e) = &resp.error {
        return SearchOutcome::err(format!("Knaben unreachable ({e})"));
    }

    // ⚠️ 有意偏离 JS（见 `tests/knaben.rs` 里的 divergence 测试）：
    // JS 拿到「200 + 非 JSON 正文」时 `data` 是个字符串，`data.hits` 是 undefined，
    // 于是静默返回 `{ results: [] }` —— 被 Cloudflare 拦了却看不出任何异常。
    // 这里交给上面的 `resp.error` 分支显式报错。
    let Some(Value::Object(map)) = resp.data else {
        // 顶层不是对象（JS 同样只会得到空结果，不算错误）
        return SearchOutcome::ok(Vec::new());
    };

    let hits: &[Value] = match map.get("hits").and_then(Value::as_array) {
        Some(v) => v.as_slice(),
        None => &[],
    };

    let results = hits.iter().filter_map(card_from_value).collect();
    SearchOutcome::ok(results)
}

fn card_from_value(item: &Value) -> Option<TorrentResult> {
    let magnet = item.get("magnetUrl").and_then(v2string);
    // JS: `it.hash || extractInfoHash(magnet)` —— hash 优先，空串/缺失才回退到磁力链
    let info_hash = item
        .get("hash")
        .and_then(v2string)
        .or_else(|| extract_info_hash(magnet.as_deref()));

    // JS: `it.categoryId.map(Number).filter(!isNaN)` → `Math.min(...)`。
    // fixture 里有一条是 `[10000000, 9001000]`，取最小才是 9001000 → Books；
    // 取首元素会错判成 Other。这条专门有测试钉住。
    let category = item
        .get("categoryId")
        .and_then(min_number)
        .and_then(knaben_category);

    let raw = RawResult {
        provider: "knaben".to_string(),
        id: None,
        name: item.get("title").and_then(v2string),
        info_hash,
        magnet,
        size: item.get("bytes").and_then(v2nt),
        seeders: item.get("seeders").and_then(v2nt),
        // knaben 的 "peers" 就是 JS 版的 leechers
        leechers: item.get("peers").and_then(v2nt),
        // ISO 8601 字符串，直接交给 normalize 的宽松日期解析
        date: item.get("date").and_then(v2nt),
        category,
        detail_url: item.get("details").and_then(v2string),
        files: None,
    };

    Some(normalize(&raw))
}
