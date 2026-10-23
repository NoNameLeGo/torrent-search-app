//! Bangumi.Moe（`bangumi.moe`，中文/日文番剧聚合）—— **POST** 的 JSON API。
//!
//! 从 `src/providers/bangumimoe.js`（56 行）移植。
//!
//! ## 请求
//!
//! ```text
//! POST https://bangumi.moe/api/v2/torrent/search
//! Content-Type: application/json
//! {"query":"one piece"}
//! ```
//!
//! 响应 `{ torrents: [...], count, page_count, success }`，每条直接带 `magnet`。
//!
//! ## ⚠️ 字段名的大小写陷阱（照抄 JS 的后果）
//!
//! 站上返回的是 **`infoHash`**（大写 H），而 JS 读的是 **`obj.infohash`**（小写）——
//! 永远是 `undefined`，于是**总是走 `extractInfoHash(magnet)` 回退**。
//! 这里必须同样先读小写键、再回退：如果"顺手修正"成读 `infoHash`，
//! 在极少数 `magnet` 缺 btih 但 `infoHash` 有值的条目上就会与 Node 版不一致。
//!
//! ## 两处字段口径
//!
//! - `size` 是**字符串**（`"1.04 GB"`），JS 会 `.trim()` 后交给 `parseSize`；
//! - `seeders` / `leechers` 是**数字**，`0` 要保留成 `0`（不能用 `v2nt_nonzero`，
//!   那是「0 当没有」的语义）。

use bt_core::http::{HttpClient, JsonResponse};
use bt_core::normalize::{extract_info_hash, normalize, NumOrText, RawResult, TorrentResult};
use serde::Serialize;
use serde_json::Value;

use crate::value::{v2nt, v2string};
use crate::SearchOutcome;

/// 站点根（JS 的 `BASE`）。
pub const BASE: &str = "https://bangumi.moe";

/// 唯一镜像。JS 里 `DOMAINS = [BASE]`，仍走 `runMirrors`（照抄）。
const DOMAINS: &[&str] = &[BASE];

/// POST 载荷 —— 与 JS 的 `postJSON(url, { query })` 逐字一致。
#[derive(Serialize)]
struct SearchReq<'a> {
    query: &'a str,
}

/// 用默认镜像搜索。`page` 与 JS 一样**被忽略**。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_with(http, DOMAINS, query).await
}

/// 复刻 `src/lib/mirrors.js` 的 `runMirrors`。JS 传的 name 是小写 `'bangumimoe'`。
pub async fn search_with(http: &HttpClient, bases: &[&str], query: &str) -> SearchOutcome {
    crate::run_mirrors("bangumimoe", bases, |b| search_at(http, b, query)).await
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, base: &str, query: &str) -> SearchOutcome {
    let url = format!("{base}/api/v2/torrent/search");
    let payload = SearchReq { query };

    let resp: JsonResponse<Value> = http.post_json(&url, &payload, None).await;
    // JS: `if (error || !data || !Array.isArray(data.torrents)) return { error: error || 'bad_response' }`
    if let Some(e) = &resp.error {
        return SearchOutcome::err(e.clone());
    }
    let Some(root) = resp.data.as_ref() else {
        return SearchOutcome::err("bad_response");
    };
    let Some(torrents) = root.get("torrents").and_then(Value::as_array) else {
        return SearchOutcome::err("bad_response");
    };

    let results = torrents
        .iter()
        .filter_map(|t| parse_torrent(t, base))
        .collect();

    SearchOutcome::ok(results)
}

/// 解析一条；缺 title 或 magnet 就丢掉（JS 的两个 `return null`）。
fn parse_torrent(obj: &Value, base: &str) -> Option<TorrentResult> {
    let name = obj.get("title").and_then(v2string)?;
    let magnet = obj.get("magnet").and_then(v2string)?;

    // ⚠️ 先读**小写** `infohash`（站上给的是 `infoHash`，所以这里几乎总是 None），
    // 再回退到从磁力里抠 —— 与 JS 的 `obj.infohash || extractInfoHash(magnet)` 同序。
    let info_hash = obj
        .get("infohash")
        .and_then(v2string)
        .or_else(|| extract_info_hash(Some(&magnet)));

    // JS: `typeof obj.size === 'string' ? obj.size.trim() : obj.size || null`
    let size = match obj.get("size") {
        Some(Value::String(s)) => Some(NumOrText::Text(s.trim().to_string())),
        Some(v) => v2nt(v),
        None => None,
    };

    let detail_url = obj
        .get("_id")
        .and_then(v2string)
        .map(|id| format!("{base}/torrent/{id}"));

    Some(normalize(&RawResult {
        provider: "bangumimoe".to_string(),
        name: Some(name),
        magnet: Some(magnet),
        info_hash,
        size,
        // ⚠️ 用 `v2nt` 而不是 `v2nt_nonzero`：这里的 0 是**真值**
        seeders: obj.get("seeders").and_then(v2nt),
        leechers: obj.get("leechers").and_then(v2nt),
        date: obj.get("publish_time").and_then(v2nt),
        category: Some("Anime".to_string()),
        detail_url,
        ..Default::default()
    }))
}
