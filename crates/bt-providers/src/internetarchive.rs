//! InternetArchive —— 走 `advancedsearch.php` 的 JSON 接口（`output=json`），不抓 HTML。
//!
//! 从 `src/providers/internetarchive.js`（58 行）移植。
//!
//! 与其他 provider 的三处不同：
//! 1. **没有 seeder/leecher** —— 它只是一个文件存档站，结果靠 DHT 而非 tracker 统计
//! 2. `docs` 不是数组时 JS 会**报错**（`no_docs`），而不是像别的 provider 那样当空结果
//! 3. `item_size` 用的是 `!= null` 判断，所以 **0 是有效值**（不是 falsy 那套）

use bt_core::http::{HttpClient, JsonResponse};
use bt_core::normalize::{encode_uri_component, normalize, NumOrText, RawResult};
use bt_core::TorrentResult;
use serde_json::Value;

use crate::value::{v2nt, v2string};
use crate::SearchOutcome;

/// 默认**基址**（不含路径）。
pub const BASE: &str = "https://archive.org";

/// mediatype → 标准桶。未知值（含大小写不符）落到 `Other`，与 JS 的 `switch` 一致。
pub fn media_type_category(media_type: Option<&str>) -> String {
    let out = match media_type {
        Some("software") => "Apps",
        Some("texts") => "Books",
        Some("movies") => "Movies",
        // JS 的 default 分支：缺失、空串、未知值都是 Other
        _ => "Other",
    };
    out.to_string()
}

/// 用默认基址搜索。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_at(http, BASE, query).await
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    // 与 JS 逐字对齐：`fl[]` 里的字段名就是服务端的契约，多一个少一个都会变
    let url = format!(
        "{base}/advancedsearch.php?q=title:{}&fl[]=title,item_size,publicdate,\
         mediatype,identifier,btih&rows=100&page=1&output=json",
        encode_uri_component(query)
    );

    let resp: JsonResponse<Value> = http.get_json(&url, None).await;
    if let Some(e) = &resp.error {
        return SearchOutcome::err(format!("internetarchive unreachable ({e})"));
    }

    // ⚠️ 有意偏离 JS（见 `tests/internetarchive.rs` 的 divergence 测试）：
    // JS 遇到「200 + 非 JSON 正文」时 `data` 是个字符串（truthy），于是走到
    // `docs` 不是数组 → 报 `no_docs`。Rust 版在上一层就报错，并附上正文开头。
    let Some(root) = resp.data.as_ref() else {
        return SearchOutcome::err("internetarchive unreachable (no_data)");
    };

    // JS: `data.response.docs` 不是数组 → **报错**（不是空结果）
    let docs: &[Value] = match root
        .get("response")
        .and_then(|r| r.get("docs"))
        .and_then(Value::as_array)
    {
        Some(v) => v,
        None => return SearchOutcome::err("no_docs"),
    };

    let results = docs.iter().filter_map(card_from_value).collect();
    SearchOutcome::ok(results)
}

fn card_from_value(item: &Value) -> Option<TorrentResult> {
    let name = item.get("title").and_then(v2string);

    // JS: `d.btih ? String(d.btih).toLowerCase().trim() : null` —— 去空白后为空也算没有
    let info_hash = item
        .get("btih")
        .and_then(v2string)
        .map(|s| s.to_lowercase().trim().to_string())
        .filter(|s| !s.is_empty());

    // 两条 `continue`
    if name.is_none() || info_hash.is_none() {
        return None;
    }

    let raw = RawResult {
        provider: "internetarchive".to_string(),
        id: None,
        name,
        info_hash,
        magnet: None,
        size: item_size(item.get("item_size")),
        // IA 不提供这两个数
        seeders: None,
        leechers: None,
        // ISO 8601 字符串，交给 normalize 的宽松日期解析
        date: item.get("publicdate").and_then(v2nt),
        category: Some(media_type_category(
            item.get("mediatype").and_then(v2string).as_deref(),
        )),
        detail_url: item
            .get("identifier")
            .and_then(v2string)
            .map(|id| format!("{BASE}/details/{id}")),
        files: None,
    };

    Some(normalize(&raw))
}

/// JS 写的是 `d.item_size != null ? Number(d.item_size) : null`。
///
/// ⚠️ 与 torrentscsv / yts 的 `x ? Number(x) : null` **不同** —— 这里 `0` 是有效值
/// （会得到 `"0 B"` 而不是 `"—"`）。所以这里不能用 `v2nt_nonzero`。
fn item_size(v: Option<&Value>) -> Option<NumOrText> {
    match v {
        None | Some(Value::Null) => None,
        // 退化路径：JS 的 `Number("")` 是 0，这里退化成"没有值"。IA 的 item_size 是数字，
        // 不会出现空串，因此不为它单列 divergence 测试。
        Some(x) => v2nt(x),
    }
}
