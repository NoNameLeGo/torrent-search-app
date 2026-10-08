//! YTS（Yify）—— 电影资源，走公开的 JSON API。
//!
//! 从 `src/providers/yts.js`（55 行）移植，对应上游 `Yts.kt`。
//!
//! 形状和别的 provider 不同：响应是 **电影 → 多个种子** 的两层结构，
//! 所以一个电影会展开成多条结果，`name` 由电影名 + 画质/类型/编码拼出来。
//! 另外它是本项目里少数**把 infoHash 小写化**的 provider（别的站点原样保留大小写）。

use bt_core::http::{HttpClient, JsonResponse};
use bt_core::normalize::{encode_uri_component, normalize, RawResult};
use serde_json::Value;

use crate::value::{v2nt, v2nt_nonzero, v2string};
use crate::SearchOutcome;

/// 默认**基址**（不含路径）—— 与 JS 的 `API` 常量一致。
pub const API: &str = "https://movies-api.accel.li/api/v2";

/// 用默认端点搜索。
pub async fn search(http: &HttpClient, query: &str, _page: u32) -> SearchOutcome {
    search_at(http, API, query).await
}

/// 用**指定基址**搜索（供离线测试指向本地一次性 HTTP 服务）。
pub async fn search_at(http: &HttpClient, api: &str, query: &str) -> SearchOutcome {
    let url = format!(
        "{api}/list_movies.json?query_term={}&limit=50",
        encode_uri_component(query)
    );

    let resp: JsonResponse<Value> = http.get_json(&url, None).await;
    if let Some(e) = &resp.error {
        return SearchOutcome::err(format!("YTS unreachable ({e})"));
    }

    // ⚠️ 与 `knaben.rs` 同一条有意偏离：JS 遇到「200 + 非 JSON 正文」会静默返回空，
    // 这里交给上面的 `resp.error` 分支显式报错（见 `tests/yts.rs` 的 divergence 测试）。
    let Some(root) = resp.data.as_ref() else {
        return SearchOutcome::ok(Vec::new());
    };

    // JS: `data && data.data && Array.isArray(data.data.movies) ? ... : []`
    let movies: &[Value] = match root
        .get("data")
        .and_then(|d| d.get("movies"))
        .and_then(Value::as_array)
    {
        Some(v) => v,
        None => &[],
    };

    let mut results = Vec::new();
    for movie in movies {
        // JS: `movie.title_long || movie.title`，两个都没有就跳过这一部
        let Some(title) = movie
            .get("title_long")
            .and_then(v2string)
            .or_else(|| movie.get("title").and_then(v2string))
        else {
            continue;
        };

        let detail_base = movie.get("url").and_then(v2string);
        // JS 会把缺失的 id 拼成字面量 "undefined"；这里退化成空串。
        // yts 实际总会给 id，属于纯退化路径，不为它单列 divergence 测试。
        let movie_id = movie.get("id").and_then(v2string).unwrap_or_default();

        let torrents: &[Value] = match movie.get("torrents").and_then(Value::as_array) {
            Some(v) => v,
            None => &[],
        };

        for t in torrents {
            // JS: `(t.hash || '').toLowerCase()` —— 空哈希的条目直接丢弃
            let Some(info_hash) = t.get("hash").and_then(v2string).map(|h| h.to_lowercase()) else {
                continue;
            };

            let quality = or_dash(t.get("quality"));
            let kind = or_dash(t.get("type"));
            let codec = or_dash(t.get("video_codec"));
            let name = format!("{title} [{quality}] [{kind}] [{codec}]");

            let detail_url = detail_base
                .as_ref()
                .map(|b| format!("{b}?movieid={movie_id}&infohash={info_hash}"));

            results.push(normalize(&RawResult {
                provider: "yts".to_string(),
                id: None,
                name: Some(name),
                info_hash: Some(info_hash),
                magnet: None,
                size: t.get("size_bytes").and_then(v2nt),
                seeders: t.get("seeds").and_then(v2nt),
                leechers: t.get("peers").and_then(v2nt),
                // JS: `t.date_uploaded_unix ? Number(t.date_uploaded_unix) : null`
                date: t.get("date_uploaded_unix").and_then(v2nt_nonzero),
                category: Some("Movies".to_string()),
                detail_url,
                files: None,
            }));
        }
    }

    SearchOutcome::ok(results)
}

/// JS 的 `t.quality || '-'` —— 缺失和空串都退化成 `-`。
fn or_dash(v: Option<&Value>) -> String {
    v.and_then(v2string).unwrap_or_else(|| "-".to_string())
}
